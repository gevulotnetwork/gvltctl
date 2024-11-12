use anyhow::{anyhow, Context, Result};
use log::info;
use mbrman::{MBRHeader, MBRPartitionEntry};
use std::fs;
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::image_file::ImageFile;
use super::LinuxVMBuildContext;

const DISK_SIGNATURE: [u8; 4] = [b'G', b'V', b'L', b'T'];
const SECTOR_SIZE: u32 = 512;
const ALIGN: u32 = 1;
const BOOTCODE_SIZE: usize = 440;

/// Master Boot Record.
#[derive(Debug)]
pub struct Mbr {
    mbr: mbrman::MBR,
    path: PathBuf,
}

impl Mbr {
    /// Create new MBR for given file with a single partition for the whole drive.
    pub fn new(path: PathBuf, disk_size: u32) -> Result<Self> {
        let mut header = MBRHeader::new(DISK_SIGNATURE);

        // First sector stores MBR, so we subtract it from partition size
        let partition_size = (disk_size - 1 * SECTOR_SIZE) / SECTOR_SIZE;

        header.partition_1 = MBRPartitionEntry {
            boot: mbrman::BOOT_ACTIVE,
            first_chs: mbrman::CHS::empty(),
            sys: 0x83, // Linux native file system
            last_chs: mbrman::CHS::empty(),
            starting_lba: 1, // sectors
            sectors: partition_size,
        };

        let mbr_desc = mbrman::MBR {
            sector_size: SECTOR_SIZE,
            header,
            logical_partitions: Vec::new(),
            align: ALIGN,
            cylinders: 0,
            heads: 0,
            sectors: 0,
            disk_size,
        };

        let mut mbr = Self {
            mbr: mbr_desc,
            path,
        };
        mbr.write()?;
        Ok(mbr)
    }

    /// Write current description to disk image.
    pub fn write(&mut self) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .context("open disk image file")?;
        self.mbr
            .write_into(&mut file)
            .context("write MBR to disk image")?;
        Ok(())
    }

    /// MBR description.
    pub fn mbr(&self) -> &mbrman::MBR {
        &self.mbr
    }

    /// Mutable reference to MBR description.
    pub fn mbr_mut(&mut self) -> &mut mbrman::MBR {
        &mut self.mbr
    }

    /// Write MBR bootcode to the disk image.
    pub fn write_bootcode(&mut self, bootcode: [u8; BOOTCODE_SIZE]) -> Result<()> {
        self.mbr.header.bootstrap_code = bootcode;
        self.write()
    }

    /// Path to disk image.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

/// Create Master Boot Record.
pub struct CreateMBR;

impl Step<LinuxVMBuildContext> for CreateMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating Master Boot Record");
        let image_file = ctx
            .0
            .get::<ImageFile>("image_file")
            .ok_or(anyhow!("cannot create partitions: disk image not found"))?;

        let mbr = Mbr::new(image_file.path().to_path_buf(), ctx.opts().image_size)
            .context("create MBR")?;
        ctx.0.set("mbr", Box::new(mbr));

        Ok(())
    }
}

// pub struct CreateGPTPartitions;

// impl Step for CreateGPTPartitions {
//     type Context = LinuxVMBuildContext;

//     fn run(&mut self, ctx: &mut Self::Context) -> anyhow::Result<()> {
//         let image_file = ctx
//             .image_file
//             .as_ref()
//             .ok_or(anyhow!("cannot create partitions: disk image not found"))?;
//         let mut file = std::fs::OpenOptions::new()
//             .write(true)
//             .open(image_file.path())
//             .context("open image file")?;

//         // Create a protective MBR at LBA0
//         let mbr =
//             gpt::mbr::ProtectiveMBR::with_lb_size((image_file.size() as u32 / SECTOR_SIZE) - 1);
//         mbr.overwrite_lba0(&mut file).context("write MBR")?;
//         drop(file);

//         let mut gdisk = gpt::GptConfig::default()
//             .writable(true)
//             .logical_block_size(gpt::disk::LogicalBlockSize::Lb512)
//             .create(image_file.path())
//             .context("create GptDisk")?;

//         // At this point, gdisk.primary_header() and gdisk.backup_header() are populated...
//         gdisk
//             .add_partition("test1", 1024 * 12, gpt::partition_types::BASIC, 0, None)
//             .context("add test1 partition")?;
//         gdisk
//             .add_partition("test2", 1024 * 18, gpt::partition_types::LINUX_FS, 0, None)
//             .context("add test2 partition")?;

//         gdisk.write().context("write GPT partition table to disk")?;

//         Ok(())
//     }
// }
