use anyhow::{anyhow, bail, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::builders::linux_vm::filesystem::fat32::Fat32;
use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::{InitSystemOpts, LinuxVMBuildContext};
use crate::builders::Step;

/// Get path to `mbr.bin` file of SYSLINUX, e.g. `/usr/share/syslinux/mbr.bin`.
///
/// This function will try a number of paths and return `None`, if none of them succeeds.
pub fn mbr_bin_path() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/extlinux/mbr.bin",
        "/usr/share/syslinux/mbr.bin",
        "/usr/lib/extlinux/mbr/mbr.bin",
        "/usr/lib/syslinux/mbr/mbr.bin",
        "/usr/lib/extlinux/bios/mbr.bin",
        "/usr/lib/syslinux/bios/mbr.bin",
    ];
    CANDIDATES
        .into_iter()
        .map(PathBuf::from)
        .find_map(|candidate| {
            if candidate.exists() {
                return Some(candidate);
            } else {
                return None;
            }
        })
}

/// Install SYSLINUX through CLI.
///
/// # Context variables required
/// - `image-file`
/// - `boot-partition-number`
pub struct Install;

impl Step<LinuxVMBuildContext> for Install {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing SYSLINUX");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let boot_partition_number = *ctx
            .get::<usize>("boot-partition-number")
            .expect("boot-partition-number");

        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;
        let (start, end) = mbr_adapter
            .partition_limits(boot_partition_number)
            .context("failed to get partition info")?;

        let fat32_adapter =
            Fat32::read_from(image_file.path(), start, end).context("failed to read FAT32")?;
        let fs = fat32_adapter
            .fs()
            .context("failed to read boot filesystem")?;
        let boot_dir = fs
            .root_dir()
            .create_dir("boot")
            .context("failed to create SYSLINUX config directory")?;
        boot_dir
            .create_dir("syslinux")
            .context("failed to create SYSLINUX config directory")?;

        run_command(&[
            OsStr::new("syslinux"),
            OsStr::new("--directory"),
            OsStr::new("/boot/syslinux"),
            OsStr::new("--install"),
            OsStr::new("--offset"),
            OsStr::new(&format!("{}", fat32_adapter.start())),
            image_file.path().as_os_str(),
        ])
        .context("syslinux failed")?;

        let mbr_path = if let Some(mbr_path) = &ctx.opts().mbr_file {
            mbr_path.clone()
        } else if let Some(mbr_path) = mbr_bin_path() {
            mbr_path
        } else {
            bail!("cannot install SYSLINUX: MBR bootcode file not found");
        };
        let mut bootcode = [0u8; Mbr::BOOTCODE_SIZE];
        fs::File::open(&mbr_path)
            .context("failed to open MBR bootcode file")?
            .read_exact(&mut bootcode)
            .context("failed to read MBR bootcode file")?;

        debug!("writing MBR bootcode from {}", mbr_path.display());
        mbr_adapter
            .write_bootcode(bootcode)
            .context("failed to write MBR bootcode")?;
        Ok(())
    }
}

/// Install SYSLINUX configuration file.
///
/// # Context variables required
/// - `image-file`
/// - `boot-partition-number`
/// - `root-partition-number`
/// - `installed-kernel`
pub struct InstallCfg;

impl Step<LinuxVMBuildContext> for InstallCfg {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing SYSLINUX config");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let boot_partition_number = *ctx
            .get::<usize>("boot-partition-number")
            .expect("boot-partition-number");
        let root_partition_number = *ctx
            .get::<usize>("root-partition-number")
            .expect("root-partition-number");
        let installed_kernel = ctx
            .get::<PathBuf>("installed-kernel")
            .expect("installed-kernel");

        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;
        let (start, end) = mbr_adapter
            .partition_limits(boot_partition_number)
            .context("failed to get partition info")?;

        let fat32_adapter =
            Fat32::read_from(image_file.path(), start, end).context("failed to read FAT32")?;
        let fs = fat32_adapter
            .fs()
            .context("failed to read boot filesystem")?;

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

        let root_partition = format!("/dev/sda{}", root_partition_number);

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
        for line in cfg.lines() {
            debug!("SYSLINUX> {}", line);
        }

        let cfg_dir = fs
            .root_dir()
            .open_dir("boot/syslinux")
            .context("failed to open SYSLINUX config directory")?;
        let mut file = cfg_dir
            .create_file("syslinux.cfg")
            .context("failed to create SYSLINUX config file")?;
        file.write_all(cfg.as_bytes())
            .context("failed to write SYSLINUX config file")?;

        info!(
            "SYSLINUX config installed to {}:/boot/syslinux/syslinux.cfg",
            image_file.path().display()
        );
        Ok(())
    }
}
