use anyhow::{anyhow, Context, Result};
use fs_extra::dir;
use log::{debug, info};
use std::path::{Path, PathBuf};
use std::fmt;
use bytesize::ByteSize;

use crate::builders::Step;

use super::LinuxVMBuildContext;

/// Root filesystem handler.
#[derive(Clone, Debug)]
pub struct RootFS {
    path: PathBuf,
    size: ByteSize,
}

impl RootFS {
    /// Create roof filesystem handler from given path.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        debug_assert!(path.is_dir());
        let size = ByteSize::b(dir::get_size(&path).context("get root filesystem size")?);
        Ok(Self { size, path })
    }

    /// Path to root filesystem directory on host machine.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Size of all files in the filesystem.
    pub fn size(&self) -> ByteSize {
        self.size
    }

    /// Install root filesystem from host to target filesystem.
    pub fn install(&self, mountpoint: &Path) -> Result<()> {
        dir::copy(self.path(), mountpoint, &dir::CopyOptions::new())
            .context("copy root filesystem content")
            .map_err(Into::into)
            .map(|_| ())
    }
}

impl fmt::Display for RootFS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path().display()))
    }
}

impl AsRef<Path> for RootFS {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

/// Use ready root filesystem from given path.
pub struct RootFSFromDir {
    path: PathBuf,
}

impl RootFSFromDir {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Step<LinuxVMBuildContext> for RootFSFromDir {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("setting root filesystem");
        let rootfs = RootFS::from_path(self.path.clone()).context("set root filesystem path")?;
        debug!("root filesystem set: {} ({} bytes)", &rootfs, rootfs.size());
        ctx.0.set("rootfs", Box::new(rootfs));
        Ok(())
    }
}

/// Install root filesystem to disk partition.
pub struct InstallRootFS;

impl Step<LinuxVMBuildContext> for InstallRootFS {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing root filesystem");

        let rootfs = ctx.0.get::<RootFS>("rootfs").ok_or(anyhow!(
            "cannot install root filesystem: root filesystem not found"
        ))?;

        let mountpoint = ctx.0.get::<PathBuf>("mountpoint").ok_or(anyhow!(
            "cannot install root filesystem: mount handler not found"
        ))?;

        rootfs.install(mountpoint)?;

        Ok(())
    }
}
