use anyhow::{Context, Result};
use log::info;

use crate::builders::Step;

use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::linux_vm::LinuxVMBuildContext;

#[cfg_attr(target_os = "linux", path = "ext4_linux.rs")]
#[cfg_attr(target_os = "macos", path = "ext4_macos.rs")]
pub mod ext4;
pub mod fat32;
pub mod squashfs;

/// Create FAT32 filesystem on boot partition.
/// The filesystem will utilize all of the partition size.
///
/// # Context variables required:
/// - `image-file`
/// - `boot-partition-number`
pub struct CreateBootFs;

impl Step<LinuxVMBuildContext> for CreateBootFs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let partition_idx = *ctx
            .get::<usize>("boot-partition-number")
            .expect("boot-partition-number");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;

        info!("creating FAT32 on boot partition #{}", partition_idx);

        let (start, end) = mbr_adapter
            .partition_limits(partition_idx)
            .context("failed to get partition info")?;

        fat32::Fat32::format(mbr_adapter.path(), start, end)
            .context("failed to create FAT32 filesystem")?;

        info!("FAT32 filesystem created");

        Ok(())
    }
}

/// Read FAT32 filesystem on boot partition.
/// This step ensures that there is a valid FAT32 on the partition.
///
/// # Context variables required:
/// - `image-file`
/// - `boot-partition-number`
pub struct ReadBootFs;

impl Step<LinuxVMBuildContext> for ReadBootFs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("reading filesystem on boot partition");

        let partition_idx = *ctx
            .get::<usize>("boot-partition-number")
            .expect("boot-partition-number");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;

        let (start, end) = mbr_adapter
            .partition_limits(partition_idx)
            .context("failed to get partition info")?;
        let _ = fat32::Fat32::read_from(image_file.path(), start, end);

        info!("found FAT32 filesystem on partition #{}", partition_idx);

        Ok(())
    }
}
