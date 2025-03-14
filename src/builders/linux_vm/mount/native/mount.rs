use anyhow::{Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use tempdir::TempDir;

use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::LinuxVMBuildContext;
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

    fn new<P>(source: P, offset: u64) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let mountpoint =
            TempDir::new("mount").context("failed to create temp directory for mounting")?;

        run_command([
            OsStr::new("mount"),
            // Partition offset (loop device is setting up and detaching automatically)
            OsStr::new("--options"),
            OsStr::new(&format!("offset={}", offset)),
            source.as_ref().as_os_str(),
            mountpoint.path().as_os_str(),
        ])?;

        Ok(Self { mountpoint })
    }

    fn unmount_no_drop(&self) -> Result<()> {
        debug!("unmounting {}", &self);
        run_command([OsStr::new("umount"), self.mountpoint.path().as_os_str()])?;
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
///
/// `self.0` defines the name of context variable (of type `usize`),
/// storing the partition number to mount, e.g. `root-partition-number`.
///
/// # Context variables required
/// - `image-file`
/// - variable defined in `self.0` option
///
/// # Context variables set
/// - `mountpoint`
/// - `mount` (holds the actual mount until dropped)
pub struct MountFileSystem(pub &'static str);

impl Step<LinuxVMBuildContext> for MountFileSystem {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("mounting filesystem");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let partition_idx = *ctx.get::<usize>(self.0).expect(self.0);

        let mbr_adapter = Mbr::read_from(image_file.path())?;
        let (offset, _) = mbr_adapter.partition_limits(partition_idx)?;

        let mount =
            NativeMount::new(image_file.path(), offset).context("failed to mount failsystem")?;
        debug!("created mount {}", &mount);

        ctx.0
            .set("mountpoint", Box::new(mount.path().to_path_buf()));
        ctx.0.set("mount", Box::new(mount));

        Ok(())
    }
}
