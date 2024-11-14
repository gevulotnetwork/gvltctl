use anyhow::{anyhow, Context, Result};
use log::{info, trace};
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

/// Master Boot Record handler.
#[derive(Clone, Debug)]
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

    /// Read MBR from file.
    pub fn from_file(path: PathBuf) -> Result<Self> {
        let mut f = fs::File::open(&path).context("open disk image file")?;
        let mbr =
            mbrman::MBR::read_from(&mut f, SECTOR_SIZE).context("read MBR from disk image")?;
        Ok(Self { mbr, path })
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

pub struct ReadMBR;

impl Step<LinuxVMBuildContext> for ReadMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("reading Master Boot Record");
        let image_file = ctx
            .0
            .get::<ImageFile>("image_file")
            .ok_or(anyhow!("cannot create partitions: disk image not found"))?;

        let mbr = Mbr::from_file(image_file.path().to_path_buf()).context("read MBR")?;
        // format!("{:#?}", &mbr)
        //     .lines()
        //     .for_each(|line| trace!("{}", line));
        ctx.0.set("mbr", Box::new(mbr));

        Ok(())
    }
}
