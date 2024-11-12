use anyhow::{bail, Context, Result};
use log::{debug, info};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{File, OpenOptions};
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
    size: u64,
}

impl ImageFile {
    /// Create new image file with given size.
    pub fn create<P>(path: P, size: u64, overwrite: bool) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        if path.as_ref().exists() && !overwrite {
            bail!("Output file '{}' already exists.", path.as_ref().display());
        }
        let mut file = File::create(&path).context("create image file")?;

        // This will create sparse file on Linux
        // TODO: research this on other platforms
        let size = file
            .seek(SeekFrom::Start(size - 1))
            .context("seek for image size")?;
        file.write_all(&[0]).context("extend image file")?;

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            size,
        })
    }

    /// Extend file returning old size.
    pub fn extend<P>(&mut self, value: u32) -> Result<u64> {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&self.path)
            .context("open image file")?;

        let current_size = file
            .seek(SeekFrom::Current(0))
            .context("seek for current image size")?;

        self.size = file
            .seek(SeekFrom::Current(value as i64 - 1))
            .context("seek for image size")?;
        file.write_all(&[0]).context("extend image file")?;

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
    pub fn size(&self) -> u64 {
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
            ctx.opts().image_size.into(),
            ctx.opts().force,
        )?;
        debug!("image file created: {}", &image_file);
        ctx.0.set("image_file", Box::new(image_file));
        Ok(())
    }
}
