//! Linux VM builder.

use anyhow::{Context as _, Result};
use bytesize::ByteSize;
use directories::ProjectDirs;
use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

use crate::builders::{Context, Pipeline, Steps};

mod container;
mod extlinux;
mod filesystem;
mod image_file;
mod mbr;
mod mount;
mod rootfs;
mod utils;

/// Image file options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFileOpts {
    /// Output file with VM image.
    pub path: PathBuf,

    /// Image size.
    pub size: u64,

    /// Overwrite existing image file.
    pub force: bool,
}

/// Container backend (docker or podman).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerBackend {
    Podman,
    Docker,
}

impl ContainerBackend {
    /// Executable name.
    pub fn exe(&self) -> &'static str {
        match self {
            Self::Podman => "podman",
            Self::Docker => "docker",
        }
    }
}

/// Used-defined kernel options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelOpts {
    /// Kernel is pre-compiled.
    Precompiled {
        /// Path to pre-compiled kernel.
        file: PathBuf,
    },

    /// Kernel is cloned from git repository and compiled from sources.
    Source {
        /// Kernel version, e.g. `v6.12`.
        version: String,

        /// Kernel URL to clone sources from.
        repository_url: String,
    },
}

/// Mounting options for target VM image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountType {
    /// Use native OS mount.
    ///
    /// This will probably require root permissions.
    Native,

    /// Use FUSE to mount target disk image.
    Fuse,
}

/// Describes how to obtain filesystem for the VM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilesystemSource {
    /// Copy files from specified directory.
    Dir(PathBuf),

    /// Create filesystem from container image (with given ref).
    Image {
        /// Image reference.
        reference: String,

        /// Container backend to use.
        backend: ContainerBackend,
    },

    /// Build container image using given Containerfile/Dockerfile and the use its filesystem.
    Containerfile {
        /// Path to Containerfile/Dockerfile.
        file: PathBuf,

        /// Container backend to use.
        backend: ContainerBackend,
    },
}

/// Init system options (MIA, systemd etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitSystemOpts {
    /// Use MIA as init process.
    Mia {
        /// MIA version to install.
        mia_version: String,

        /// Filesystems to mount as startup.
        mounts: Vec<String>,

        /// Create default mounts (like `/proc`).
        default_mounts: bool,

        /// Kernel modules to load at startup.
        kernel_modules: Vec<String>,

        /// Create Gevulot-compatible VM (create required directories and configs).
        gevulot_runtime: bool,
    },

    /// Use custom init process.
    ///
    /// When using custom init, user will have to meet Gevulot-compatible VM spec himself.
    Custom {
        /// Init executable.
        init: String,

        /// Init executable arguments.
        init_args: Option<String>,
    },
}

/// User-defined build options.
///
/// These options should be used as read-only during build.
/// Mutable variables are stored in [`LinuxVMBuildContext`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildOpts {
    /// Image file options.
    pub image_file_opts: ImageFileOpts,

    /// Linux kernel options.
    pub kernel_opts: KernelOpts,

    /// Type of the mount for target VM image.
    pub mount_type: MountType,

    /// Install NVidia drivers.
    pub nvidia_drivers: bool,

    /// Init system options.
    pub init_system_opts: InitSystemOpts,

    /// Filesystem source to use.
    pub fs_source: FilesystemSource,

    /// Create image from scratch (don't use base VM image).
    pub from_scratch: bool,

    /// Mount root filesystem as read-write.
    pub rw_root: bool,

    /// Generate only base (template) of VM image.
    ///
    /// This image will include bootloader, partition table and a single partition with filesystem.
    /// Size of this image is the smallest possible.
    pub gen_base_img: bool,
}

// This size is chosen 9MiB to avoid `Filesystem too small for a journal` when creating EXT4 fs.
// Since kernel size is approximately 12-14 MB, it will almost never be shrinked.
/// Minimal initial image size.
/// Used when image size is not specified implicitly.
pub const MIN_IMAGE_SIZE: ByteSize = ByteSize::mib(9);

/// Linux VM build context.
///
/// To create context, use [`LinuxVMBuildContext::from_opts()`].
///
/// Expansion of [`Context`].
pub struct LinuxVMBuildContext(Context);

impl LinuxVMBuildContext {
    /// Create empty context from user-defined build options.
    ///
    /// Set following context fields:
    /// - `opts` - copy of build options
    /// - `tmp` - general temporary directory
    /// - `cache` - general cache directory (for Linux builds and etc.)
    pub fn from_opts(opts: BuildOpts) -> Result<Self> {
        let mut ctx = Context::new();
        ctx.set("opts", Box::new(opts));
        ctx.set(
            "tmp",
            Box::new(
                TempDir::new("linux-vm-build").context("failed to create temporary directory")?,
            ),
        );
        let project_dirs = ProjectDirs::from("", "gevulot", "gvltctl");
        // Normally it will be `$HOME/.cache/gvltctl` on Linux
        //  or `$HOME/Library/Caches/gevulot.gvltctl` on MacOS
        let cache_path = project_dirs
            .map(|dirs| dirs.cache_dir().to_path_buf())
            .unwrap_or(PathBuf::from(".cache"));
        if !cache_path.is_dir() {
            fs::create_dir_all(&cache_path).context(format!(
                "failed to create cache directory: {}",
                cache_path.display()
            ))?;
        }
        ctx.set("cache", Box::new(cache_path));
        Ok(Self(ctx))
    }

