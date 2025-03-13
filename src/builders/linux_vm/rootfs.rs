//! Root filesystem handling.
//!
//! Main type is [`RootFS`], which represents filesystem source files to pack into VM.

use anyhow::{Context, Result};
use bytesize::ByteSize;
use log::{debug, info};
use std::path::PathBuf;

use crate::builders::linux_vm::directory::Directory;
use crate::builders::linux_vm::filesystem::squashfs::SquashFs;
use crate::builders::Step;

use super::LinuxVMBuildContext;

/// Create local temp directory for root filesystem.
///
/// # Context variables defined
/// - `root-fs`: [`PathBuf`]
pub struct Init;

impl Step<LinuxVMBuildContext> for Init {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("setting empty root filesystem");
        let path = ctx.tmp().join("rootfs");
        std::fs::create_dir(&path)
            .context("failed to create temp directory for root filesystem")?;
        debug!("root filesystem set: {}", path.display(),);
        ctx.set("root-fs", Box::new(path));
        Ok(())
    }
}

/// Copy root filesystem from given path.
///
/// # Context variables required
/// - `root-fs`: [`PathBuf`]
pub struct CopyExisting {
    path: PathBuf,
}

impl CopyExisting {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Step<LinuxVMBuildContext> for CopyExisting {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using root filesystem: {}", self.path.display());
        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");

        let src = Directory::from_path(&self.path)?;
        let dest = Directory::from_path(rootfs)?;
        debug!(
            "copying content from {} to {}",
            src.path().display(),
            dest.path().display()
        );
        src.copy_content(dest.path())
            .context("failed to copy root filesystem content to temp directory")?;

        debug!(
            "root filesystem set: {} ({})",
            dest.path().display(),
            ByteSize::b(dest.size()?).to_string_as(true),
        );
        Ok(())
    }
}

/// Install root filesystem to disk partition.
///
/// # Context variables required
/// - `root-fs`
/// - `mountpoint`
pub struct InstallToMount;

impl Step<LinuxVMBuildContext> for InstallToMount {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");
        info!("installing root filesystem");

        let mountpoint = ctx.get::<PathBuf>("mountpoint").expect("mountpoint");
        debug!("{} -> {}", rootfs.display(), mountpoint.display());

        let rootfs_dir = Directory::from_path(rootfs)?;
        rootfs_dir
            .copy_content(mountpoint)
            .context("failed to install root filesystem")?;
        info!(
            "root filesystem installed to {}:/",
            ctx.opts().image_file_opts.path.display()
        );

        Ok(())
    }
}

/// Write root filesystem to SquashFS.
///
/// # Context variables required
/// - `root-fs`
/// - `squashfs`
pub struct InstallToSquashFs;

impl Step<LinuxVMBuildContext> for InstallToSquashFs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("writing root filesystem to SquashFS");
        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs").to_owned();
        let squashfs = ctx.get_mut::<SquashFs>("squashfs").expect("squashfs");
        squashfs.push_dir_recursively(&rootfs)?;
        Ok(())
    }
}
