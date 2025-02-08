use anyhow::{bail, Context};
use bytesize::ByteSize;
use log::{debug, info, trace, warn};
use mbrman::{MBRHeader, MBRPartitionEntry};
use std::fs;
use std::path::Path;

use crate::builders::Step;

use super::image_file::ImageFile;
use super::LinuxVMBuildContext;

/// Master Boot Record adapter error.
#[derive(Debug, thiserror::Error)]
#[error("MBR error: {message}")]
pub struct MbrError {
    message: String,
    #[source]
    pub source: MbrErrorKind,
}

impl MbrError {
    pub fn new(message: String, source: MbrErrorKind) -> Self {
        Self { message, source }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MbrErrorKind {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("MBR error: {0}")]
    MbrError(#[from] mbrman::Error),

    #[error("all MBR partitions are used")]
    AllPartitionsUsed,

    #[error("no space left on drive")]
    NoSpaceLeft,

    #[error("failed to extend the partition")]
    PartitionExtendError,

    #[error("boot partition is missing")]
    MissingBootPartition,
}

/// Master Boot Record adapter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Mbr<'a> {
    path: &'a Path,
}

impl<'a> Mbr<'a> {
    pub const DISK_SIGNATURE: [u8; 4] = [b'G', b'V', b'L', b'T'];
    pub const SECTOR_SIZE: u32 = 512;
    pub const ALIGN: u32 = 2048; // default alignment mbrman::DEFAULT_ALIGN
    pub const BOOTCODE_SIZE: usize = 440;

    /// Round-up to aligned value.
    pub fn round_up(value: u32) -> u32 {
        (value / Self::ALIGN * Self::ALIGN) + (value % Self::ALIGN != 0) as u32 * Self::ALIGN
    }

    /// Create new MBR (msdos table) on given disk image file.
    ///
    /// Disk size is going to be set to the maximum number of sectors fitting into image file.
    pub fn new(path: &'a Path) -> Result<Self, MbrError> {
        let metadata = fs::metadata(path).map_err(|err| {
            MbrError::new("failed to get disk image metadata".to_string(), err.into())
        })?;
        let disk_size = (metadata.len() / Self::SECTOR_SIZE as u64)
            .try_into()
            .unwrap_or_else(|_| {
                let max = ByteSize::b(u32::MAX as u64 * Self::SECTOR_SIZE as u64);
                warn!(
                    "disk image is too big for MBR (MBR supports disks up to {})",
                    max
                );
                warn!("creating MBR for disk size {}", max);
                u32::MAX
            });

        let mbr = mbrman::MBR {
            sector_size: Self::SECTOR_SIZE,
            header: MBRHeader::new(Self::DISK_SIGNATURE),
            logical_partitions: Vec::new(),
            align: Self::ALIGN,
            cylinders: 0,
            heads: 0,
            sectors: 0,
            disk_size,
        };

        let handler = Self { path };
        handler.write(mbr)?;
        Ok(handler)
    }

    /// Read MBR from file.
    pub fn read_from(path: &'a Path) -> Result<Self, MbrError> {
        let mut f = fs::File::open(path).map_err(|err| {
            MbrError::new("failed to open disk image file".to_string(), err.into())
        })?;
        let _ = mbrman::MBR::read_from(&mut f, Self::SECTOR_SIZE).map_err(|err| {
            MbrError::new("failed to read MBR from disk image".to_string(), err.into())
        })?;
        Ok(Self { path })
    }

    /// Write MBR description to disk image.
    pub fn write(&self, mut mbr: mbrman::MBR) -> Result<(), MbrError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .map_err(|err| {
                MbrError::new("failed to open disk image file".to_string(), err.into())
            })?;
        mbr.write_into(&mut file).map_err(|err| {
            MbrError::new("failed to write MBR to disk image".to_string(), err.into())
        })?;
        Ok(())
    }

    /// MBR description.
    pub fn mbr(&self) -> Result<mbrman::MBR, MbrError> {
        let mut f = fs::File::open(&self.path).map_err(|err| {
            MbrError::new("failed to open disk image file".to_string(), err.into())
        })?;
        mbrman::MBR::read_from(&mut f, Self::SECTOR_SIZE).map_err(|err| {
            MbrError::new("failed to read MBR from disk image".to_string(), err.into())
        })
    }

    /// Write MBR bootcode to the disk image.
    pub fn write_bootcode(&self, bootcode: [u8; Mbr::BOOTCODE_SIZE]) -> Result<(), MbrError> {
        let mut mbr = self.mbr()?;
        mbr.header.bootstrap_code = bootcode;
        self.write(mbr)
    }

