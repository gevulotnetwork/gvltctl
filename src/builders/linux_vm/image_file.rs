use anyhow::{bail, Context, Result};
use bytesize::ByteSize;
use log::{debug, info};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::LinuxVMBuildContext;

/// Image file.
#[derive(Clone, Debug)]
pub struct ImageFile {
    /// Path to the file.
    path: PathBuf,

    /// Current size of the file.
    size: ByteSize,
}

impl ImageFile {
    /// Create new image file with given size.
    pub fn create<P>(path: P, size: ByteSize, overwrite: bool) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        if path.as_ref().exists() && !overwrite {
            bail!("Output file '{}' already exists.", path.as_ref().display());
        }
        let mut file = File::create(&path).context("failed to create image file")?;

        // This will create sparse file on Linux
        // TODO: research this on other platforms
        let size = file
            .seek(SeekFrom::Start(size.as_u64() - 1))
            .context("seek for image size")?;
        file.write_all(&[0])
            .context("failed to extend image file")?;

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            size: ByteSize::b(size),
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
        let size = fs::copy(source.as_ref(), path.as_ref()).context("failed to copy image file")?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            size: ByteSize::b(size),
        })
    }

    /// Extend file returning old size.
    pub fn extend(&mut self, value: ByteSize) -> Result<ByteSize> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .context("failed to open image file")?;

        let current_size = self.size;
        self.size = current_size + value;

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
        std::fs::remove_file(self.path).map_err(Into::into)
    }

    /// Path to file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Current size of the file.
    pub fn size(&self) -> ByteSize {
        self.size
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
pub struct CreateImageFile;

impl Step<LinuxVMBuildContext> for CreateImageFile {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating image file");
        let image_file = ImageFile::create(
            &ctx.opts().image_path,
            ctx.opts().image_size,
            ctx.opts().force,
        )?;
        debug!(
            "image file created: {} ({})",
            &image_file,
            image_file.size()
        );
        ctx.0.set("image_file", Box::new(image_file));
        Ok(())
    }
}

/// Use existing disk image file.
pub struct UseImageFile {
    base_image: PathBuf,
}

impl UseImageFile {
    pub fn new<P>(base_image: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            base_image: base_image.as_ref().to_path_buf(),
        }
    }
}

impl Step<LinuxVMBuildContext> for UseImageFile {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using base image file");
        debug!("base image file: {}", self.base_image.display());
        let image_file =
            ImageFile::from_existing(&self.base_image, &ctx.opts().image_path, ctx.opts().force)?;
        debug!("image file copied: {} ({})", &image_file, image_file.size());
        ctx.0.set("image_file", Box::new(image_file));
        Ok(())
    }
}
