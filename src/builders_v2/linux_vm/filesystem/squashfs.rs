use anyhow::{bail, Context, Result};
use backhand::{FilesystemWriter, NodeHeader};
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::fs;
use std::io::{self, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::Step;

use super::LinuxVMBuildContext;

/// SquashFS handler.
#[derive(Debug)]
pub struct SquashFs<'a, 'b, 'c> {
    fs_writer: FilesystemWriter<'a, 'b, 'c>,
}

impl<'a, 'b, 'c> SquashFs<'a, 'b, 'c> {
    /// Create new SquashFS handler.
    pub fn new() -> Self {
        let mut fs_writer = FilesystemWriter::default();
        fs_writer.set_current_time();
        Self { fs_writer }
    }

    /// Reference to filesystem writer.
    #[allow(unused)]
    pub fn fs_writer(&self) -> &FilesystemWriter<'a, 'b, 'c> {
        &self.fs_writer
    }

    /// Mutable reference to filesystem writer.
    pub fn fs_writer_mut(&mut self) -> &mut FilesystemWriter<'a, 'b, 'c> {
        &mut self.fs_writer
    }

    /// Add directory and all of its content recursively to the root of filesystem.
    pub fn push_dir_recursively(&mut self, source: &Path) -> Result<()> {
        debug_assert!(source.is_dir());
        Self::push_dir_recursively_inner(&mut self.fs_writer, source, source)
    }

    fn push_dir_recursively_inner(
        fs_writer: &mut FilesystemWriter,
        base: &Path,
        source: &Path,
    ) -> Result<()> {
        for entry_result in source
            .read_dir()
            .context("failed to read RootFS directory")?
        {
            let entry = entry_result.context("failed to read RootFS directory")?;
            trace!("handling entry {:?}", &entry);
            let metadata = entry
                .metadata()
                .context("failed to obtain entry metadata")?;
            let mode: u16 = metadata
                .permissions()
                .mode()
                .try_into()
                .context("failed to resolve mode of the entry")?;
            let header = NodeHeader {
                permissions: mode,
                ..NodeHeader::default()
            };
            let file_type = metadata.file_type();
            let entry_path = entry.path();
            let relative_path = entry_path
                .strip_prefix(base)
                .context("failed to strip prefix from entry path")?;
            if file_type.is_dir() {
                trace!("creating directory squashfs:/{}", relative_path.display());
                fs_writer.push_dir(relative_path, header).context(format!(
                    "failed to create directory in SquashFS: {}",
                    relative_path.display()
                ))?;
                Self::push_dir_recursively_inner(fs_writer, base, &entry.path())?;
            } else if file_type.is_file() {
                let reader = fs::File::open(&entry_path)?;
                trace!("creating file squashfs:/{}", relative_path.display());
                fs_writer
                    .push_file(reader, relative_path, header)
                    .context("failed to create file in SquashFS")?;
            } else if file_type.is_symlink() {
                let target = fs::read_link(&entry_path).context("failed to read link entry")?;
                trace!(
                    "creating symlink squashfs:/{} -> {}",
                    relative_path.display(),
                    target.display()
                );
                fs_writer
                    .push_symlink(&target, relative_path, header)
                    .context("failed to create symlink in SquashFS")?;
            } else {
                bail!("unknown file type for entry '{}'", entry.path().display());
            }
        }
        // TODO: handle duplications
        Ok(())
    }
}

/// Initialize SquashFS handler.
///
/// # Context variables defined:
/// - `squashfs`: [`SquashFs`]
pub struct Init;

impl Step<LinuxVMBuildContext> for Init {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("initializing SquashFS handler");
        let squashfs = SquashFs::new();
        ctx.set("squashfs", Box::new(squashfs));
        Ok(())
    }
}

/// Evaluate the size of the partition required to store this filesystem.
/// Also writes SquashFS image into temp file.
///
/// # Context variables required:
/// - `squashfs` (will be removed on this step to prevent multiple writes)
///
/// # Context variables defined:
/// - `root-partition-size`: [`u64`]
/// - `squashfs-image`: [`PathBuf`]
pub struct EvaluateSize;

impl Step<LinuxVMBuildContext> for EvaluateSize {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("calculating disk space for SquashFS");
        let mut squashfs = ctx.pop::<SquashFs>("squashfs").expect("squashfs");

        let squashfs_image_path = ctx.tmp().join("root.squashfs");
        debug!(
            "writing SquashFS to temp file {}",
            squashfs_image_path.display(),
        );
        let mut tmp = fs::File::create_new(&squashfs_image_path)
            .context("failed to create temp file for SquashFS")?;
        let (_, size) = squashfs
            .fs_writer_mut()
            .write(&mut tmp)
            .context("failed to write SquashFS to temp file")?;
        debug!("SquashFS size: {}", ByteSize::b(size).to_string_as(true));

        ctx.set("root-partition-size", Box::new(size));
        ctx.set("squashfs-image", Box::new(squashfs_image_path));
        Ok(())
    }
}

/// Write SquashFS image into root partition.
///
/// # Context variables required:
/// - `image-file`
/// - `squashfs-image`
/// - `root-partition-number`
pub struct WriteSquashFs;

impl Step<LinuxVMBuildContext> for WriteSquashFs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let squashfs_image = ctx
            .get::<PathBuf>("squashfs-image")
            .expect("squashfs-image");
        let root_partition_number = *ctx
            .get::<usize>("root-partition-number")
            .expect("root-partition-number");

        info!("writing SquashFS to partition #{}", root_partition_number);

        let mbr_adapter = Mbr::read_from(image_file.path())?;

        let (start, _) = mbr_adapter.partition_limits(root_partition_number)?;
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .open(image_file.path())
            .context("failed to open disk image file")?;
        writer
            .seek(SeekFrom::Start(start))
            .context("failed to seek disk image file")?;

        let mut reader =
            fs::File::open(&squashfs_image).context("failed to open SquashFS image file")?;

        let written =
            io::copy(&mut reader, &mut writer).context("failed to write SquashFS to disk image")?;

        info!(
            "SquashFS written to partition #{} ({})",
            root_partition_number,
            ByteSize::b(written),
        );
        ctx.set("root-partition-number", Box::new(root_partition_number));

        Ok(())
    }
}
