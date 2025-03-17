use anyhow::{Context, Result};
use log::{debug, info, trace};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::directory::Directory;
use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

/// EXT4 filesystem adapter on Linux.
pub struct Ext4<'a> {
    /// Path to image file.
    #[allow(unused)]
    path: &'a Path,

    /// Partition start offset in bytes.
    #[allow(unused)]
    offset: u64,
}

impl<'a> Ext4<'a> {
    const BLOCK_SIZE: u64 = 0x1000;
    const INODE_SIZE: u64 = 0x100;
    const INODE_RATIO: u64 = 0x4000;

    /// Round up a value to the size of EXT4 block.
    pub fn round_up(value: u64) -> u64 {
        (value / Self::BLOCK_SIZE * Self::BLOCK_SIZE)
            + (value % Self::BLOCK_SIZE != 0) as u64 * Self::BLOCK_SIZE
    }

    #[allow(unused)]
    pub fn new(path: &'a Path, start: u64, _end: u64) -> Self {
        Self {
            path,
            offset: start,
        }
    }

    #[allow(unused)]
    pub fn offset(&self) -> u64 {
        self.offset
    }

    #[allow(unused)]
    pub fn path(&self) -> &'a Path {
        self.path
    }

    /// Format new EXT4 filesystem.
    pub fn format(path: &'a Path, offset: u64, partition_size: u64) -> Result<()> {
        // Size of the filesystem in blocks (rounded to floor)
        let size = partition_size / Ext4::BLOCK_SIZE;

        run_command([
            OsStr::new("mkfs.ext4"),
            // quiet
            OsStr::new("-q"),
            // block size
            OsStr::new("-b"),
            OsStr::new(Ext4::BLOCK_SIZE.to_string().as_str()),
            // inode size
            OsStr::new("-I"),
            OsStr::new(Ext4::INODE_SIZE.to_string().as_str()),
            // inode ratio
            OsStr::new("-i"),
            OsStr::new(Ext4::INODE_RATIO.to_string().as_str()),
            // don't reserve blocks for super-user
            OsStr::new("-m"),
            OsStr::new("0"),
            // deactivate journal and resize_inode
            OsStr::new("-O"),
            OsStr::new("^has_journal,^resize_inode"),
            // offset of the filesystem partition on the disk
            OsStr::new("-E"),
            OsStr::new(&format!("offset={}", offset)),
            // path
            path.as_os_str(),
            // fs size
            OsStr::new(size.to_string().as_str()),
        ])?;

        Ok(())
    }

    /// Run filesystem check.
    ///
    /// Wrapper for `e2fsck -f -n IMAGE?offset=OFFSET`
    #[allow(unused)]
    pub fn check(&self) -> Result<()> {
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
    #[allow(unused)]
    pub fn resize(&self, new_size: u64) -> Result<()> {
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
            .open(self.path)
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

/// Evaluate the size of the partition to store this filesystem.
///
/// # Context variables required:
/// - `root-fs`
///
/// # Context variables defined:
/// - `root-partition-size`: [`u64`]
pub struct EvaluateSize;

impl Step<LinuxVMBuildContext> for EvaluateSize {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("evaluating size of partition for EXT4");
        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");
        let dir = Directory::from_path(rootfs)?;

        let size_bytes = Ext4::round_up(dir.size()?);
        trace!(
            "total data bytes: {} ({} blocks)",
            bytesize::ByteSize::b(size_bytes).to_string_as(true),
            size_bytes / Ext4::BLOCK_SIZE
        );

        // FIXME: looks like this estimation doesn't work
        let partition_size =
            (size_bytes * Ext4::INODE_RATIO) / (Ext4::INODE_RATIO - Ext4::INODE_SIZE);
        // This is a very dirty ugly solution and needs to be fixed
        let partition_size = partition_size * 2;

        debug!(
            "size of partition for EXT4: {}",
            bytesize::ByteSize::b(partition_size).to_string_as(true),
        );

        ctx.set("root-partition-size", Box::new(partition_size));

        Ok(())
    }
}

/// Create new EXT4 filesystem on root partition.
///
/// # Context variables required
/// - `image-file`
/// - `root-partition-number`
pub struct Format;

impl Step<LinuxVMBuildContext> for Format {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating EXT4 filesystem on the partition");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let root_partition_number = *ctx
            .get::<usize>("root-partition-number")
            .expect("root-partition-number");

        let mbr_adapter = Mbr::read_from(image_file.path())?;
        let (start, end) = mbr_adapter.partition_limits(root_partition_number)?;

        Ext4::format(image_file.path(), start, end - start)
            .context("failed to create filesystem")?;

        Ok(())
    }
}
