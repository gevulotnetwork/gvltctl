use anyhow::{Context, Result};
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::marker::PhantomData;

use crate::builders::linux_vm::{LinuxVMBuildContext, LinuxVMBuilderError as Error};
use crate::builders::Step;

use super::filesystem::FileSystemHandler;
use super::image_file::ImageFile;
use super::kernel::Kernel;
use super::mbr::Mbr;
use super::nvidia::NvidiaDriversFs;
use super::rootfs::RootFS;

/// This step is used to resize the base VM image to proper size.
/// Is should be called after all sources are defined.
/// It will extend image file size, partition size and then filesystem size.
///
/// # Context variables required
/// - `image-file`
/// - `mbr`
/// - `fs`
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
    /// Padding to add for safety (in filesystem blocks).
    ///
    /// This value is just an heuristics. I tried multiple different ones to get rid of
    /// fragmentation issues and this one worked.
    /// Unfortunatelly it adds 16 MiB of memory, which is not good. Should be fixed in the future.
    const PAD: u64 = 4096;
    // FIXME: reduce padding avoiding "no space left error" on kernel (and other stuff) installation.

    /// This function calculates total space required on the target filesystem
    /// to store everything we want.
    /// The size is aligned with filesystem block size.
    fn calculate_required_space(ctx: &mut LinuxVMBuildContext) -> Result<ByteSize> {
        let mut required = ByteSize::b(0);

        if let Some(kernel) = ctx.get::<Kernel>("kernel") {
            let size = kernel.size();
            trace!(
                "found kernel: {} ({} bytes)",
                size.to_string_as(true),
                size.as_u64()
            );
            required += kernel.size();
        }

        if let Some(rootfs) = ctx.get::<RootFS>("rootfs") {
            let size = rootfs.size()?;
            trace!(
                "found rootfs: {} ({} bytes)",
                size.to_string_as(true),
                size.as_u64()
            );
            required += size;
        }

        if let Some(nvidia_drivers) = ctx.get::<NvidiaDriversFs>("nvidia-drivers") {
            let size = nvidia_drivers.size()?;
            trace!(
                "found NVIDIA drivers: {} ({} bytes)",
                ByteSize::b(size),
                size
            );
            required += ByteSize::b(size);
        }

        // This is an approximate size (with kmod). We do not calculate it precisely because we are
        // pretty sure it won't change for now.
        // TODO: calculate this size precisely.
        let mia_size = ByteSize::mb(5);
        trace!("MIA (constant size): {}", mia_size);
        required += mia_size;

        let ext_linux_cfg_size = ByteSize::kb(1); // approximately
        required += ext_linux_cfg_size;
        trace!(
            "required size: {} ({} bytes)",
            required.to_string_as(true),
            required.as_u64()
        );

        // Align to block size
        required = ByteSize::b((required.as_u64() / F::BLOCK_SIZE + 1) * F::BLOCK_SIZE);
        trace!(
            "aligned size: {} ({} bytes)",
            required.to_string_as(true),
            required.as_u64()
        );

        // Add padding
        required += ByteSize::b(Self::PAD * F::BLOCK_SIZE);
        trace!(
            "padded size: {} ({} bytes)",
            required.to_string_as(true),
            required.as_u64()
        );

        Ok(required)
    }
}

impl<F> Step<LinuxVMBuildContext> for ResizeAll<F>
where
    F: FileSystemHandler + 'static,
{
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("resizing VM image");

        let current_image_size = ctx
            .get_mut::<ImageFile>("image-file")
            .ok_or(Error::invalid_context(
                "get image size",
                "image file handler",
            ))?
            .size()?;
        trace!(
            "current image file size: {} bytes",
            current_image_size.as_u64()
        );

        let current_part_size = ctx
            .get_mut::<Mbr>("mbr")
            .ok_or(Error::invalid_context("get partition size", "MBR handler"))?
            .mbr()
            .header
            .partition_1
            .sectors
            * Mbr::SECTOR_SIZE;
        trace!("current partition size: {} bytes", current_part_size);

        // Calculate total space required on target filesystem (in bytes)
        let space_required = Self::calculate_required_space(ctx)?;
        debug!("space required on target filesystem: {}", space_required);

        let extend = space_required.as_u64() - current_part_size as u64;
        trace!("extending disk and partition by {} bytes", extend);

        // Extend image file
        let image_file = ctx
            .get_mut::<ImageFile>("image-file")
            .ok_or(Error::invalid_context("resize image", "image file handler"))?;
        image_file
            .extend(ByteSize::b(extend))
            .context("failed to extend image file")?;

        // Extend partition
        // Partition will have the same size as filesystem
        let mbr = ctx
            .get_mut::<Mbr>("mbr")
            .ok_or(Error::invalid_context("resize partition", "MBR handler"))?;
        mbr.extend_partition(extend)
            .context("failed to extend partition")?;
        trace!(
            "new partition size: {} bytes",
            mbr.mbr().header.partition_1.sectors * Mbr::SECTOR_SIZE
        );

        // Resize filesystem
        let fs = ctx.get::<F>("fs").ok_or(Error::invalid_context(
            "resize filesystem",
            "filesystem handler",
        ))?;
        debug!(
            "resizing filesystem to {} bytes ({} blocks)",
            space_required.as_u64(),
            space_required.as_u64() / F::BLOCK_SIZE
        );
        debug_assert_eq!(space_required.as_u64() % F::BLOCK_SIZE, 0);
        fs.resize(space_required.as_u64() / F::BLOCK_SIZE)
            .context("failed to resize filesystem")?;

        trace!("running filesystem check");
        fs.check()?;

        debug!("resize completed");
        Ok(())
    }
}

// TODO: probably it would be better if every entity that needs space on drive will add it to
// some context variable and this step will simply read it. In that case the responsibility for size
// calculation will be on the side of that entities.
