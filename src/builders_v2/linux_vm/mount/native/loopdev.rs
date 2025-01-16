use anyhow::{anyhow, Context, Result};
use log::debug;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::utils::run_command;
use super::LinuxVMBuildContext;

/// Loop device manipulator.
#[derive(Clone, Debug)]
pub struct LoopDev {
    path: PathBuf,
}

impl LoopDev {
    /// Setup loop device for file.
    pub fn setup(file: PathBuf) -> Result<Self> {
        let loopdev = run_command(
            &[
                OsStr::new("losetup"),
                OsStr::new("-f"),
                OsStr::new("P"),
                OsStr::new("--show"),
                file.as_os_str(),
            ],
            true,
        )
        .context("set up loop device")?;
        Ok(Self {
            path: PathBuf::from(loopdev),
        })
    }

    /// Detach loop device.
    pub fn detach(self) -> Result<()> {
        run_command(
            &[
                OsStr::new("losetup"),
                OsStr::new("-d"),
                self.path.as_os_str(),
            ],
            true,
        )
        .context("detach loop device")
        .map(|_| ())
    }

    /// Get partitioned device, e.g. `/dev/loop1p1`.
    pub fn part(&self, n: u32) -> Result<PathBuf> {
        let mut path = self.path.clone();
        let mut name = path
            .file_name()
            .ok_or(anyhow!("bad loop device"))?
            .to_os_string();
        name.push(format!("p{}", n));
        path.set_file_name(name);
        Ok(path)
    }

    /// Path to loop device.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl fmt::Display for LoopDev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path.display()))
    }
}

impl AsRef<OsStr> for LoopDev {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

pub struct AttachLoopDevice;

impl Step for AttachLoopDevice {
    type Context = LinuxVMBuildContext;

    fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
        let loopdev = LoopDev::setup(
            ctx.image_file
                .as_ref()
                .ok_or(anyhow!("cannot attach loop device: image file not found"))?
                .path()
                .to_path_buf(),
        )?;
        debug!("loop device attached: {}", &loopdev);
        ctx.loopdev = Some(loopdev);
        Ok(())
    }
}

pub struct DetachLoopDevice;

impl Step for DetachLoopDevice {
    type Context = LinuxVMBuildContext;

    fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
        if let Some(loopdev) = ctx.loopdev.take() {
            loopdev.detach()?;
        }
        Ok(())
    }
}