    /// Change disk size returning old disk size.
    pub fn resize_disk(&self, new_disk_size: u32) -> Result<u32, MbrError> {
        let mut mbr = self.mbr()?;
        let old_disk_size = mbr.disk_size;
        mbr.disk_size = new_disk_size;
        self.write(mbr)?;
        Ok(old_disk_size)
    }

    /// Add new partition.
    ///
    /// # Arguments
    /// - `size` - number of sectors in the partition ([`Self::SECTOR_SIZE`])
    /// - `partition_type` - type of the partition, e.g. `0x83` for Linux filesystem
    /// (see https://en.wikipedia.org/wiki/Partition_type)
    /// - `boot` - whether the partition is bootable or not
    ///
    /// # Returns
    /// Index of created partition
    ///
    /// # Note
    /// It's better to align all partition sizes ([`Self::round_up`]).
    pub fn add_partition(
        &self,
        size: u32,
        partition_type: u8,
        boot: bool,
    ) -> Result<usize, MbrError> {
        if size % Self::ALIGN != 0 {
            warn!(
                "Creating partition of size {}s, which is not aligned ({}s)",
                size,
                Self::ALIGN
            );
            warn!("It's not an error, but it is undesirable");
        }
        let mut mbr = self.mbr()?;

        let idx = mbr
            .iter()
            .find(|(_, partition)| partition.is_unused())
            .map(|(idx, _)| idx)
            .ok_or(MbrError {
                message: "failed to find free partiton".to_string(),
                source: MbrErrorKind::AllPartitionsUsed,
            })?;

        let starting_lba = mbr.find_optimal_place(size).ok_or(MbrError {
            message: "failed to find a place for new partition".to_string(),
            source: MbrErrorKind::NoSpaceLeft,
        })?;

        let boot = if boot {
            mbrman::BOOT_ACTIVE
        } else {
            mbrman::BOOT_INACTIVE
        };

        mbr[idx] = MBRPartitionEntry {
            boot,
            first_chs: mbrman::CHS::empty(),
            sys: partition_type,
            last_chs: mbrman::CHS::empty(),
            starting_lba,
            sectors: size,
        };
        self.write(mbr)?;

        Ok(idx)
    }

    /// Return inclusive start offset and exclusive end offset of the partition in bytes.
    pub fn partition_limits(&self, partition_idx: usize) -> Result<(u64, u64), MbrError> {
        let mbr = self.mbr()?;
        let partition = &mbr[partition_idx];
        let start = partition.starting_lba as u64 * mbr.sector_size as u64;
        let end = start + (partition.sectors as u64 * mbr.sector_size as u64);
        Ok((start, end))
    }

    /// Path to disk image.
    pub fn path(&self) -> &'a Path {
        self.path
    }

    /// Pretty print MBR with all partitions.
    pub fn pretty_print(&self) -> Result<String, MbrError> {
        let mbr = self.mbr()?;
        let mut out = String::new();
        out.push_str(&format!(
            "> Master Boot Record\n\
        > Sector size: {} bytes\n\
        > Disk size: {} sectors\n\
        > Aligned to: {} sectors\n\
        > ------------------\n\
        > N   start   size      type   boot",
            mbr.sector_size, mbr.disk_size, mbr.align,
        ));

        for (idx, partition) in mbr.iter().filter(|(_, partition)| partition.is_used()) {
            out.push_str(&format!(
                "\n> {}   {}s   {}s   0x{:x}   {}",
                idx,
                partition.starting_lba,
                partition.sectors,
                partition.sys,
                partition.is_active().to_string(),
            ));
        }
        Ok(out)
    }
}

// TODO: there are a lot of openings and closings of the image file in this implementation.
// Probably this can be optimized.

/// Create Master Boot Record (msdos table).
///
/// # Context variables required
/// - `image-file`
pub struct CreateMBR;

impl Step<LinuxVMBuildContext> for CreateMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        info!("creating Master Boot Record");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");

        let mbr = Mbr::new(image_file.path())?;

        for line in mbr.pretty_print()?.lines() {
            debug!("{}", line);
        }

        Ok(())
    }
}

/// Read Master Boot Record.
///
/// # Context variables required
/// - `image-file`
pub struct ReadMBR;

impl Step<LinuxVMBuildContext> for ReadMBR {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        info!("reading Master Boot Record");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");

