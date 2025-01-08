use anyhow::{bail, Context, Result};
use bytesize::ByteSize;
use log::{debug, info};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::{LinuxVMBuildContext, BASE_IMAGE};

/// Image file.
#[derive(Clone, Debug)]
pub struct ImageFile {
    /// Path to the file.
    path: PathBuf,
}

impl ImageFile {
    /// Create new image file with given size.
    pub fn create<P>(path: P, size: u64, overwrite: bool) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        if path.as_ref().exists() {
            if overwrite {
                fs::remove_file(&path).context("failed to remove image file")?;
            } else {
                bail!("Output file '{}' already exists.", path.as_ref().display());
            }
        }
        let mut file = File::create_new(&path).context("failed to create image file")?;

        // This will create sparse file on Linux
        // TODO: research this on other platforms
        file.seek(SeekFrom::Start(size - 1))
            .context("seek for image size")?;
        file.write_all(&[0])
            .context("failed to extend image file")?;

        Ok(Self {
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Use existing image file.
    pub fn from_existing<P1, P2>(source: P1, path: P2, overwrite: bool) -> Result<Self>
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        if path.as_ref().exists() && !overwrite {
            bail!("Output file '{}' already exists.", path.as_ref().display());
        }
        fs::copy(source.as_ref(), path.as_ref()).context("failed to copy image file")?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Extend file returning old size.
    pub fn extend(&mut self, value: ByteSize) -> Result<ByteSize> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .context("failed to open image file")?;

        let current_size = self.size()?;

        ByteSize::b(
            file.seek(SeekFrom::Current(value.0 as i64 - 1))
                .context("failed to seek for image size")?,
        );
        file.write_all(&[0])
            .context("failed to extend image file")?;

        Ok(current_size)
    }

    /// Delete image file.
    pub fn delete(self) -> Result<()> {
        fs::remove_file(self.path).map_err(Into::into)
    }

    /// Path to file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Current size of the file.
    pub fn size(&self) -> Result<ByteSize> {
        let meta = fs::metadata(&self.path).context("failed to get image file metadata")?;
        Ok(ByteSize::b(meta.len()))
    }
}

impl fmt::Display for ImageFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path.display()))
    }
}

impl AsRef<OsStr> for ImageFile {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

/// Create new disk image file.
///
/// # Context variables defined
/// - `image-file`
pub struct CreateImageFile;

impl Step<LinuxVMBuildContext> for CreateImageFile {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating image file");
        let image_file = ImageFile::create(
            &ctx.opts().image_file_opts.path,
            ctx.opts().image_file_opts.size,
            ctx.opts().image_file_opts.force,
        )?;
        debug!(
            "image file created: {} ({})",
            &image_file,
            image_file.size()?
        );
        ctx.set("image-file", Box::new(image_file));
        Ok(())
    }
}

/// Use existing disk image file.
///
/// # Context variables defined
/// - `image-file`
pub struct UseImageFile;

impl Step<LinuxVMBuildContext> for UseImageFile {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let base_image_path = ctx.cache().join("base.img");
        if !base_image_path.exists() {
            info!("creating base image file: {}", base_image_path.display());
            let mut file = fs::File::create_new(&base_image_path)
                .context("failed to create base image file")?;
            file.write_all(BASE_IMAGE)
                .context("failed to write base image file")?;
        }

        info!("using base image file: {}", base_image_path.display());
        let image_file = ImageFile::from_existing(
            &base_image_path,
            &ctx.opts().image_file_opts.path,
            ctx.opts().image_file_opts.force,
        )?;
        debug!(
            "image file copied: {} ({})",
            &image_file,
            image_file.size()?
        );
        ctx.set("image-file", Box::new(image_file));
        Ok(())
    }
}
