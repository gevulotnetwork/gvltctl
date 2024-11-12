use anyhow::{anyhow, Context, Result};
use fatfs::{FatType, FormatVolumeOptions, StdIoWrapper};
use fscommon::StreamSlice;
use log::info;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use unix_path::{Component, Path as UnixPath};

use crate::builders::Step;

use super::image_file::ImageFile;
use super::mbr::Mbr;
use super::LinuxVMBuildContext;

#[derive(Debug)]
pub struct FileSystem {
    /// Partition start offset in bytes.
    part_start: u64,

    /// Partition end offset in bytes (excluded).
    part_end: u64,

    /// Path to image file.
    path: PathBuf,
}

impl FileSystem {
    pub fn new(part_start: u64, part_end: u64, path: PathBuf) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)?;
        let part_slice =
            StreamSlice::new(file, part_start, part_end).context("get partition slice")?;
        fatfs::format_volume(
            &mut StdIoWrapper::new(part_slice),
            FormatVolumeOptions::new().fat_type(FatType::Fat32),
        )?;

        Ok(Self {
            part_start,
            part_end,
            path,
        })
    }

    /// Partition start offset in bytes.
    pub fn start(&self) -> u64 {
        self.part_start
    }

    /// Partition end offset in bytes (excluded).
    pub fn end(&self) -> u64 {
        self.part_end
    }

    /// Path to image file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Get filesystem internal wrapper. Can be used to read/write files & directories.
    pub fn get_fs(&self) -> Result<fatfs::FileSystem<StdIoWrapper<StreamSlice<fs::File>>>> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)?;
        let buf_stream = StreamSlice::new(file, self.part_start, self.part_end)
            .context("get partition slice")?;
        let fs = fatfs::FileSystem::new(buf_stream, fatfs::FsOptions::new())
            .context("get FAT filesystem")?;
        Ok(fs)
    }

    /// Create directories alongside path (like `mkdir -p`) relative to root (e.g. `boot/syslinux`).
    pub fn create_dir(&self, path: &UnixPath) -> Result<()> {
        let fs = self.get_fs()?;
        let mut cur_dir = fs.root_dir();
        debug_assert!(path.is_relative());
        for component in path.components() {
            debug_assert!(matches!(&component, Component::Normal(_)));
            cur_dir = cur_dir.create_dir(
                component
                    .as_unix_str()
                    .to_str()
                    .ok_or(anyhow!("non-UTF-8 Unix path"))?,
            )?;
        }
        Ok(())
    }

    /// Write file with given path.
    pub fn write_file(&self, path: &UnixPath, content: &[u8]) -> Result<()> {
        let fs = self.get_fs()?;
        let mut file = fs
            .root_dir()
            .create_file(
                path.as_unix_str()
                    .to_str()
                    .ok_or(anyhow!("non-UTF-8 Unix path"))?,
            )
            .context(format!("create file `{}`", path.display()))?;
        file.truncate()
            .context(format!("truncate file `{}`", path.display()))?;
        file.write_all(content)
            .context(format!("write file `{}`", path.display()))?;
        Ok(())
    }
}

pub struct CreateFat;

impl Step<LinuxVMBuildContext> for CreateFat {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating FAT filesystem on the partition");
        let image_file = ctx.0.get::<ImageFile>("image_file").ok_or(anyhow!(
            "cannot create FAT filesystem: disk image not found"
        ))?;
        let mbr = ctx
            .0
            .get::<Mbr>("mbr")
            .ok_or(anyhow!("cannot create FAT filesystem: MBR not found"))?
            .mbr();

        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let start: u64 = u64::from(partition.starting_lba) * sector_size; // bytes
        let end: u64 = start + u64::from(partition.sectors) * sector_size + 1; // bytes

        let fs = FileSystem::new(start, end, image_file.path().to_path_buf())?;
        ctx.0.set("fs", Box::new(fs));
        Ok(())
    }
}
