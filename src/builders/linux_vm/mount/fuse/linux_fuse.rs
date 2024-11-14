use anyhow::{anyhow, bail, Context, Result};
use log::{debug, info, trace};
use semver::Version;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempdir::TempDir;

use crate::builders::linux_vm::filesystem::FileSystem;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

const FUSE2FS_BINNAME: &str = "fuse2fs";
const MIN_VERSION: &str = "1.47.0";

/// `fuse2fs` wrapper.
#[derive(Debug)]
struct Fuse2fs {
    path: PathBuf,
}

impl Fuse2fs {
    /// Create wrapper from given path.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let fuse2fs = Self {
            path: path.as_ref().to_path_buf(),
        };
        fuse2fs.check_version()?;
        Ok(fuse2fs)
    }

    /// Automatically locate fuse2fs (like `which fuse2fs`).
    pub fn locate() -> Result<Self> {
        let path = which::which(FUSE2FS_BINNAME).context("")?;
        Self::from_path(path)
    }

    fn check_version(&self) -> Result<()> {
        trace!("check version of {}", self.path.display());
        let (_, version_output) = self.run(["--version"])?;
        version_output.lines().for_each(|line| trace!("{}", line));
        let version = Version::parse(&version_output[8..14])
            .context("parsing output of `fuse2fs --version`")?;
        let required_version =
            Version::parse(MIN_VERSION).context("parsing required version string")?;
        if version < required_version {
            bail!(
                "fuse2fs version is too low. Required >={}, found {}",
                required_version.to_string(),
                version.to_string()
            );
        }
        Ok(())
    }

    /// Run fuse2fs with given args returning decoded stdout and stderr.
    pub fn run<I, S>(&self, args: I) -> Result<(String, String)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let child = Command::new(self.path.as_os_str())
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn fuse2fs process")?;
        let output = child
            .wait_with_output()
            .context("failed to wait for fuse2fs to finish")?;
        if output.status.success() {
            Ok((
                String::from_utf8(output.stdout).context("failed to decode fuse2fs stdout")?,
                String::from_utf8(output.stderr).context("failed to decode fuse2fs stderr")?,
            ))
        } else {
            Err(anyhow!("fuse2fs failed with status {}", output.status))
        }
    }
}

/// FUSE mount.
#[derive(Debug)]
pub struct Mount {
    fuse2fs: Fuse2fs,
    mountpoint: TempDir,
}

impl fmt::Display for Mount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.mountpoint.path().display()))
    }
}

impl Mount {
    /// Mount filesystem.
    pub fn new<P: AsRef<Path>>(fs: &FileSystem, source: P) -> Result<Self> {
        let fuse2fs = Fuse2fs::locate().context("locating fuse2fs")?;
        let offset = fs.start();
        let mountpoint = TempDir::new("mount").context("create temp directory for mounting")?;
        fuse2fs.run([
            OsStr::new("-o"),
            OsStr::new(&format!("fakeroot,offset={}", offset)),
            source.as_ref().as_os_str(),
            mountpoint.path().as_os_str(),
        ])?;
        Ok(Self {
            fuse2fs,
            mountpoint,
        })
    }

    /// `fuse2fs` wrapper.
    pub fn fuse2fs(&self) -> &Fuse2fs {
        &self.fuse2fs
    }

    /// Path to mounted directory.
    pub fn path(&self) -> &Path {
        self.mountpoint.path()
    }

    /// Unmount filesystem.
    pub fn unmount(self) -> Result<()> {
        self.unmount_no_drop()
    }

    /// Unmount filesystem without dropping self object.
    /// Makes self invalid.
    fn unmount_no_drop(&self) -> Result<()> {
        let mut umount_args = vec![OsStr::new("umount"), self.mountpoint.path().as_os_str()];
        match run_command(
            [OsStr::new("lsof"), self.mountpoint.path().as_os_str()],
            false,
        ) {
            Ok(_) => {
                trace!("umount target is busy, performing lazy umount");
                umount_args.insert(1, OsStr::new("--lazy"));
            }
            Err(_) => {
                trace!("umount target is not busy");
            }
        }
        run_command(&umount_args, false).context("unmounting filesystem")?;
        Ok(())
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        // ignore errors
        debug!("unmounting {}", &self);
        let _ = self.unmount_no_drop();
    }
}

pub struct MountFileSystem;

impl Step<LinuxVMBuildContext> for MountFileSystem {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("mounting filesystem");

        let fs = ctx.0.get::<FileSystem>("fs").ok_or(anyhow!(
            "cannot mount filesystem: filesystem handler not found"
        ))?;

        let mount = Mount::new(fs, fs.path()).context("mount filesystem")?;
        debug!("mounted filesystem at {}", &mount);

        ctx.0.set("mount", Box::new(mount));

        Ok(())
    }
}
