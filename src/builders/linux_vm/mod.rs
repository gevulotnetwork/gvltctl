//! Linux VM builder.

use anyhow::{Context as _, Result};
use bytesize::ByteSize;
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

use crate::builders::{Context, Pipeline, Steps};

mod container;
mod extlinux;
mod filesystem;
mod gevulot_runtime;
mod image_file;
mod kernel;
mod mbr;
mod mia;
mod mount;
mod nvidia;
mod resize;
mod rootfs;
mod utils;

/// Describes how to obtain filesystem for the VM.
#[derive(Clone, Debug)]
pub enum FilesystemSource {
    /// Copy files from specified directory.
    Dir(PathBuf),

    /// Create filesystem from container image (with given ref).
    Image(String),

    /// Build container image using given Containerfile/Dockerfile and the use its filesystem.
    Containerfile(PathBuf),
}

/// User-defined build options.
#[derive(Clone, Debug)]
pub struct BuildOpts {
    /// Output file with VM image.
    pub image_path: PathBuf,

    /// Overwrite existing image file.
    pub force: bool,

    /// Use FUSE to mount target disk image.
    ///
    /// If not set, native OS mount will be used.
    pub fuse: bool,

    /// Image size.
    pub image_size: ByteSize,

    /// Kernel version, e.g. `v6.10.11`.
    pub kernel_version: String,

    /// Path to pre-compiled kernel.
    pub kernel_file: Option<PathBuf>,

    /// Kernel URL to clone sources from.
    pub kernel_url: String,

    /// Install NVidia drivers.
    pub nvidia_drivers: bool,

    /// Container backend to use.
    pub container_backend: container::Backend,

    /// Filesystem source to use.
    pub fs_source: FilesystemSource,

    /// Create Gevulot-compatible VM (create required directories and configs).
    pub gevulot_runtime: bool,

    /// MIA version to install.
    pub mia_version: String,

    /// Mounts.
    pub mounts: Vec<String>,

    /// Create default mounts.
    pub default_mounts: bool,

    /// Init executable.
    pub init: Option<String>,
    pub init_args: Option<String>,

    /// Create image from scratch (don't use base VM image).
    pub from_scratch: bool,

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
pub struct LinuxVMBuildContext(pub Context);

impl LinuxVMBuildContext {
    /// Create empty context from user-defined build options.
    ///
    /// Set following context fields:
    /// - `opts` - copy of build options
    /// - `tmpdir` - general temporary directory
    /// - `cache` - general cache directory (for Linux builds and etc.)
    pub fn from_opts(opts: BuildOpts) -> Result<Self> {
        let mut ctx = Context::new();
        ctx.set("opts", Box::new(opts));
        ctx.set(
            "tmpdir",
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
        self.0
            .get("opts")
            .expect("internal error: build options must always be in Linux VM build context")
    }

    /// Get path to temporary directory, which will be cleaned at the end.
    pub fn tmpdir(&self) -> &Path {
        self.0
            .get::<TempDir>("tmpdir")
            .expect("internal error: temporary directory must always be in Linux VM build context")
            .path()
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

pub fn build(ctx: &mut LinuxVMBuildContext) -> Result<()> {
    let pipeline = setup_pipeline(ctx);
    // TODO: add interruptions handler:
    // ctrlc::set_handler(|| println!("ctrl+c applied")).context("setup interruptions handler")?;
    pipeline.run().context("build pipeline failed")
}

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
        FilesystemSource::Image(reference) => {
            steps.push(Box::new(rootfs::RootFSEmpty));

            let image = container::ContainerImage::new(
                ctx.opts().container_backend,
                reference.clone(),
                false,
            );
            ctx.0.set("container-image", Box::new(image));
            steps.push(Box::new(container::CopyFilesystem));
        }
        FilesystemSource::Containerfile(path) => {
            steps.push(Box::new(rootfs::RootFSEmpty));

            steps.push(Box::new(container::BuildContainerImage::new(
                ctx.opts().container_backend,
                path.clone(),
            )));
            steps.push(Box::new(container::CopyFilesystem));
        }
    }

    // Prepare Linux kernel
    if ctx.opts().kernel_file.is_some() {
        steps.push(Box::new(kernel::Precompiled));
    } else {
        steps.push(Box::new(kernel::Build));
    }

    // Prepare NVidia drivers
    if ctx.opts().nvidia_drivers {
        steps.push(Box::new(nvidia::BuildDrivers));
    }

    if ctx.opts().from_scratch {
        // Creating bootable image from scratch
        steps.push(Box::new(image_file::CreateImageFile));
        steps.push(Box::new(mbr::CreateMBR));
        steps.push(Box::new(filesystem::Create));
    } else {
        // Using pre-built base image
        steps.push(Box::new(image_file::UseImageFile::new(BASE_IMAGE_PATH)));
        steps.push(Box::new(mbr::ReadMBR));
        steps.push(Box::new(filesystem::UseExisting));
    }

    // Resize VM image before filling it with the content if needed.
    // IMPORTANT: all the artifacts must be generated before this step to correctly get their sizes.
    steps.push(Box::new(resize::ResizeAll::<filesystem::Ext4>::new()));

    if ctx.opts().fuse {
        steps.push(Box::new(mount::fuse::MountFileSystem));
    } else {
        steps.push(Box::new(mount::native::MountFileSystem));
    }

    // EXTLINUX is installed on mounted filesystem.
    // It doesn't work with FUSE mounts, that's why --from-scratch implies --no-fuse.
    if ctx.opts().from_scratch {
        steps.push(Box::new(extlinux::InstallExtlinux));
    }

    steps.push(Box::new(kernel::Install));
    steps.push(Box::new(rootfs::InstallRootFS));
    steps.push(Box::new(extlinux::InstallExtlinuxCfg));

    if ctx.opts().nvidia_drivers {
        steps.push(Box::new(nvidia::InstallDrivers));
    }

    if ctx.opts().gevulot_runtime {
        steps.push(Box::new(gevulot_runtime::CreateGevulotRuntimeDirs));
    }

    if ctx.opts().init.is_none() {
        // FIXME: this step will fail if using `FilesystemSource::Dir`
        steps.push(Box::new(container::GetContainerRuntime));

        steps.push(Box::new(mia::InstallMia::new(
            ctx.opts().mia_version.clone(),
            ctx.opts().gevulot_runtime,
            ctx.opts().mounts.clone(),
            ctx.opts().default_mounts,
        )));
    }

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
