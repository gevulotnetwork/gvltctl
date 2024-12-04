use anyhow::{Context, Result};
use bytesize::ByteSize;
use log::{debug, info};

use crate::builders::linux_vm::{LinuxVMBuildContext, LinuxVMBuilderError as Error};
use crate::builders::Step;

use super::filesystem::Ext4;
use super::image_file::ImageFile;
use super::kernel::Kernel;
use super::mbr::Mbr;
use super::rootfs::RootFS;

/// This step is used to resize the base VM image to proper size.
/// Is should be called after all sources are defined.
/// It will extend image file size, partition size and then filesystem size.
pub struct ResizeAll;

/// Margin to add for safety (1%).
const MARGIN: f64 = 0.5;

impl Step<LinuxVMBuildContext> for ResizeAll {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("resizing VM image");

        // Collect total size required (in bytes)
        let mut total_size = ByteSize::b(0);
        if let Some(kernel) = ctx.0.get::<Kernel>("kernel") {
            debug!("found kernel: {} bytes", kernel.size());
            total_size += kernel.size();
        }
        if let Some(rootfs) = ctx.0.get::<RootFS>("rootfs") {
            debug!("found rootfs: {} bytes", rootfs.size());
            total_size += rootfs.size();
        }
        debug!("required size: {}", total_size);

        // Add margin for safety (to avoid error when installing stuff like EXLINUX config)
        total_size += ByteSize::b((total_size.as_u64() as f64 * MARGIN) as u64);
        // Align to fs block size
        total_size += Ext4::BLOCK_SIZE - total_size.as_u64() % Ext4::BLOCK_SIZE;
        debug!("desired allocation size with margin: {}", total_size);

        // Extend image file
        let image_file = ctx
            .0
            .get_mut::<ImageFile>("image_file")
            .ok_or(Error::invalid_context("resize image", "image file handler"))?;

        let current_size = image_file.size();
        debug!("current image file size: {}", current_size);

        debug_assert!(total_size > current_size);
        let extend_value = ByteSize::b(total_size.as_u64() - current_size.as_u64());
        debug!("extending image by {}", extend_value);
        image_file.extend(extend_value).context("failed to extend image file")?;
        debug!("new image file size: {}", image_file.size());

        // Extend partition
        let mbr = ctx
            .0
            .get_mut::<Mbr>("mbr")
            .ok_or(Error::invalid_context("resize partition", "MBR handler"))?;

        mbr.extend_partition(extend_value.as_u64()).context("failed to extend partition")?;
        debug!("new partition size: {}s", mbr.mbr().header.partition_1.sectors);

        debug!("extending filesystem");
        // Resize filesystem
        let fs = ctx.0.get::<Ext4>("fs").ok_or(Error::invalid_context(
            "resize filesystem",
            "filesystem handler",
        ))?;

        fs.resize(total_size.as_u64()).context("failed to resize filesystem")?;

        debug!("running filesystem check");
        fs.check()?;

        debug!("resize completed");
        Ok(())
    }
}
