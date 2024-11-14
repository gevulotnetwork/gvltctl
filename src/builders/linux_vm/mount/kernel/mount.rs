use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::fmt;
use std::ffi::OsStr;

use crate::builders::Step;

use super::utils::run_command;
use super::LinuxVMBuildContext;

pub struct Mount {
    path: PathBuf,
}

impl Mount {
    pub fn mount(path: PathBuf) -> Result<Self> {
        Ok(Self { path })
    }

    pub fn umount(self) -> Result<()> {
        Ok(())
    }
}

impl fmt::Display for Mount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path.display()))
    }
}

impl AsRef<OsStr> for Mount {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

// impl Step for Mount {
//     type Context = LinuxVMBuildContext;

//     fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
//         // ctx.rootfs_mountpoint = ctx.tmpdir.path().join("mnt");
//         // ctx.boot_mountpoint = ctx.tmpdir.path().join("mnt").join("boot");

//         // fs::create_dir_all(&ctx.rootfs_mountpoint).context("Failed to create mount directory")?;

//         // run_command(
//         //     &[
//         //         "mount",
//         //         &ctx.rootfs_loopdev.to_str().unwrap(),
//         //         &ctx.rootfs_mountpoint.to_str().unwrap(),
//         //     ],
//         //     true,
//         // )
//         // .context("Failed to mount root filesystem")?;

//         // run_command(
//         //     &["mkdir", "-p", &ctx.boot_mountpoint.to_str().unwrap()],
//         //     true,
//         // )
//         // .context("Failed to create boot directory")?;

//         // run_command(
//         //     &[
//         //         "mount",
//         //         &ctx.boot_loopdev.to_str().unwrap(),
//         //         env::temp_dir().join("mnt").join("boot").to_str().unwrap(),
//         //     ],
//         //     true,
//         // )
//         // .context("Failed to mount boot filesystem")?;
//         Ok(())
//     }
// }

// pub struct Unmount;

// impl Step for Unmount {
//     type Context = LinuxVMBuildContext;

//     fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
//         todo!()
//     }
// }
