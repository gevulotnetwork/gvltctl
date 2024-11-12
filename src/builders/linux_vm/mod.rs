//! Linux VM builder.

use std::path::PathBuf;

use crate::builders::{Context, Pipeline, Steps};

mod filesystem;
mod image_file;
mod kernel;
// mod loopdev;
mod mbr;
// mod mount;
mod rootfs;
#[cfg(feature = "syslinux")]
mod syslinux;
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

pub fn build(ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
    let mut steps: Steps<LinuxVMBuildContext> = Vec::new();
    dbg!(ctx.opts());

    if ctx.opts().rootfs_dir.is_some() {
        steps.push(Box::new(rootfs::RootFSFromDir));
    }

    if ctx.opts().kernel_file.is_some() {
        steps.push(Box::new(kernel::Precompiled));
    } else {
        steps.push(Box::new(kernel::Build));
    }

    // These steps may be replaced with pre-built image
    steps.push(Box::new(image_file::CreateImageFile));
    steps.push(Box::new(mbr::CreateMBR));
    steps.push(Box::new(filesystem::CreateFat));
    steps.push(Box::new(syslinux::InstallSyslinux));

    steps.push(Box::new(kernel::Install));
    steps.push(Box::new(syslinux::InstallSyslinuxCfg));

    steps.push(Box::new(rootfs::InstallRootFS));

    // steps.push(Box::new(loopdev::AttachLoopDevice));
    // steps.push(Box::new(mount::Mount));

    let mut pipeline = Pipeline::from_ctx(ctx);
    pipeline.add_steps(steps);

    // ctrlc::set_handler(|| println!("ctrl+c applied")).context("setup interruptions handler")?;
    pipeline.run()
}
