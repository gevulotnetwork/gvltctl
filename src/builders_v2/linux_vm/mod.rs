//! Linux VM builder.

use anyhow::{Context as _, Result};
use directories::ProjectDirs;
use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

use crate::builders::{Context, Pipeline, Steps};

mod container;
mod directory;
mod filesystem;
mod gevulot_runtime;
mod image_file;
mod kernel;
mod mbr;
mod mia;
mod mount;
mod nvidia;
mod rootfs;
mod syslinux;
mod utils;

/// Image file options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFileOpts {
    /// Output file with VM image.
    pub path: PathBuf,

    /// Image size.
    ///
    /// If the size is `Some(_)`, image file must be of this size and cannot be resized.
    pub size: Option<u64>,

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

/// Filesystem to use for root filesystem in the VM.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RootFsOpts {
    /// SquashFS
    ///
    /// This filesystem will be written directly without mounting,
    /// so there is no mount options.
    SquashFs,

    /// EXT4
    Ext4 {
        /// Mounting options.
        mount_type: MountType,
    },
}

/// Mounting options for target VM image.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

    /// Root filesystem options.
    pub root_fs_opts: RootFsOpts,

    /// Install NVidia drivers.
    pub nvidia_drivers: bool,

    /// Init system options.
    pub init_system_opts: InitSystemOpts,

    /// Filesystem source to use.
    pub fs_source: FilesystemSource,

    /// Create image from scratch (don't use base VM image).
    pub from_scratch: bool,

    /// MBR bootcode file.
    pub mbr_file: Option<PathBuf>,

    /// Mount root filesystem as read-write.
    pub rw_root: bool,

    /// Generate only base (template) of VM image.
    ///
    /// This image will include bootloader, partition table and a single partition with filesystem.
    /// Size of this image is the smallest possible.
    pub gen_base_img: bool,
}

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

        let tmp = TempDir::new("linux-vm-build").context("failed to create temporary directory")?;
        log::debug!("temp directory: {} (removed on exit)", tmp.path().display());
        ctx.set("tmp", Box::new(tmp));

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
        log::debug!("cache directory: {}", cache_path.display());
        ctx.set("cache", Box::new(cache_path));
        Ok(Self(ctx))
    }

    /// Get build options.
    pub fn opts(&self) -> &BuildOpts {
        self.get("opts")
            .expect("internal error: build options must always be in Linux VM build context")
    }

    /// Get path to temporary directory, which will be cleaned at the end.
    pub fn tmp(&self) -> &Path {
        self.get::<TempDir>("tmp")
            .expect("internal error: temporary directory must always be in Linux VM build context")
            .path()
    }

    /// Path to cache directory.
    pub fn cache(&self) -> &Path {
        self.get::<PathBuf>("cache")
            .expect("internal error: cache directory must always be in Linux VM build context")
            .as_path()
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
    #[allow(unused)]
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

/// This image contains:
///  - msdos partition table
///  - mbr bootcode
///  - bootloader (syslinux)
///  - partition p1
///  - fat32 filesystem on p1
///
/// This image was created using `--generate-base-image` option.
///
/// Unfortunatelly this solution increases gvltctl executable size by ~20MB.
pub const BASE_IMAGE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders_v2/linux_vm/data/base.img"
));