    /// Get build options.
    pub fn opts(&self) -> &BuildOpts {
        self.get("opts")
            .expect("internal error: build options must always be in Linux VM build context")
    }

    /// Get path to temporary directory, which will be cleaned at the end.
    pub fn tmpdir(&self) -> &Path {
        self.get::<TempDir>("tmpdir")
            .expect("internal error: temporary directory must always be in Linux VM build context")
            .path()
    }

    /// Get reference to value by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn get<T>(&self, key: &'static str) -> Option<&T>
    where
        T: 'static,
    {
        self.0.get(key)
    }

    /// Get mutable reference to value by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn get_mut<T>(&mut self, key: &'static str) -> Option<&mut T>
    where
        T: 'static,
    {
        self.0.get_mut(key)
    }

    /// Pop value from context by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn pop<T>(&mut self, key: &'static str) -> Option<Box<T>>
    where
        T: 'static,
    {
        self.0.pop(key)
    }

    /// Set value for the key.
    pub fn set(&mut self, key: &'static str, value: Box<dyn Any>) {
        self.0.set(key, value);
    }
}

/// This error should be treated as internal builder error.
#[derive(thiserror::Error, Debug)]
pub enum LinuxVMBuilderError {
    /// Cannot perform action because required element in the context wasn't found.
    #[error("internal builder error: cannot {action:?}: {context_elem:?} not found")]
    InvalidContext {
        action: String,
        context_elem: String,
    },
}

impl LinuxVMBuilderError {
    pub fn invalid_context(action: &str, context_elem: &str) -> Self {
        Self::InvalidContext {
            action: action.to_string(),
            context_elem: context_elem.to_string(),
        }
    }
}

// FIXME: fix this, because gvltctl cannot be distributed as self-contained binary this way.
/// This image contains:
///  - msdos partition table
///  - mbr bootcode
///  - bootloader (syslinux)
///  - partition p1
///  - ext4 filesystem
///
/// This image was created using `--generate-base-image` option.
const BASE_IMAGE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/base.img"
);

/// Setup pipeline steps depending on the context.
fn setup_pipeline(ctx: &mut LinuxVMBuildContext) -> Pipeline<LinuxVMBuildContext> {
    if ctx.opts().gen_base_img {
        return setup_base_image_pipeline(ctx);
    }

    let mut steps: Steps<_> = Vec::new();

    // Define source filesystem
    match &ctx.opts().fs_source {
        FilesystemSource::Dir(path) => {
            steps.push(Box::new(rootfs::RootFSFromDir::new(path.clone())));
        }
        FilesystemSource::Image { reference, backend } => {
            steps.push(Box::new(rootfs::RootFSEmpty));

            let image = container::ContainerImage::new(*backend, reference.clone(), false);
            ctx.0.set("container-image", Box::new(image));
            steps.push(Box::new(container::GetContainerRuntime));
            steps.push(Box::new(container::CopyFilesystem));
        }
        FilesystemSource::Containerfile { file, backend } => {
            steps.push(Box::new(rootfs::RootFSEmpty));

            steps.push(Box::new(container::BuildContainerImage::new(
                *backend,
                file.clone(),
            )));
            steps.push(Box::new(container::GetContainerRuntime));
            steps.push(Box::new(container::CopyFilesystem));
        }
    }

    if ctx.opts().from_scratch {
        steps.push(Box::new(image_file::CreateImageFile));
        steps.push(Box::new(mbr::CreateMBR));
        steps.push(Box::new(filesystem::Create));
    } else {
        steps.push(Box::new(image_file::UseImageFile::new(BASE_IMAGE_PATH)));
        steps.push(Box::new(mbr::ReadMBR));
        steps.push(Box::new(filesystem::UseExisting));
    }

    match ctx.opts().mount_type {
        MountType::Fuse => {
            steps.push(Box::new(mount::fuse::MountFileSystem));
        }
        MountType::Native => {
            steps.push(Box::new(mount::native::MountFileSystem));
        }
    }

    // EXTLINUX is installed on mounted filesystem.
    // It doesn't work with FUSE mounts, that's why --from-scratch implies --no-fuse.
    if ctx.opts().from_scratch {
        steps.push(Box::new(extlinux::InstallExtlinux));
    }

    steps.push(Box::new(rootfs::InstallRootFS));
    steps.push(Box::new(extlinux::InstallExtlinuxCfg));

    Pipeline::from_steps(ctx, steps)
}

fn setup_base_image_pipeline(ctx: &mut LinuxVMBuildContext) -> Pipeline<LinuxVMBuildContext> {
    let steps: Steps<_> = vec![
        Box::new(image_file::CreateImageFile),
        Box::new(mbr::CreateMBR),
        Box::new(filesystem::Create),
        Box::new(mount::native::MountFileSystem),
        Box::new(extlinux::InstallExtlinux),
    ];
    Pipeline::from_steps(ctx, steps)
}

pub fn build(ctx: &mut LinuxVMBuildContext) -> Result<serde_json::Value> {
    let pipeline = setup_pipeline(ctx);

    // TODO: add interruptions handler:
    // ctrlc::set_handler(|| println!("ctrl+c applied")).context("setup interruptions handler")?;

    pipeline.run().context("Linux VM build failed")?;

    Ok(serde_json::json!({
        "message": format!("{}", ctx.opts().image_file_opts.path.display()),
        "image": &ctx.opts().image_file_opts.path,
    }))
}
