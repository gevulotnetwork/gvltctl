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

#[derive(Debug)]
pub struct NativeMount {
    mountpoint: TempDir,
}

impl MountHandler for NativeMount {
    fn path(&self) -> &Path {
        self.mountpoint.path()
    }

    fn new<F, P>(fs: &F, source: P) -> Result<Self>
    where
        F: FileSystemHandler,
        P: AsRef<Path>,
    {
        let mountpoint =
            TempDir::new("mount").context("failed to create temp directory for mounting")?;

        run_command(&[
            OsStr::new("mount"),
            // Filesystem offset (loop device is setting up and detaching automatically)
            OsStr::new("--options"),
            OsStr::new(&format!("offset={}", fs.offset())),
            source.as_ref().as_os_str(),
            mountpoint.path().as_os_str(),
        ])?;

        Ok(Self { mountpoint })
    }

    fn unmount_no_drop(&self) -> Result<()> {
        debug!("unmounting {}", &self);
        run_command(&[OsStr::new("umount"), self.mountpoint.path().as_os_str()])?;
        Ok(())
    }
}

impl fmt::Display for NativeMount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.mountpoint.path().display()))
    }
}

impl Drop for NativeMount {
    fn drop(&mut self) {
        // ignore errors
        let _ = self.unmount_no_drop();
    }
}

/// Create new native filesystem mount.
pub struct MountFileSystem;

impl Step<LinuxVMBuildContext> for MountFileSystem {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("mounting filesystem");

        let fs = ctx.0.get::<Ext4>("fs").ok_or(Error::invalid_context(
            "mount filesystem",
            "filesystem handler",
        ))?;

        let mount = NativeMount::new(fs, fs.path()).context("failed to mount failsystem")?;
        debug!("created mount {}", &mount);

        ctx.0
            .set("mountpoint", Box::new(mount.path().to_path_buf()));
        ctx.0.set("mount", Box::new(mount));

        Ok(())
    }
}
