use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::fs;
use std::io::{Seek, SeekFrom, Write};

use crate::builders::Step;

use super::{FileSystem, LinuxVMBuildContext, Mbr};

/// This boot code is taken from extlinux.
const BOOTCODE: &[u8; 440] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/syslinux/mbr.bin"
));

const LDLINUX_C32: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/syslinux/ldlinux.c32"
));

const LDLINUX_SYS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/syslinux/ldlinux.sys"
));

const VBR_IMG: &[u8; 512] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/builders/linux_vm/data/syslinux/vbr.img"
));

const SYSLINUX_LIBS: [(&str, &[u8]); 2] =
    [("ldlinux.c32", LDLINUX_C32), ("ldlinux.sys", LDLINUX_SYS)];

const DEFAULT_SYSLINUX_DIR: &str = "boot/syslinux";

/// SYSLINUX installer.
#[derive(Debug)]
pub struct Syslinux {
    // TODO: enforce Unix path here
    /// Relative path to directory where to install SYSLINUX. e.g. `boot/syslinux`.
    pub directory: String,
}

impl Default for Syslinux {
    fn default() -> Self {
        Self::new(String::from(DEFAULT_SYSLINUX_DIR))
    }
}

impl Syslinux {
    /// Create new SYSYLINUX installer.
    ///
    /// *NOTE:* `directory` must be relative to root and not have trailing slash.
    pub fn new(directory: String) -> Self {
        Self { directory }
    }

    /// Install SYSLINUX using given MBR.
    pub fn install(&self, mbr: &mut Mbr, fs: &FileSystem) -> Result<()> {
        debug!("writing MBR bootcode of SYSLINUX");
        mbr.write_bootcode(BOOTCODE.clone())?;

        debug!("writing VBR");
        // This cannot be done with `fatfs` crate, so doing it manually
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(mbr.path())
            .context("open disk image file")?;
        debug!("fs start = {}", fs.start());
        file.seek(SeekFrom::Start(fs.start()))
            .context("seek filesystem offset on disk image")?;
        file.write_all(VBR_IMG).context("write VBR")?;
        drop(file);

        debug!("installing SYSLINUX libraries");
        let fs = fs.get_fs()?;
        let root_dir = fs.root_dir();
        root_dir
            .create_dir("boot")
            .context("create boot directory")?;
        root_dir
            .create_dir(&self.directory)
            .context(format!("create {} directory", &self.directory))?;

        for (filename, bytes) in SYSLINUX_LIBS {
            let file_path = format!("boot/syslinux/{}", &filename);
            let mut file = root_dir
                .create_file(&file_path)
                .context(format!("create {} file", file_path))?;
            file.truncate()
                .context(format!("truncate {} file", file_path))?;
            file.write_all(bytes)
                .context(format!("write {} file", file_path))?;
        }

        Ok(())
    }

    /// Install SYSLINUX config file.
    pub fn install_config(&self, fs: &FileSystem, cfg: &str) -> Result<()> {
        let cfg_file = format!("{}/syslinux.cfg", &self.directory);
        let fs = fs.get_fs()?;

        let root_dir = fs.root_dir();
        root_dir
            .create_dir(&self.directory)
            .context(format!("create {} directory", &self.directory))?;
        let mut file = root_dir
            .create_file(&cfg_file)
            .context(format!("create {} file", &cfg_file))?;
        file.truncate()
            .context(format!("truncate {} file", &cfg_file))?;
        file.write_all(cfg.as_bytes())
            .context(format!("write {} file", &cfg_file))?;
        Ok(())
    }
}

pub struct InstallSyslinux;

impl Step for InstallSyslinux {
    type Context = LinuxVMBuildContext;

    fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
        info!("installing SYSLINUX");

        let mbr = ctx
            .mbr
            .as_mut()
            .ok_or(anyhow!("cannot install SYSLINUX: MBR not found"))?;

        let fs = ctx
            .fs
            .as_ref()
            .ok_or(anyhow!("cannot install SYSLINUX: filesystem not found"))?;

        let syslinux = Syslinux::default();
        syslinux.install(mbr, fs)?;

        ctx.bootloader = Some(syslinux);

        Ok(())
    }
}

pub struct InstallSyslinuxCfg;

impl Step for InstallSyslinuxCfg {
    type Context = LinuxVMBuildContext;

    fn run(&mut self, ctx: &mut Self::Context) -> Result<()> {
        info!("installing SYSLINUX");

        let syslinux = ctx.bootloader.as_ref().ok_or(anyhow!(
            "cannot install SYSLINUX config: bootloader not found"
        ))?;

        let fs = ctx.fs.as_ref().ok_or(anyhow!(
            "cannot install SYSLINUX config: filesystem not found"
        ))?;

        let cfg = r#"DEFAULT linux
PROMPT 0
TIMEOUT 50

LABEL linux
    LINUX /bzImage
    APPEND root=/dev/sda1 ro console=ttyS0
"#;

        syslinux.install_config(fs, cfg)?;

        Ok(())
    }
}
