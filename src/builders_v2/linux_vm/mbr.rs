use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use mbrman::{MBRHeader, MBRPartitionEntry};
use std::fs;
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::image_file::ImageFile;
use super::{LinuxVMBuildContext, LinuxVMBuilderError};

/// Master Boot Record handler.
#[derive(Clone, Debug)]
pub struct Mbr {
    mbr: mbrman::MBR,
    path: PathBuf,
}

impl Mbr {
    pub const DISK_SIGNATURE: [u8; 4] = [b'G', b'V', b'L', b'T'];
    pub const SECTOR_SIZE: u32 = 512;
    pub const ALIGN: u32 = 1;
    pub const BOOTCODE_SIZE: usize = 440;

    /// Create new MBR for given file with a single partition for the whole drive.
    ///
    /// File size is given in bytes. If the size is not divisable by sector size, disk size will be
    /// rounded to floor.
    pub fn new(path: PathBuf, file_size: u64) -> Result<Self> {
        let mut header = MBRHeader::new(Self::DISK_SIGNATURE);

        // Size of the disk in sectors
        let disk_size = (file_size / Self::SECTOR_SIZE as u64)
            .try_into()
            .map_err(|_| anyhow!("disk size is too big (max supported by MBR is 2 TiB)"))?;

        // First sector stores VBR, so we subtract it from partition size
        let partition_size = disk_size - 1;

        header.partition_1 = MBRPartitionEntry {
            boot: mbrman::BOOT_ACTIVE,
            first_chs: mbrman::CHS::empty(),
            sys: 0x83, // Linux native file system
            last_chs: mbrman::CHS::empty(),
            starting_lba: 1, // sectors
            sectors: partition_size,
        };

        let mbr_desc = mbrman::MBR {
            sector_size: Self::SECTOR_SIZE,
            header,
            logical_partitions: Vec::new(),
            align: Self::ALIGN,
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

    /// Read MBR from file.
    pub fn from_file(path: PathBuf) -> Result<Self> {
        let mut f = fs::File::open(&path).context("failed to open disk image file")?;
        let mbr = mbrman::MBR::read_from(&mut f, Self::SECTOR_SIZE)
            .context("failed to read MBR from disk image")?;
        Ok(Self { mbr, path })
    }

    /// Write current description to disk image.
    pub fn write(&mut self) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .context("failed to open disk image file")?;
        self.mbr
            .write_into(&mut file)
            .context("failed to write MBR to disk image")?;
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
    pub fn write_bootcode(&mut self, bootcode: [u8; Self::BOOTCODE_SIZE]) -> Result<()> {
        self.mbr.header.bootstrap_code = bootcode;
        self.write()
    }

    /// Extend partition-1 returning old size. Disk size value is also updated.
    /// Changes are written to disk.
    /// `extend` is given in bytes.
    pub fn extend_partition(&mut self, extend: u64) -> Result<()> {
        let extend: u32 = (extend / Self::SECTOR_SIZE as u64)
            .try_into()
            .map_err(|_| anyhow!("disk size is too big"))?;
        self.mbr.disk_size += extend;
        self.mbr.header.partition_1.sectors += extend;
        self.write()
    }

    /// Path to disk image.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

/// Create Master Boot Record.
///
/// # Context variables required
/// - `image-file`
///
/// # Context variables defined:
/// - `mbr`
pub struct CreateMBR;

impl Step<LinuxVMBuildContext> for CreateMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating Master Boot Record");
        let image_file =
            ctx.get::<ImageFile>("image-file")
                .ok_or(LinuxVMBuilderError::invalid_context(
                    "create partitions",
                    "disk image",
                ))?;

        let mbr = Mbr::new(image_file.path().to_path_buf(), image_file.size()?.as_u64())
            .context("failed to create MBR")?;

        debug!(
            "disk size={}s, sector size={}",
            mbr.mbr().disk_size,
            mbr.mbr().sector_size
        );
        debug!(
            "created partition: start={}s, size={}s",
            mbr.mbr().header.partition_1.starting_lba,
            mbr.mbr().header.partition_1.sectors
        );
        ctx.set("mbr", Box::new(mbr));

        Ok(())
    }
}

/// Read Master Boot Record.
///
/// # Context variables required
/// - `image-file`
///
/// # Context variables defined:
/// - `mbr`
pub struct ReadMBR;

impl Step<LinuxVMBuildContext> for ReadMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("reading Master Boot Record");
        let image_file = ctx
            .get::<ImageFile>("image-file")
            .ok_or(anyhow!("cannot create partitions: disk image not found"))?;

        let mbr = Mbr::from_file(image_file.path().to_path_buf()).context("failed to read MBR")?;
        ctx.set("mbr", Box::new(mbr));

        Ok(())
    }
}