        let mbr = Mbr::read_from(image_file.path())?;
        info!("found MBR on disk {}", &image_file);

        for line in mbr.pretty_print()?.lines() {
            debug!("{}", line);
        }

        Ok(())
    }
}

/// Create boot MBR partition (type `0x0c`).
///
/// This partition is used as boot partition in VM
/// and stores only kernel and bootloader with its config file.
///
/// The size of the partition is given in MBR sectors.
///
/// # Context variables required:
/// - `image-file`
///
/// # Context variables defined:
/// - `boot-partition-number`
pub struct CreateBootPartition {
    size: u32,
}

impl CreateBootPartition {
    pub fn new(size: u32) -> Self {
        Self { size }
    }
}

impl Step<LinuxVMBuildContext> for CreateBootPartition {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        info!("creating boot partition");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");

        let mbr_adapter = Mbr::read_from(image_file.path())?;

        let partition_size = Mbr::round_up(self.size);

        try_resize_to_fit(partition_size, image_file, mbr_adapter)
            .context("failed to create boot partition")?;

        let partition_idx = mbr_adapter
            .add_partition(partition_size, 0x0c, true)
            .context("failed to create boot partition")?;

        for line in mbr_adapter.pretty_print()?.lines() {
            debug!("{}", line);
        }

        info!(
            "boot partition #{} ({})",
            partition_idx,
            ByteSize::b(partition_size as u64 * mbr_adapter.mbr()?.sector_size as u64)
        );
        ctx.set("boot-partition-number", Box::new(partition_idx));

        Ok(())
    }
}

/// Read boot MBR partition.
///
/// # Context variables required:
/// - `image-file`
///
/// # Context variables defined:
/// - `boot-partition-number`
pub struct ReadBootPartition;

impl Step<LinuxVMBuildContext> for ReadBootPartition {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        info!("reading boot partition");
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");

        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;
        let mbr = mbr_adapter.mbr().context("failed to read MBR")?;

        let partition_idx = mbr
            .iter()
            .find_map(
                |(idx, part)| {
                    if part.is_active() {
                        Some(idx)
                    } else {
                        None
                    }
                },
            )
            .ok_or(MbrError::new(
                "failed to find boot partition".to_string(),
                MbrErrorKind::MissingBootPartition,
            ))?;
        let partition_size = mbr[partition_idx].sectors;

        info!(
            "found boot partition #{} ({})",
            partition_idx,
            ByteSize::b(partition_size as u64 * mbr.sector_size as u64)
        );
        ctx.set("boot-partition-number", Box::new(partition_idx));

        Ok(())
    }
}

/// Create root MBR partition (type `0x32`).
///
/// The size of the partition is given in bytes.
///
/// # Context variables required:
/// - `image-file`
/// - `root-partition-size`
///
/// # Context variables defined:
/// - `root-partition-number`
pub struct CreateRootPartition;

impl Step<LinuxVMBuildContext> for CreateRootPartition {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> anyhow::Result<()> {
        let image_file = ctx
            .get::<ImageFile>("image-file")
            .expect("image-file")
            .clone();
        let root_partition_size = ctx
            .get::<u64>("root-partition-size")
            .expect("root-partition-size");

        let mbr_adapter = Mbr::read_from(image_file.path())?;
        let mbr = mbr_adapter.mbr()?;

        // Number of sectors required to store root filesystem
        let size = u32::try_from(root_partition_size / mbr.sector_size as u64)
            .context("root filesystem is too big for MBR")?
            + 1;
        trace!("sectors required for root filesystem: {}s", size);

        let partition_size = Mbr::round_up(size);

        try_resize_to_fit(partition_size, &image_file, mbr_adapter)
            .context("failed to create root partition")?;

        let partition_idx = mbr_adapter
            .add_partition(partition_size, 0x83, false)
            .context("failed to create root partition")?;

        for line in mbr_adapter.pretty_print()?.lines() {
            debug!("{}", line);
        }

        ctx.set("root-partition-number", Box::new(partition_idx));

        Ok(())
    }
}

