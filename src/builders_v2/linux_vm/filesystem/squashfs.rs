use anyhow::{bail, Context, Result};
use backhand::compression::Compressor;
use backhand::{FilesystemCompressor, FilesystemReader, FilesystemWriter, NodeHeader};
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::fs;
use std::io::{self, BufReader, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::Mbr;
use crate::builders::Step;

use super::LinuxVMBuildContext;

/// SquashFS adapter.
#[derive(Debug)]
pub struct SquashFs<'a> {
    fs_image: &'a Path,
}

impl<'a> SquashFs<'a> {
    /// Get new SquashFS adapter for existing SquashFS image.
    pub fn get(fs_image: &'a Path) -> Self {
        Self { fs_image }
    }

    /// Create new SquashFS image,
    pub fn format(fs_image: &'a Path) -> Result<()> {
        let file =
            fs::File::create_new(fs_image).context("failed to create SquashFS image file")?;
        let mut fs_writer = FilesystemWriter::default();
        let compressor = FilesystemCompressor::new(Compressor::None, None)
            .context("failed to create compressor for SquashFS")?;
        fs_writer.set_compressor(compressor);
        fs_writer
            .write(file)
            .context("failed to write filesystem")?;
        Ok(())
    }

    /// Compress SquashFS with default compressor (`xz`), returning the size of compressed image.
    pub fn compress(&self) -> Result<u64> {
        let file = BufReader::new(
            fs::File::open(self.fs_image).context("failed to open SquashFS image file")?,
        );
        let fs_reader =
            FilesystemReader::from_reader(file).context("failed to read SquashFS image")?;
        let mut fs_writer = FilesystemWriter::from_fs_reader(&fs_reader)
            .context("failed to create SquashFS writer")?;

        fs_writer.set_compressor(FilesystemCompressor::default());

        // Write modified SquashFS to temp file
        let tmp_dir = tempdir::TempDir::new("").context("failed to create temp directory")?;
        let tmp_path = tmp_dir.path().join("tmp");
        let mut tmp = fs::File::create_new(&tmp_path).context("failed to create temp file")?;
        fs_writer
            .write(&mut tmp)
            .context("failed to write to temp file")?;
        drop(tmp);

        // Implicitly close image file to re-open it later
        drop(fs_writer);
        drop(fs_reader);

        // Replace image content with content from temp file
        let size = fs::copy(&tmp_path, self.fs_image).context("failed to write SquashFS image")?;
        Ok(size)
    }

    /// Add directory and all of its content recursively to the root of filesystem.
    pub fn push_dir_recursively(&self, source: &Path) -> Result<()> {
        debug_assert!(source.is_dir());
        Self::push_dir_recursively_inner(self.fs_image, source, source)
    }

    fn push_dir_recursively_inner(fs_image: &Path, base: &Path, source: &Path) -> Result<()> {
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

            // Note: file juggling below is required to avoid opening too many files at the same time.
            // FilesystemWriter stores opened file descriptor. Using a single instance of that to push
            // whole directory may result into "Too many open files (os error 24)".
            // Because of that we re-create SquashFS image for each file we write.
            // Unfortunately this does affect the performance. Hopefully there is a better solution.

            // Get current SquashFS
            let file = BufReader::new(
                fs::File::open(fs_image).context("failed to open SquashFS image file")?,
            );
            let fs_reader =
                FilesystemReader::from_reader(file).context("failed to read SquashFS image")?;
            let mut fs_writer = FilesystemWriter::from_fs_reader(&fs_reader)
                .context("failed to create SquashFS writer")?;

            // Modify it
            if file_type.is_dir() {
                trace!("creating directory squashfs:/{}", relative_path.display());
                fs_writer.push_dir(relative_path, header).context(format!(
                    "failed to create directory in SquashFS: {}",
                    relative_path.display()
                ))?;
            } else if file_type.is_file() {
                let reader = fs::File::open(&entry_path).context("failed to open source file")?;
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

            // Write modified SquashFS to temp file
            let tmp_dir = tempdir::TempDir::new("").context("failed to create temp directory")?;
            let tmp_path = tmp_dir.path().join("tmp");
            let mut tmp = fs::File::create_new(&tmp_path).context("failed to create temp file")?;
            fs_writer
                .write(&mut tmp)
                .context("failed to write to temp file")?;
            drop(tmp);

            // Implicitly close image file to re-open it later
            drop(fs_writer);
            drop(fs_reader);

            // Replace image content with content from temp file
            fs::copy(&tmp_path, fs_image).context("failed to write SquashFS image")?;

            // Remove temp file
            drop(tmp_dir);

            if file_type.is_dir() {
                Self::push_dir_recursively_inner(fs_image, base, &entry.path())?;
            }
        }
        // TODO: handle duplications
        Ok(())
    }
}

/// Format new SquashFS image.
///
/// # Context variables defined:
/// - `squashfs`: [`PathBuf`]
pub struct Format;

impl Step<LinuxVMBuildContext> for Format {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("formatting SquashFS");
        let squashfs_image_path = ctx.tmp().join("root.squashfs");
        SquashFs::format(&squashfs_image_path).context("failed to format SquashFS")?;
        ctx.set("squashfs", Box::new(squashfs_image_path));
        Ok(())
    }
}

/// Compress and evaluate the size of the partition required to store this filesystem.
///
/// # Context variables required:
/// - `squashfs`
///
/// # Context variables defined:
/// - `root-partition-size`: [`u64`]
pub struct EvaluateSize;

impl Step<LinuxVMBuildContext> for EvaluateSize {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        debug!("calculating disk space for SquashFS");
        let squashfs = SquashFs::get(ctx.get::<PathBuf>("squashfs").expect("squashfs"));
        let size = squashfs.compress().context("failed to compress SquashFS")?;

        debug!("SquashFS size: {}", ByteSize::b(size).to_string_as(true));

        ctx.set("root-partition-size", Box::new(size));
        Ok(())
    }
}

/// Write SquashFS image into root partition.
///
/// # Context variables required:
/// - `image-file`
/// - `squashfs`
/// - `root-partition-number`
pub struct WriteSquashFs;

impl Step<LinuxVMBuildContext> for WriteSquashFs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let squashfs_image = ctx.get::<PathBuf>("squashfs").expect("squashfs");
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
