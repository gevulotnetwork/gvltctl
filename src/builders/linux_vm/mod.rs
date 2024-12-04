//! Linux VM builder.

use anyhow::{Context as _, Result};
use bytesize::ByteSize;
use std::path::PathBuf;

use crate::builders::{Context, Pipeline, Steps};

mod extlinux;
mod filesystem;
mod image_file;
mod kernel;
mod mbr;
mod mount;
mod resize;
mod rootfs;
mod utils;

/// User-defined build options.
#[derive(Debug)]
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

    /// Root filesystem to install.
    pub rootfs_dir: Option<PathBuf>,

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
    pub fn from_opts(opts: BuildOpts) -> Self {
        let mut ctx = Context::new();
        ctx.set("opts", Box::new(opts));
        Self(ctx)
    }

    /// Get build options.
    pub fn opts(&self) -> &BuildOpts {
        self.0
            .get("opts")
            .expect("internal error: build options must always be in Linux VM build context")
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

// TODO: fix this, because gvltctl cannot be distributed as self-contained binary this way.
/// This image contains:
///  - msdos partition table
///  - mbr bootcode
///  - bootloader (syslinux)
///  - partition p1
///  - ext4 filesystem
const BASE_IMAGE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/base.img"
);

pub fn build(ctx: &mut LinuxVMBuildContext) -> Result<()> {
    let pipeline = setup_pipeline(ctx);
    // ctrlc::set_handler(|| println!("ctrl+c applied")).context("setup interruptions handler")?;
    pipeline.run().context("build pipeline failed")
}

/// Setup pipeline steps depending on the context.
fn setup_pipeline(ctx: &mut LinuxVMBuildContext) -> Pipeline<LinuxVMBuildContext> {
    if ctx.opts().gen_base_img {
        return setup_base_image_pipeline(ctx);
    }

    let mut steps: Steps<_> = Vec::new();

    if let Some(path) = &ctx.opts().rootfs_dir {
        steps.push(Box::new(rootfs::RootFSFromDir::new(path.clone())));
    }
    // TODO: add steps for container-based rootfs

    if ctx.opts().kernel_file.is_some() {
        steps.push(Box::new(kernel::Precompiled));
    } else {
        steps.push(Box::new(kernel::Build));
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
        steps.push(Box::new(filesystem::Check));
    }

    // Resize VM image before filling it with the content if needed.
    steps.push(Box::new(resize::ResizeAll));

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
