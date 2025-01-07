//! Root filesystem handling.
//!
//! Main type is [`RootFS`], which represents filesystem source files to pack into VM.

use anyhow::{Context, Result};
use bytesize::ByteSize;
use fs_extra::dir;
use log::{debug, info};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::utils::run_command;
use crate::builders::Step;

use super::{LinuxVMBuildContext, LinuxVMBuilderError};

/// Root filesystem handler.
#[derive(Clone, Debug)]
pub struct RootFS {
    path: PathBuf,
}

impl RootFS {
    /// Create roof filesystem handler from given path.
    pub fn from_path(path: PathBuf) -> Result<Self> {
        debug_assert!(path.is_dir());
        Ok(Self { path })
    }

    /// Path to root filesystem directory on host machine.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Size of all files in the filesystem.
    pub fn size(&self) -> Result<ByteSize> {
        dir::get_size(&self.path)
            .context("failed to get root filesystem size")
            .map(|bytes| ByteSize::b(bytes))
    }

    /// Install root filesystem from host to target filesystem.
    pub fn install(&self, mountpoint: &Path) -> Result<()> {
        // FIXME: for some weird reason commented code below fails with error:
        //   "No such file or directory"
        // So using 'cp -r' instead. Probably will fix in the future.

        // let copy_opts = dir::CopyOptions::new()
        //     .content_only(true);
        // if let Err(err) = dir::copy(self.path(), mountpoint, &copy_opts) {
        //     // `fs_extra` error reporting is crazy, so we manually print the actual error here.
        //     let inner = format!("{:?}", &err.kind);
        //     return Err(err)
        //         .context(inner)
        //         .context("failed to copy root filesystem");
        // }

        run_command([
            "sh",
            "-c",
            &format!("cp -r {}/* {}", self.path().display(), mountpoint.display()),
        ])
        .context("failed to copy root filesystem content")?;
        Ok(())
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

/// Create empty root filesystem (essentially just a temporary directory).
///
/// # Context variables defined
/// - `rootfs`
pub struct RootFSEmpty;

impl Step<LinuxVMBuildContext> for RootFSEmpty {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("setting empty root filesystem");
        let path = ctx.tmpdir().join("rootfs");
        std::fs::create_dir(&path)
            .context("failed to create temp directory for root filesystem")?;
        let rootfs = RootFS::from_path(path).context("set root filesystem path")?;
        debug!(
            "root filesystem set: {} ({} bytes)",
            &rootfs,
            rootfs.size()?
        );
        ctx.set("rootfs", Box::new(rootfs));
        Ok(())
    }
}

/// Use ready root filesystem from given path.
///
/// # Context variables defined
/// - `rootfs`
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
        debug!(
            "root filesystem set: {} ({} bytes)",
            &rootfs,
            rootfs.size()?
        );
        ctx.set("rootfs", Box::new(rootfs));
        Ok(())
    }
}

/// Install root filesystem to disk partition.
///
/// # Context variables required
/// - `rootfs`
/// - `mountpoint`
pub struct InstallRootFS;

impl Step<LinuxVMBuildContext> for InstallRootFS {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let rootfs = ctx
            .get::<RootFS>("rootfs")
            .ok_or(LinuxVMBuilderError::invalid_context(
                "install root filesystem",
                "root filesystem handler",
            ))?;
        info!("installing root filesystem: {}", rootfs.path().display());

        let mountpoint =
            ctx.get::<PathBuf>("mountpoint")
                .ok_or(LinuxVMBuilderError::invalid_context(
                    "install root filesystem",
                    "mountpoint",
                ))?;

        rootfs
            .install(mountpoint)
            .context("failed to install root filesystem")?;
        debug!(
            "root filesystem installed to {}:/",
            ctx.opts().image_file_opts.path.display()
        );

        Ok(())
    }
}
