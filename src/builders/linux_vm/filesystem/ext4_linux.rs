use anyhow::{anyhow, bail, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

use super::FileSystemHandler;

/// EXT4 filesystem handler on Linux.
pub struct Ext4 {
    /// Path to image file.
    path: PathBuf,

    /// Partition start offset in bytes.
    offset: u64,
}

impl Ext4 {
    /// EXT4 block size.
    pub const BLOCK_SIZE: u64 = 0x1000;

    pub fn new(path: PathBuf, offset: u64) -> Self {
        Self { path, offset }
    }

    /// Create new filesystem.
    pub fn create(path: PathBuf, offset: u64, partition_size: u64) -> Result<Self> {
        // Size of the filesystem in blocks (rounded to floor)
        let size = partition_size / Self::BLOCK_SIZE;

        run_command([
            OsStr::new("mkfs.ext4"),
            // quiet
            OsStr::new("-q"),
            // block size
            OsStr::new("-b"),
            OsStr::new(Self::BLOCK_SIZE.to_string().as_str()),
            // don't reserve blocks for super-user
            OsStr::new("-m"),
            OsStr::new("0"),
            // offset of the filesystem partition on the disk
            OsStr::new("-E"),
            OsStr::new(&format!("offset={}", offset)),
            // path
            path.as_os_str(),
            // fs size
            OsStr::new(size.to_string().as_str()),
        ])?;

        Ok(Self { path, offset })
    }

    /// Check filesystem.
    pub fn check(&self) -> Result<()> {
        let mut target = self.path.as_os_str().to_os_string();
        target.push(OsStr::new(&format!("?offset={}", self.offset)));
        debug!("checking filesystem on {}", target.to_string_lossy());
        run_command([
            OsStr::new("e2fsck"),
            OsStr::new("-f"),
            OsStr::new("-n"),
            target.as_os_str(),
        ])
        .context("filesystem check failed")?;
        Ok(())
    }

    /// Get current writable free space of filesystem in bytes.
    pub fn free_space(&self) -> Result<u64> {
        let (output, _) = run_command([
            "dumpe2fs",
            &format!("{}?offset={}", self.path.display(), self.offset),
        ])
        .context("getting filesystem size")?;
        let free_blocks = output
            .lines()
            .find_map(|line| line.strip_prefix("Free blocks:"))
            .ok_or(anyhow!("failed to get number of free blocks in filesystem"))?
            .trim_start()
            .trim_end()
            .parse::<u64>()
            .context("failed to parse number of free blocks in filesystem")?;
        let block_size = output
            .lines()
            .find_map(|line| line.strip_prefix("Block size:"))
            .ok_or(anyhow!("failed to get block size in filesystem"))?
            .trim_start()
            .trim_end()
            .parse::<u64>()
            .context("failed to parse block size in filesystem")?;
        Ok(free_blocks * block_size)
    }

    /// Resize filesystem. `new_size` is given in bytes.
    pub fn resize(&self, new_size: u64) -> Result<()> {
        // Size of the filesystem in blocks (rounded to floor)
        let size = new_size / Self::BLOCK_SIZE;
        let mut target = self.path.as_os_str().to_os_string();
        target.push(OsStr::new(&format!("?offset={}", self.offset)));

        run_command([
            OsStr::new("resize2fs"),
            // avoid checks
            OsStr::new("-f"),
            // path
            &target,
            // fs size
            OsStr::new(size.to_string().as_str()),
        ])?;
        Ok(())
    }

    /// Copy file from source to destination.
    pub fn copy_file<P, Q>(&self, source: P, destination: Q) -> Result<u64>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        fs::copy(source, destination).map_err(Into::into)
    }
}

impl FileSystemHandler for Ext4 {
    fn offset(&self) -> u64 {
        self.offset
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

/// Create new filesystem on partition.
pub struct Create;

impl Step<LinuxVMBuildContext> for Create {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating EXT4 filesystem on the partition");

        let mbr = ctx
            .0
            .get::<Mbr>("mbr")
            .ok_or(anyhow!("cannot create filesystem: MBR handler not found"))?;

        let path = mbr.path().to_path_buf();

        // Calculate fs partition offset and size from MBR data
        let mbr = mbr.mbr();
        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let offset: u64 = u64::from(partition.starting_lba) * sector_size; // bytes
        let partition_size: u64 = u64::from(partition.sectors) * sector_size; // bytes

        let fs =
            Ext4::create(path, offset, partition_size).context("failed to create filesystem")?;

        ctx.0.set("fs", Box::new(fs));

        Ok(())
    }
}

/// Read existing filesystem from partition.
pub struct Check;

impl Step<LinuxVMBuildContext> for Check {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using existing EXT4 filesystem on the partition");

        let mbr = ctx
            .0
            .get::<Mbr>("mbr")
            .ok_or(anyhow!("cannot read filesystem: MBR handler not found"))?;

        let path = mbr.path().to_path_buf();

        // Calculate fs partition offset from MBR data
        let mbr = mbr.mbr();
        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let offset: u64 = u64::from(partition.starting_lba) * sector_size; // bytes

        let fs = Ext4::new(path, offset);
        fs.check()?;

        ctx.0.set("fs", Box::new(fs));

        Ok(())
    }
}
