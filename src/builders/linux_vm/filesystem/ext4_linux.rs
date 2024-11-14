use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::fs;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::linux_vm::LinuxVMBuildContext;
use crate::builders::Step;

/// EXT4 filesystem handler on Linux.
pub struct FileSystem {
    /// Partition start offset in bytes.
    start: u64,

    /// Partition end offset in bytes (excluded).
    end: u64,

    /// Path to image file.
    path: PathBuf,
}

impl FileSystem {
    /// Partition start offset in bytes.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Partition end offset in bytes (excluded).
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Path to image file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn create() -> Result<Self> {
        todo!()
    }

    pub fn read(mbr: &Mbr) -> Result<Self> {
        let path = mbr.path().to_path_buf();
        let mbr = mbr.mbr();
        let partition = &mbr.header.partition_1;
        let sector_size: u64 = mbr.sector_size.into();
        let start: u64 = u64::from(partition.starting_lba) * sector_size; // bytes
        let end: u64 = start + u64::from(partition.sectors) * sector_size + 1; // bytes
        debug!(
            "checking filesystem on {} (offset={})",
            path.display(),
            start
        );
        run_command(
            [
                "e2fsck",
                "-n",
                &format!("{}?offset={}", path.display(), start),
            ],
            false,
        )
        .context("running filesystem check")?;
        Ok(Self { start, end, path })
    }

    pub fn resize(&mut self, new_size: u64) -> Result<()> {
        todo!()
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

/// Create new filesystem on partition.
pub struct Create;

impl Step<LinuxVMBuildContext> for Create {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating EXT4 filesystem on the partition");
        Ok(())
    }
}

/// Read existing filesystem from partition.
pub struct Read;

impl Step<LinuxVMBuildContext> for Read {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("reading EXT4 filesystem on the partition");
        let mbr = ctx
            .0
            .get::<Mbr>("mbr")
            .ok_or(anyhow!("cannot read filesystem: MBR handler not found"))?;
        let fs = FileSystem::read(mbr)?;
        ctx.0.set("fs", Box::new(fs));
        Ok(())
    }
}
