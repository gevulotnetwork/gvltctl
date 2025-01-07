use anyhow::{anyhow, Context, Result};
use log::info;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::{LinuxVMBuildContext, LinuxVMBuilderError};
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
}

impl FileSystemHandler for Ext4 {
    const BLOCK_SIZE: u64 = 0x1000;

    fn offset(&self) -> u64 {
        self.offset
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Run filesystem check.
    ///
    /// Wrapper for `e2fsck -f -n IMAGE?offset=OFFSET`
    fn check(&self) -> Result<()> {
        let mut target = self.path.as_os_str().to_os_string();
        target.push(OsStr::new(&format!("?offset={}", self.offset)));
        run_command([
            OsStr::new("e2fsck"),
            OsStr::new("-f"),
            OsStr::new("-n"),
            target.as_os_str(),
        ])
        .context("filesystem check failed")?;
        Ok(())
    }

    /// Resize filesystem.
    ///
    /// `new_size` - new size of the filesystem in blocks.
    ///
    /// Wrapper for `resize2fs`.
    fn resize(&self, new_size: u64) -> Result<()> {
        // HACK: becase resize2fs IMAGE?offset=OFFSET produces broken filesystem,
        // we copy the filesystem into temp file, temporarily stripping pre-fs sectors,
        // then we resize it there and copy back.

        // Copy filesystem into temp file
        let tmp_dir =
            tempdir::TempDir::new("linux-vm-fs").context("failed to create temp directory")?;
        let tmp_path = tmp_dir.path().join("tmpfs");
        let mut tmp_file =
            fs::File::create_new(&tmp_path).context("failed to create temp filesystem image")?;

        let mut cur_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .context("failed to open image file")?;
        cur_file
            .seek(SeekFrom::Start(self.offset))
            .context("failed to seek image file")?;

        io::copy(&mut cur_file, &mut tmp_file)
            .context("failed to copy filesystem into temp image")?;

        // Close file to avoid issues when resizing fs.
        drop(tmp_file);

        // Resize filesystem in temp file
        // Size of the filesystem in blocks (rounded to floor)
        run_command([
            OsStr::new("resize2fs"),
            // avoid checks
            OsStr::new("-f"),
            // path
            tmp_path.as_os_str(),
            // fs size
            OsStr::new(new_size.to_string().as_str()),
        ])?;

        // Copy filesystem back to original image file
        cur_file
            .seek(SeekFrom::Start(self.offset))
            .context("failed to seek image file")?;
        let mut tmp_file =
            fs::File::open(&tmp_path).context("failed to open temp filesystem image")?;

        io::copy(&mut tmp_file, &mut cur_file)
            .context("failed to copy filesystem from temp image")?;

        Ok(())
    }
}

/// Create new EXT4 filesystem on partition.
///
/// # Context variables required
/// - `mbr`
///
/// # Context variables defined
/// - `fs`
pub struct Create;

impl Step<LinuxVMBuildContext> for Create {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating EXT4 filesystem on the partition");

        let mbr = ctx
            .get::<Mbr>("mbr")
            .ok_or(LinuxVMBuilderError::invalid_context(
                "create EXT4 filesystem",
                "MBR handler",
            ))?;

        let path = mbr.path().to_path_buf();

        // Calculate fs partition offset and size from MBR data
        let mbr = mbr.mbr();
        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let offset: u64 = u64::from(partition.starting_lba) * sector_size; // bytes
        let partition_size: u64 = u64::from(partition.sectors) * sector_size; // bytes

        let fs =
            Ext4::create(path, offset, partition_size).context("failed to create filesystem")?;

        ctx.set("fs", Box::new(fs));

        Ok(())
    }
}

/// Read existing filesystem from partition.
///
/// # Context variables required
/// - `mbr`
///
/// # Context variables defined
/// - `fs`
pub struct UseExisting;

impl Step<LinuxVMBuildContext> for UseExisting {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using existing EXT4 filesystem on the partition");

        let mbr = ctx
            .get::<Mbr>("mbr")
            .ok_or(LinuxVMBuilderError::invalid_context(
                "read EXT4 filesystem",
                "MBR handler",
            ))?;

        let path = mbr.path().to_path_buf();

        // Calculate fs partition offset from MBR data
        let mbr = mbr.mbr();
        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let offset: u64 = u64::from(partition.starting_lba) * sector_size; // bytes

        let fs = Ext4::new(path, offset);
        fs.check()?;

        ctx.set("fs", Box::new(fs));

        Ok(())
    }
}
