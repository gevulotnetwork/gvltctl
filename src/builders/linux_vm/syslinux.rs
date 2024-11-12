use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use unix_path::{Path as UnixPath, PathBuf as UnixPathBuf};

use crate::builders::Step;

use super::filesystem::FileSystem;
use super::mbr::Mbr;
use super::utils::run_command;
use super::LinuxVMBuildContext;

const DEFAULT_SYSLINUX_DIR: &str = "/boot/syslinux";

/// This boot code is taken from extlinux.
const BOOTCODE: &[u8; 440] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/syslinux/mbr.bin"
));

/// SYSLINUX wrapper.
#[derive(Debug)]
pub struct Syslinux {
    /// Path to directory where to install SYSLINUX. e.g. `/boot/syslinux`.
    pub directory: String,
}

impl Default for Syslinux {
    fn default() -> Self {
        Self::new(String::from(DEFAULT_SYSLINUX_DIR))
    }
}

impl Syslinux {
    /// Create new SYSYLINUX installer.
    pub fn new(directory: String) -> Self {
        Self { directory }
    }

    /// Install SYSLINUX.
    pub fn install(&self, fs: &FileSystem) -> Result<()> {
        fs.create_dir(UnixPath::new("boot/syslinux"))
            .context("create /boot/syslinux directory")?;
        let offset = fs.start();
        let file = fs
            .path()
            .as_os_str()
            .to_str()
            .ok_or(anyhow!("non-UTF-8 in disk image path"))?;
        run_command(
            &[
                "syslinux",
                "--directory",
                self.directory.as_str(),
                "--offset",
                offset.to_string().as_str(),
                file,
            ],
            false,
        )
        .context("run syslinux")?;
        Ok(())
    }

    /// Install SYSLINUX config file.
    pub fn install_config(&self, fs: &FileSystem, cfg: &str) -> Result<()> {
        let path = UnixPath::new(&self.directory);
        fs.write_file(
            path.join("syslinux.cfg")
                .strip_prefix(&unix_path::MAIN_SEPARATOR.to_string())
                .context("stip path prefix")?,
            cfg.as_bytes(),
        )?;
        Ok(())
    }
}

/// Install SYSLINUX through CLI.
pub struct InstallSyslinux;

impl Step<LinuxVMBuildContext> for InstallSyslinux {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing SYSLINUX");

        let fs = ctx
            .0
            .get::<FileSystem>("fs")
            .ok_or(anyhow!("cannot install SYSLINUX: filesystem not found"))?;

        let syslinux = Syslinux::default();
        syslinux.install(fs).context("install SYSLINUX")?;

        let mbr = ctx
            .0
            .get_mut::<Mbr>("mbr")
            .ok_or(anyhow!("cannot install SYSLINUX: MBR not found"))?;

        debug!("writing MBR bootcode");
        mbr.write_bootcode(BOOTCODE.clone())
            .context("write MBR bootcode")?;

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

        let fs = ctx.0.get::<FileSystem>("fs").ok_or(anyhow!(
            "cannot install SYSLINUX config: filesystem not found"
        ))?;

        let installed_kernel = ctx.0.get::<UnixPathBuf>("installed-kernel").ok_or(anyhow!(
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
            .install_config(fs, cfg.as_str())
            .context("install SYSLINUX config")?;

        Ok(())
    }
}
