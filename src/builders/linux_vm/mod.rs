//! Linux VM builder.

use std::path::PathBuf;

use crate::builders::{Context, Pipeline, Steps};

mod filesystem;
mod image_file;
mod kernel;
mod mbr;
mod mount;
mod rootfs;
mod extlinux;
mod utils;

/// User-defined build options.
#[derive(Debug)]
pub struct BuildOpts {
    /// Output file with VM image.
    pub image_path: PathBuf,

    /// Overwrite existing image file.
    pub force: bool,

    /// Image size.
    pub image_size: u32,

    /// Kernel version, e.g. `v6.10.11`.
    pub kernel_version: String,

    /// Path to pre-compiled kernel.
    pub kernel_file: Option<PathBuf>,

    /// Kernel URL to clone sources from.
    pub kernel_url: String,

    /// Root filesystem to install.
    pub rootfs_dir: Option<PathBuf>,

    pub from_scratch: bool,
}

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

pub fn build(ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
    let mut steps: Steps<LinuxVMBuildContext> = Vec::new();

    if let Some(path) = &ctx.opts().rootfs_dir {
        steps.push(Box::new(rootfs::RootFSFromDir::new(path.clone())));
    }

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
        steps.push(Box::new(extlinux::InstallExtlinux));
    } else {
        // Using pre-built base image
        steps.push(Box::new(image_file::UseImageFile::new(BASE_IMAGE_PATH)));
        steps.push(Box::new(mbr::ReadMBR));
        steps.push(Box::new(filesystem::Read));
    }

    cfg_if::cfg_if! {
        if #[cfg(feature = "fuse")] {
            steps.push(Box::new(mount::MountFileSystem));
        } else {
            steps.push(Box::new(loopdev::AttachLoopDevice));
            steps.push(Box::new(mount::Mount));
        }
    }

    steps.push(Box::new(kernel::Install));
    steps.push(Box::new(rootfs::InstallRootFS));
    steps.push(Box::new(extlinux::InstallExtlinuxCfg));

    let mut pipeline = Pipeline::from_ctx(ctx);
    pipeline.add_steps(steps);

    // ctrlc::set_handler(|| println!("ctrl+c applied")).context("setup interruptions handler")?;
    pipeline.run()
}