/// Setup pipeline steps depending on the context.
fn setup_pipeline(ctx: &mut LinuxVMBuildContext) -> Pipeline<LinuxVMBuildContext> {
    if ctx.opts().gen_base_img {
        return Pipeline::from_steps(ctx, setup_base_image_steps());
    }

    let mut steps: Steps<_> = Vec::new();

    // TODO: build artifacts should go to cache
    match &ctx.opts().kernel_opts {
        KernelOpts::Precompiled { file } => {
            steps.push(Box::new(kernel::Precompiled::new(file.clone())));
        }
        KernelOpts::Source {
            version,
            repository_url,
        } => {
            steps.push(Box::new(kernel::Build::new(
                repository_url.clone(),
                version.clone(),
            )));
        }
    }

    // Define source filesystem
    steps.push(Box::new(rootfs::Init));

    match &ctx.opts().fs_source {
        FilesystemSource::Dir(path) => {
            steps.push(Box::new(rootfs::CopyExisting::new(path.clone())));
        }
        FilesystemSource::Image { reference, backend } => {
            // Directly set container image here (no additional steps needed)
            let image = container::ContainerImage::new(*backend, reference.clone(), false);
            ctx.set("container-image", Box::new(image));

            steps.push(Box::new(container::GetContainerRuntime));
            steps.push(Box::new(container::ExportFilesystem));
        }
        FilesystemSource::Containerfile { file, backend } => {
            steps.push(Box::new(container::BuildContainerImage::new(
                *backend,
                file.clone(),
            )));
            steps.push(Box::new(container::GetContainerRuntime));
            steps.push(Box::new(container::ExportFilesystem));
        }
    }

    // Prepare NVidia drivers
    // TODO: build artifacts should go to cache
    if ctx.opts().nvidia_drivers {
        steps.push(Box::new(nvidia::BuildDrivers));
    }

    // Get VM image with partitions and boot filesystem
    if ctx.opts().from_scratch {
        steps.append(&mut setup_base_image_steps());
    } else {
        steps.push(Box::new(image_file::UseImageFile));
        steps.push(Box::new(mbr::ReadMBR));
        steps.push(Box::new(mbr::ReadBootPartition));
        steps.push(Box::new(filesystem::ReadBootFs));
    }

    // Install kernel into boot filesystem.
    steps.push(Box::new(kernel::Install));

    // Install NVIDIA drivers into root filesystem.
    if ctx.opts().nvidia_drivers {
        steps.push(Box::new(nvidia::InstallDrivers));
    }

    // Install MIA into root filesystem.
    if let InitSystemOpts::Mia {
        mia_version,
        mounts,
        default_mounts,
        kernel_modules,
        gevulot_runtime,
    } = &ctx.opts().init_system_opts
    {
        if *gevulot_runtime {
            steps.push(Box::new(gevulot_runtime::CreateGevulotRuntimeDirs));
        }
        steps.push(Box::new(mia::InstallMia::new(
            mia_version.clone(),
            *gevulot_runtime,
            kernel_modules.clone(),
            mounts.clone(),
            *default_mounts,
        )));
    }

    match &ctx.opts().root_fs_opts {
        RootFsOpts::SquashFs => {
            steps.push(Box::new(filesystem::squashfs::Format));
            steps.push(Box::new(rootfs::InstallToSquashFs));
            steps.push(Box::new(filesystem::squashfs::EvaluateSize));
        }
        RootFsOpts::Ext4 { .. } => {
            steps.push(Box::new(filesystem::ext4::EvaluateSize));
        }
    }

    steps.push(Box::new(mbr::CreateRootPartition));

    match ctx.opts().root_fs_opts {
        RootFsOpts::SquashFs => {
            steps.push(Box::new(filesystem::squashfs::WriteSquashFs));
        }
        RootFsOpts::Ext4 { mount_type } => {
            steps.push(Box::new(filesystem::ext4::Format));
            match mount_type {
                MountType::Fuse => {
                    steps.push(Box::new(mount::fuse::MountFileSystem(
                        "root-partition-number",
                    )));
                }
                MountType::Native => {
                    steps.push(Box::new(mount::native::MountFileSystem(
                        "root-partition-number",
                    )));
                }
            }
            steps.push(Box::new(rootfs::InstallToMount));
        }
    }

    // Install SYSLINUX configuration
    steps.push(Box::new(syslinux::InstallCfg));

    Pipeline::from_steps(ctx, steps)
}

/// Set up pipeline for building base image.
fn setup_base_image_steps() -> Steps<LinuxVMBuildContext> {
    let mut steps: Steps<_> = Vec::new();

    steps.push(Box::new(image_file::CreateImageFile));
    steps.push(Box::new(mbr::CreateMBR));
    // FIXME: there is an issue with resizing existing FAT32 filesystem and not
    // loosing files and VBR written by SYSLINUX.
    // Because of that it is impossible right now to format a smaller filesystem
    // only for bootloader and then resize it before writing the kernel.
    // To solve this we over-allocate 20 MiB (40960 sectors) for now, which should be enough
    // for most of the kernels.
    // If you are facing a panic on kernel installation step ("FAT32 resizing"),
    // increase this amount here.
    // Ideally we would like to only allocate the space for bootloader and
    // filesystem metadata here.
    steps.push(Box::new(mbr::CreateBootPartition::new(40960)));
    steps.push(Box::new(filesystem::CreateBootFs));
    steps.push(Box::new(syslinux::Install));

    steps
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
