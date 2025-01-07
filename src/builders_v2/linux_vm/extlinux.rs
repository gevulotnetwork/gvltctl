use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{self, Path, PathBuf};

use crate::builders::Step;

use super::mbr::Mbr;
use super::utils::run_command;
use super::{InitSystemOpts, LinuxVMBuildContext};

const DEFAULT_EXTLINUX_DIR: &str = "/boot/extlinux";

/// This boot code is copied from extlinux 6.04.
const BOOTCODE: &[u8; 440] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders_v2/linux_vm/data/extlinux/mbr.bin"
));

/// EXTLINUX wrapper.
#[derive(Clone, Debug)]
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

    /// Same as [`Self::directory()`] but without leading `/`.
    pub fn directory_relative(&self) -> Result<&Path> {
        self.directory()
            .strip_prefix(path::MAIN_SEPARATOR_STR)
            .context("failed to stip prefix from extlinux target directory")
    }

    /// Install EXTLINUX.
    pub fn install<P: AsRef<Path>>(directory: P, mountpoint: &Path) -> Result<Self> {
        let extlinux = Extlinux {
            directory: directory.as_ref().to_path_buf(),
        };
        let path = mountpoint.join(extlinux.directory_relative()?);
        fs::create_dir_all(&path).context("failed to create EXTLINUX directory")?;
        run_command(&[
            OsStr::new("extlinux"),
            OsStr::new("--install"),
            path.as_os_str(),
        ])
        .context("extlinux failed")?;
        Ok(extlinux)
    }

    /// Install EXTLINUX config file.
    pub fn install_config(&self, mountpoint: &Path, cfg: &str) -> Result<()> {
        let cfg_path = mountpoint
            .join(self.directory_relative()?)
            .join("extlinux.conf");
        let mut file =
            fs::File::create_new(&cfg_path).context("failed to create EXTLINUX config file")?;
        file.write_all(cfg.as_bytes())
            .context("failed to write EXTLINUX config file")?;
        Ok(())
    }
}

/// Install EXTLINUX through CLI.
///
/// # Context variables required
/// - `mountpoint`
/// - `mbr`
///
/// # Context variabled defined
/// - `extlinux`
pub struct InstallExtlinux;

impl Step<LinuxVMBuildContext> for InstallExtlinux {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing EXTLINUX");

        let mountpoint = ctx
            .get::<PathBuf>("mountpoint")
            .ok_or(anyhow!("cannot install EXTLINUX: mount point not found"))?;

        let extlinux = Extlinux::install(DEFAULT_EXTLINUX_DIR, &mountpoint)
            .context("failed to install EXTLINUX")?;

        let mbr = ctx
            .get_mut::<Mbr>("mbr")
            .ok_or(anyhow!("cannot install EXTLINUX: MBR not found"))?;

        debug!("writing MBR bootcode");
        mbr.write_bootcode(BOOTCODE.clone())
            .context("failed to write MBR bootcode")?;

        ctx.set("extlinux", Box::new(extlinux));
        Ok(())
    }
}

/// Install EXTLINUX configuration file.
///
/// # Context variables required
/// - `mountpoint`
/// - `extlinux`
/// - `installed-kernel`
pub struct InstallExtlinuxCfg;

impl Step<LinuxVMBuildContext> for InstallExtlinuxCfg {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing EXTLINUX config");

        let extlinux = ctx.get::<Extlinux>("extlinux").cloned().unwrap_or_default();

        let mountpoint = ctx.get::<PathBuf>("mountpoint").ok_or(anyhow!(
            "cannot install EXTLINUX config: mount point not found"
        ))?;

        let installed_kernel = ctx.get::<PathBuf>("installed-kernel").ok_or(anyhow!(
            "cannot install EXTLINUX config: no installed kernel"
        ))?;

        let (init, init_args) = match &ctx.opts().init_system_opts {
            InitSystemOpts::Mia { .. } => ("".to_string(), "".to_string()),
            InitSystemOpts::Custom { init, init_args } => (
                format!(" init={}", init),
                if let Some(init_args) = init_args {
                    format!(" -- {}", init_args)
                } else {
                    "".to_string()
                },
            ),
        };

        // TODO: define this dynamically
        let root_partition = "/dev/sda1".to_string();

        let root_dev_mode = if ctx.opts().rw_root { "rw" } else { "ro" };

        let cfg = format!(
            r#"DEFAULT linux
PROMPT 0
TIMEOUT 50

LABEL linux
    LINUX {}
    APPEND root={} {} console=ttyS0{}{}
"#,
            installed_kernel
                .to_str()
                .ok_or(anyhow!("non-UTF-8 path to installed kernel"))?,
            root_partition,
            root_dev_mode,
            init,
            init_args,
        );

        extlinux
            .install_config(mountpoint, cfg.as_str())
            .context("failed to install EXTLINUX config")?;

        Ok(())
    }
}
