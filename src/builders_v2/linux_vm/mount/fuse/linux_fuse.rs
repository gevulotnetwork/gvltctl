use anyhow::{Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use tempdir::TempDir;

use crate::builders::linux_vm::filesystem::{Ext4, FileSystemHandler};
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::{LinuxVMBuildContext, LinuxVMBuilderError as Error};
use crate::builders::Step;

use super::MountHandler;

/// FUSE mount.
///
/// Mounted directory will be unmounted and removed on drop.
#[derive(Debug)]
pub struct FuseMount {
    mountpoint: TempDir,
}

impl MountHandler for FuseMount {
    fn path(&self) -> &Path {
        self.mountpoint.path()
    }

    fn new<F, P>(fs: &F, source: P) -> Result<Self>
    where
        F: FileSystemHandler,
        P: AsRef<Path>,
    {
        let offset = fs.offset();
        let mountpoint =
            TempDir::new("linux-vm-mount").context("create temp directory for mounting")?;
        run_command([
            OsStr::new("fuse2fs"),
            OsStr::new("-o"),
            OsStr::new(&format!("fakeroot,offset={}", offset)),
            source.as_ref().as_os_str(),
            mountpoint.path().as_os_str(),
        ])?;
        Ok(Self { mountpoint })
    }

    fn unmount_no_drop(&self) -> Result<()> {
        run_command(&[
            OsStr::new("umount"),
            OsStr::new("--lazy"),
            self.mountpoint.path().as_os_str(),
        ])
        .context("unmounting filesystem failed")?;
        Ok(())
    }
}

impl fmt::Display for FuseMount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.mountpoint.path().display()))
    }
}

impl Drop for FuseMount {
    fn drop(&mut self) {
        // ignore errors
        debug!("unmounting {}", &self);
        let _ = self.unmount_no_drop();
    }
}

/// Create new filesystem FUSE-based mount.
///
/// # Context variables required
/// - `fs`
///
/// # Context variables set
/// - `mountpoint`
/// - `mount` (holds the actual mount until dropped)
pub struct MountFileSystem;

impl Step<LinuxVMBuildContext> for MountFileSystem {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("mounting filesystem (FUSE)");

        let fs = ctx.get::<Ext4>("fs").ok_or(Error::invalid_context(
            "mount filesystem",
            "filesystem handler",
        ))?;

        let mount = FuseMount::new(fs, fs.path()).context("mount filesystem")?;
        debug!("mounted filesystem at {}", &mount);

        // TODO: probably there is a nice way to retrieve this path from trait object of Mount.
        // However I couldn't find a way to cast into something like `dyn HasMountPoint`.
        // So we store mountpoint as a separate trivial context variable
        ctx.set("mountpoint", Box::new(mount.path().to_path_buf()));

        ctx.set("mount", Box::new(mount));

        Ok(())
    }
}