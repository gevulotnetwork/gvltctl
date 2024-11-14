use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{self, Path, PathBuf};

use crate::builders::Step;

use super::mbr::Mbr;
use super::mount::Mount;
use super::utils::run_command;
use super::LinuxVMBuildContext;

const DEFAULT_EXTLINUX_DIR: &str = "/boot/extlinux";

/// This boot code is taken from extlinux.
const BOOTCODE: &[u8; 440] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/extlinux/mbr.bin"
));

/// EXTLINUX wrapper.
#[derive(Debug)]
pub struct Extlinux {
    directory: PathBuf,
}

impl Default for Extlinux {
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_EXTLINUX_DIR),
        }
    }
}

impl Extlinux {
    /// EXTLINUX installation directory on target filesystem, e.g. `/boot/extlinux`.
    pub fn directory(&self) -> &Path {
        self.directory.as_path()
    }

    pub fn directory_relative(&self) -> Result<&Path> {
        self.directory
            .strip_prefix(path::MAIN_SEPARATOR_STR)
            .context("stip prefix from extlinux target directory")
    }

    /// Install EXTLINUX.
    pub fn install<P: AsRef<Path>>(directory: P, mount: &Mount) -> Result<Self> {
        let extlinux = Extlinux {
            directory: directory.as_ref().to_path_buf(),
        };
        let path = mount.path().join(extlinux.directory_relative()?);
        fs::create_dir_all(&path).context("create EXTLINUX directory")?;
        run_command(
            &[
                OsStr::new("extlinux"),
                OsStr::new("--install"),
                path.as_os_str(),
            ],
            false,
        )
        .context("run extlinux")?;
        Ok(extlinux)
    }

    /// Install EXTLINUX config file.
    pub fn install_config(&self, mount: &Mount, cfg: &str) -> Result<()> {
        let cfg_path = mount.path().join(self.directory_relative()?);
        let mut file = fs::File::create_new(&cfg_path).context("create EXTLINUX config file")?;
        file.write_all(cfg.as_bytes())
            .context("write EXTLINUX config file")?;
        Ok(())
    }
}

/// Install EXTLINUX through CLI.
pub struct InstallExtlinux;

impl Step<LinuxVMBuildContext> for InstallExtlinux {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        // info!("installing EXTLINUX");

        // let fs = ctx
        //     .0
        //     .get::<FileSystem>("fs")
        //     .ok_or(anyhow!("cannot install EXTLINUX: filesystem not found"))?;

        // let extlinux = Extlinux::default();
        // extlinux.install(fs).context("install EXTLINUX")?;

        // let mbr = ctx
        //     .0
        //     .get_mut::<Mbr>("mbr")
        //     .ok_or(anyhow!("cannot install EXTLINUX: MBR not found"))?;

        // debug!("writing MBR bootcode");
        // mbr.write_bootcode(BOOTCODE.clone())
        //     .context("write MBR bootcode")?;

        // ctx.0.set("extlinux", Box::new(extlinux));
        // Ok(())
        todo!()
    }
}

/// Install EXTLINUX configuration file.
pub struct InstallExtlinuxCfg;

impl Step<LinuxVMBuildContext> for InstallExtlinuxCfg {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing EXTLINUX config");

        let extlinux = ctx.0.get::<Extlinux>("extlinux").ok_or(anyhow!(
            "cannot install EXTLINUX config: extlinux not found"
        ))?;

        let mount = ctx.0.get::<Mount>("mount").ok_or(anyhow!(
            "cannot install EXTLINUX config: mount handler not found"
        ))?;

        let installed_kernel = ctx.0.get::<PathBuf>("installed-kernel").ok_or(anyhow!(
            "cannot install EXTLINUX config: no installed kernel"
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

        extlinux
            .install_config(mount, cfg.as_str())
            .context("install EXTLINUX config")?;

        Ok(())
    }
}
