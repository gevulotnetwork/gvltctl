use anyhow::{Context, Result};
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::marker::PhantomData;

use crate::builders::linux_vm::{LinuxVMBuildContext, LinuxVMBuilderError as Error};
use crate::builders::Step;

use super::filesystem::{Ext4, FileSystemHandler};
use super::image_file::ImageFile;
use super::kernel::Kernel;
use super::mbr::Mbr;
use super::rootfs::RootFS;

/// This step is used to resize the base VM image to proper size.
/// Is should be called after all sources are defined.
/// It will extend image file size, partition size and then filesystem size.
#[derive(Debug)]
pub struct ResizeAll<F: FileSystemHandler> {
    phantom_data: PhantomData<F>,
}

impl<F: FileSystemHandler> ResizeAll<F> {
    pub fn new() -> Self {
        Self {
            phantom_data: PhantomData,
        }
    }
}

impl<F> ResizeAll<F>
where
    F: FileSystemHandler,
{
    /// Margin to add for safety in percetages.
    const MARGIN: f64 = 0.5;

    /// This function calculates total space required on the target filesystem
    /// to store everything we want.
    /// The size is aligned with filesystem block size.
    fn calculate_required_space(ctx: &mut LinuxVMBuildContext) -> ByteSize {
        let mut total_size = ByteSize::b(0);

        if let Some(kernel) = ctx.0.get::<Kernel>("kernel") {
            trace!("found kernel: {}", kernel.size());
            total_size += kernel.size();
        }

        if let Some(rootfs) = ctx.0.get::<RootFS>("rootfs") {
            trace!("found rootfs: {}", rootfs.size());
            total_size += rootfs.size();
        }

        let ext_linux_cfg_size = ByteSize::kb(1); // approximately
        total_size += ext_linux_cfg_size;

        // let mia_size = ByteSize::mb(5); // approximately (with kmod)
        // total_size += mia_size;

        // Align to block size
        total_size = ByteSize::b((total_size.as_u64() / F::BLOCK_SIZE + 1) * F::BLOCK_SIZE);

        total_size
    }
}

impl<F> Step<LinuxVMBuildContext> for ResizeAll<F>
where
    F: FileSystemHandler + 'static,
{
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

        let space_required = Self::calculate_required_space(ctx);
        debug!("space required on target filesystem: {}", space_required);

        // Add margin for safety (to avoid error when installing stuff like EXLINUX config)
        total_size += ByteSize::b((total_size.as_u64() as f64 * Self::MARGIN) as u64);
        // Align to fs block size
        total_size += Ext4::BLOCK_SIZE - total_size.as_u64() % Ext4::BLOCK_SIZE;
        debug!(
            "desired allocation size with margin: {} ({} blocks)",
            total_size,
            total_size.as_u64() / F::BLOCK_SIZE
        );

        // Extend image file
        let image_file = ctx
            .0
            .get_mut::<ImageFile>("image_file")
            .ok_or(Error::invalid_context("resize image", "image file handler"))?;

        let current_size = image_file.size()?;
        debug!("current image file size: {}", current_size);

        debug_assert!(total_size > current_size);
        let extend_value = ByteSize::b(total_size.as_u64() - current_size.as_u64());

        // Extend image file
        debug!("extending image by {}", extend_value + ByteSize::mib(5));
        image_file
            .extend(extend_value + ByteSize::mib(5))
            .context("failed to extend image file")?;

        // Extend partition
        let mbr = ctx
            .0
            .get_mut::<Mbr>("mbr")
            .ok_or(Error::invalid_context("resize partition", "MBR handler"))?;

        let current_part_size = mbr.mbr().header.partition_1.sectors * Mbr::SECTOR_SIZE;
        debug!("current partition size: {} bytes", current_part_size);
        let part_ext_size = extend_value + ByteSize::mib(4);
        let expected_new_size = current_part_size as u64 + part_ext_size.as_u64();
        let aligned_new_part_size = ((expected_new_size / F::BLOCK_SIZE) + 1) * F::BLOCK_SIZE;
        let part_ext_size = aligned_new_part_size - current_part_size as u64;

        debug!("extending partition by {} bytes", part_ext_size);
        mbr.extend_partition(part_ext_size)
            .context("failed to extend partition")?;
        let new_partition_size = mbr.mbr().header.partition_1.sectors * Mbr::SECTOR_SIZE;
        debug!(
            "new partition size: {} bytes ({} blocks)",
            new_partition_size,
            new_partition_size as u64 / F::BLOCK_SIZE
        );

        debug!("extending filesystem");
        // Resize filesystem
        let fs = ctx.0.get::<F>("fs").ok_or(Error::invalid_context(
            "resize filesystem",
            "filesystem handler",
        ))?;

        fs.resize(total_size.as_u64() / F::BLOCK_SIZE)
            .context("failed to resize filesystem")?;

        debug!("running filesystem check");
        fs.check()?;

        debug!("resize completed");
        Ok(())
    }
}