/// Try to resize disk image file and MBR disk to fit required size.
/// This may be called before creating a new partition to ensure
/// there is enough disk space for it.
///
/// # Arguments
/// - `required` - required free size on disk in sectors.
/// - `image_file` - disk image file.
/// - `mbr_adapter` - MBR adapter.
pub fn try_resize_to_fit(
    required: u32,
    image_file: &ImageFile,
    mbr_adapter: Mbr<'_>,
) -> anyhow::Result<()> {
    debug!("trying to fit {}s into {}", required, image_file);

    let mbr = mbr_adapter.mbr()?;

    let max_partition_size = mbr.get_maximum_partition_size().ok().unwrap_or_default();
    trace!("current max partition size: {}s", max_partition_size);

    if required > max_partition_size {
        // We need to extend the disk
        if !image_file.resizable() {
            bail!("not enough space on disk image");
        }

        debug!("resizing disk image to fit partition of size {}s", required);

        let sectors_to_add = required - max_partition_size;
        let bytes_to_add = sectors_to_add as u64 * mbr.sector_size as u64;

        trace!(
            "current image file size: {}",
            ByteSize::b(image_file.size()?)
        );
        image_file.extend(bytes_to_add)?;
        trace!(
            "resized image file size: {}",
            ByteSize::b(image_file.size()?)
        );

        mbr_adapter.resize_disk(mbr.disk_size + sectors_to_add)?;
        trace!(
            "max partition size after resize: {}s",
            mbr_adapter
                .mbr()?
                .get_maximum_partition_size()
                .ok()
                .unwrap_or_default()
        );
    }

    Ok(())
}

/// Try to resize disk image file, MBR disk and partition to fit required size.
/// This may be called before writing to a partition to ensure it has enough
/// free space.
///
/// If there is already enough space on partition, nothing will be done.
///
/// Partition **MUST BE** the last partition on the disk. Otherwise
/// no data will be changed on disk and an error will be returned.
///
/// # Arguments
/// - `partition_idx` - index of MBR partition to extent up to required size.
/// - `required` - required free size on disk in sectors.
/// - `image_file` - disk image file.
/// - `mbr_adapter` - MBR adapter.
pub fn try_resize_to_fit_into(
    partition_idx: usize,
    required: u32,
    image_file: &ImageFile,
    mbr_adapter: Mbr<'_>,
) -> anyhow::Result<()> {
    let mbr = mbr_adapter.mbr()?;
    let partition = &mbr[partition_idx];
    let partition_size = partition.sectors;

    if partition_size < required {
        // We need to extend partiton

        // Check that partition is the last one
        let is_last = mbr
            .iter()
            .filter(|(idx, part)| part.is_used() && *idx != partition_idx)
            .fold(true, |acc, (_, part)| {
                acc && (part.starting_lba + part.sectors <= partition.starting_lba)
            });
        if !is_last {
            return Err(MbrError::new(
                "attempt to resize partition which is not the last one on the disk".to_string(),
                MbrErrorKind::PartitionExtendError,
            )
            .into());
        }

        // Calculate free space on disk available for this partition extension
        let free_space = mbr
            .find_free_sectors()
            .into_iter()
            .filter_map(|(start, size)| {
                if start >= partition.starting_lba {
                    Some(size)
                } else {
                    None
                }
            })
            .fold(0u32, |acc, size| acc + size);
        trace!(
            "free space left on drive for this partition extension: {}s",
            free_space
        );

        trace!(
            "current partition #{} size: {}s",
            partition_idx,
            partition_size
        );

        if partition_size + free_space < required {
            // We need to extend disk size

            let sectors_to_add = required - partition_size - free_space;
            let bytes_to_add = sectors_to_add as u64 * mbr.sector_size as u64;
            trace!("attempt to add {}s to disk", sectors_to_add);

            // We need to extend the disk image file
            if !image_file.resizable() {
                bail!("not enough space on disk image");
            }

            debug!("resizing disk image to fit partition of size {}s", required);

            trace!(
                "current image file size: {}",
                ByteSize::b(image_file.size()?)
            );
            image_file.extend(bytes_to_add)?;
            trace!(
                "resized image file size: {}",
                ByteSize::b(image_file.size()?)
            );

            mbr_adapter.resize_disk(mbr.disk_size + sectors_to_add)?;

            // Re-read MBR after changing
            let mbr = mbr_adapter.mbr()?;
            trace!("disk size after resize: {}s", mbr.disk_size);
        }

        // Re-read MBR after possible changes
        let mut mbr = mbr_adapter.mbr()?;
        // Update partition size
        mbr[partition_idx].sectors = required;
        mbr_adapter.write(mbr)?;

        // Re-read MBR after changing
        let mbr = mbr_adapter.mbr()?;
        trace!(
            "partition #{} size after resize: {}s",
            partition_idx,
            mbr[partition_idx].sectors
        );
    }

    Ok(())
}
