use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{self, Path, PathBuf};

use crate::builders::Step;

use super::filesystem::FileSystem;
use super::mbr::Mbr;
use super::utils::run_command;
use super::LinuxVMBuildContext;

const DEFAULT_SYSLINUX_DIR: &str = "/boot/syslinux";

/// This boot code is taken from extlinux.
const BOOTCODE: &[u8; 440] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/extlinux/mbr.bin"
));

/// SYSLINUX wrapper.
#[derive(Debug)]
pub struct Syslinux {
    directory: PathBuf,
}

impl Default for Syslinux {
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_SYSLINUX_DIR),
        }
    }
}

impl Syslinux {
    /// SYSLINUX installation directory on target filesystem, e.g. `/boot/syslinux`.
    pub fn directory(&self) -> &Path {
        self.directory.as_path()
    }

    /// Same as [`Self::directory()`] but without leading `/`.
    pub fn directory_relative(&self) -> Result<&Path> {
        self.directory()
            .strip_prefix(path::MAIN_SEPARATOR_STR)
            .context("failed to stip prefix from syslinux target directory")
    }

    /// Install SYSLINUX.
    pub fn install<P: AsRef<Path>>(path: &Path, offset: u64, directory: P) -> Result<Self> {
        let syslinux = Syslinux {
            directory: directory.as_ref().to_path_buf(),
        };
        run_command(
            &[
                OsStr::new("syslinux"),
                OsStr::new("--offset"),
                OsStr::new(offset.to_string().as_str()),
                OsStr::new("--install"),
                OsStr::new("--directory"),
                OsStr::new(syslinux.directory().as_os_str()),
                path.as_os_str(),
            ],
            false,
        )
        .context("syslinux failed")?;
        Ok(syslinux)
    }

    /// Install SYSLINUX config file.
    pub fn install_config(&self, mountpoint: &Path, cfg: &str) -> Result<()> {
        let cfg_path = mountpoint.join(self.directory_relative()?);
        let mut file =
            fs::File::create_new(&cfg_path).context("failed to create SYSLINUX config file")?;
        file.write_all(cfg.as_bytes())
            .context("failed to write SYSLINUX config file")?;
        Ok(())
    }
}

/// Install SYSLINUX through CLI.
pub struct InstallSyslinux;

impl Step<LinuxVMBuildContext> for InstallSyslinux {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing SYSLINUX");

        let fs = ctx.0.get::<FileSystem>("fs").ok_or(anyhow!(
            "cannot install SYSLINUX: filesystem handler not found"
        ))?;

        let syslinux = Syslinux::install(fs.path(), fs.offset(), DEFAULT_SYSLINUX_DIR)
            .context("failed to install SYSLINUX")?;

        let mbr = ctx
            .0
            .get_mut::<Mbr>("mbr")
            .ok_or(anyhow!("cannot install SYSLINUX: MBR not found"))?;

        debug!("writing MBR bootcode");
        mbr.write_bootcode(BOOTCODE.clone())
            .context("failed write MBR bootcode")?;

        ctx.0.set("syslinux", Box::new(syslinux));
        Ok(())
    }
}

/// Install SYSLINUX configuration file.
pub struct InstallSyslinuxCfg;

impl Step<LinuxVMBuildContext> for InstallSyslinuxCfg {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing SYSLINUX config");

        let syslinux = ctx.0.get::<Syslinux>("syslinux").ok_or(anyhow!(
            "cannot install SYSLINUX config: syslinux not found"
        ))?;

        let mountpoint = ctx.0.get::<PathBuf>("mountpoint").ok_or(anyhow!(
            "cannot install SYSLINUX config: mount point not found"
        ))?;

        let installed_kernel = ctx.0.get::<PathBuf>("installed_kernel").ok_or(anyhow!(
            "cannot install SYSLINUX config: no installed kernel"
        ))?;

        let cfg = format!(
            r#"DEFAULT linux
PROMPT 0
TIMEOUT 50

LABEL linux
    LINUX {}
    APPEND root=/dev/sda1 ro console=ttyS0 init=/bin/testapp
"#,
            installed_kernel.to_str().ok_or(anyhow!("non-UTF-8 path"))?
        );

        syslinux
            .install_config(mountpoint, cfg.as_str())
            .context("failed to install SYSLINUX config")?;

        Ok(())
    }
}
