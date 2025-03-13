use anyhow::{Context, Result};
use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions};
use fscommon::StreamSlice;
use std::fs;
use std::path::Path;

/// FAT32 filesystem adapter.
///
/// This adapter doesn't assume any mounting.
/// Reading/writing can be done with [`Self::fs()`] handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fat32<'a> {
    /// Path to image file.
    path: &'a Path,

    /// Inclusive offset of the filesystem in disk image.
    start: u64,

    /// Exclusive offset of the filesystem in disk image.
    end: u64,
}

impl<'a> Fat32<'a> {
    /// Format new FAT32 filesystem.
    ///
    /// # Arguments
    /// - `path` - image file
    /// - `start` - inclusive starting offset of the partition on disk image in bytes
    /// - `end` - exclusive ending offset of the partition on disk image in bytes
    pub fn format(path: &'a Path, start: u64, end: u64) -> Result<()> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .context("failed to open image file")?;
        let slice = StreamSlice::new(file, start, end).context("failed to seek image file")?;
        let options = FormatVolumeOptions::new().fat_type(FatType::Fat32);
        fatfs::format_volume(slice, options).context("failed to format FAT32 filesystem")?;
        Ok(())
    }

    pub fn resize() -> Result<()> {
        // This turns out to be tricky
        todo!("FAT32 resizing")
    }

    /// Read existing FAT32 filesystem.
    ///
    /// # Arguments
    /// - `path` - image file
    /// - `start` - inclusive starting offset of the partition on disk image in bytes
    /// - `end` - exclusive ending offset of the partition on disk image in bytes
    pub fn read_from(path: &'a Path, start: u64, end: u64) -> Result<Self> {
        let file = fs::File::open(path).context("failed to open image file")?;
        let slice = StreamSlice::new(file, start, end).context("failed to seek image file")?;
        let _ = FileSystem::new(slice, FsOptions::new()).context("failed to read FAT32")?;
        Ok(Self { path, start, end })
    }

    /// Path to image file.
    #[allow(unused)]
    pub fn path(&self) -> &'a Path {
        self.path
    }

    /// Inclusive offset of the filesystem in disk image.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Exclusive offset of the filesystem in disk image.
    #[allow(unused)]
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Filesystem handler.
    ///
    /// This filesystem adapter doesn't assume mounting.
    /// Instead this handler can be used to read/write files and directories.
    pub fn fs(&self) -> Result<FileSystem<StreamSlice<fs::File>>> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .context("failed to open image file")?;
        let slice =
            StreamSlice::new(file, self.start, self.end).context("failed to seek image file")?;
        Ok(FileSystem::new(slice, FsOptions::new())?)
    }
}
