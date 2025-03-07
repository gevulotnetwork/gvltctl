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

/// Image file adapter.
#[derive(Clone, Debug)]
pub struct ImageFile {
    /// Path to the file.
    path: PathBuf,

    /// Whether the image can be resized or not.
    resizable: bool,
}

impl ImageFile {
    /// Size of the image created initially, when size is not specified by user.
    ///
    /// This will be enough to create MBR.
    pub const MIN_IMAGE_SIZE: u64 = ByteSize::mib(1).as_u64();

    /// Create new image file with given size.
    pub fn create<P>(path: P, size: Option<u64>, overwrite: bool) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        if path.as_ref().exists() {
            if overwrite {
                fs::remove_file(&path).context("failed to remove image file")?;
            } else {
                bail!("output file '{}' already exists", path.as_ref().display());
            }
        }
        let mut file = File::create_new(&path).context("failed to create image file")?;

        let resizable = size.is_none();
        let size = size.unwrap_or(Self::MIN_IMAGE_SIZE);

        // This will create sparse file on Linux
        // TODO: research this on other platforms
        file.seek(SeekFrom::Start(size - 1))
            .context("seek for image size")?;
        file.write_all(&[0])
            .context("failed to extend image file")?;

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            resizable,
        })
    }

    /// Use existing image file.
    pub fn from_existing<P1, P2>(
        source: P1,
        path: P2,
        size: Option<u64>,
        overwrite: bool,
    ) -> Result<Self>
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        if path.as_ref().exists() && !overwrite {
            bail!("output file '{}' already exists", path.as_ref().display());
        }
        let base_image_size =
            fs::copy(source.as_ref(), path.as_ref()).context("failed to copy base image file")?;
        let resizable = size.is_none();
        let image = Self {
            path: path.as_ref().to_path_buf(),
            resizable,
        };
        if let Some(size) = size {
            if base_image_size > size {
                // We don't want to emit error on this action
                let _ = fs::remove_file(path.as_ref());
                bail!("not enough space on disk image");
            }
            image.extend(size - base_image_size)?;
        }
        Ok(image)
    }

    /// Extend file by `value` returning old size.
    pub fn extend(&self, value: u64) -> Result<u64> {
        let current_size = self.size()?;

        if value == 0 {
            return Ok(current_size);
        }

        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.path)
            .context("failed to open image file")?;

        let current_pos = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(current_pos + value - 1))
            .context("failed to seek for image size")?;
        file.write_all(&[0])
            .context("failed to extend image file")?;

        Ok(current_size)
    }

    /// Delete image file.
    #[allow(unused)]
    pub fn delete(self) -> Result<()> {
        fs::remove_file(self.path).map_err(Into::into)
    }

    /// Path to file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Whether the image can be resized or not.
    pub fn resizable(&self) -> bool {
        self.resizable
    }

    /// Current size of the file.
    pub fn size(&self) -> Result<u64> {
        let meta = fs::metadata(&self.path).context("failed to get image file metadata")?;
        Ok(meta.len())
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
        info!(
            "creating image file: {}",
            ctx.opts().image_file_opts.path.display()
        );
        let image_file = ImageFile::create(
            &ctx.opts().image_file_opts.path,
            ctx.opts().image_file_opts.size,
            ctx.opts().image_file_opts.force,
        )?;
        debug!(
            "image file created: {} ({})",
            &image_file,
            ByteSize::b(image_file.size()?),
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
        let crc_instance = crc::Crc::<u64>::new(&crc::CRC_64_ECMA_182);
        let checksum = format!("{:x}", crc_instance.checksum(BASE_IMAGE));
        debug!("base image checksum: {}", &checksum);
        let base_image_path = ctx.cache().join(format!("{}.base.img", checksum));
        if !base_image_path.is_file() {
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
            ctx.opts().image_file_opts.size,
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

#[cfg(test)]
mod tests {
    use super::ImageFile;
    use anyhow::Result;
    use tempdir::TempDir;

    #[test]
    fn test_image_file_create_fixed() -> Result<()> {
        let tmp = TempDir::new("image-file-tests")?;
        let path = tmp.path().join("test");
        let size = 3;

        let image = ImageFile::create(path.clone(), Some(size), false)?;
        assert!(path.is_file());
        assert_eq!(image.size()?, size);
        assert!(!image.resizable());

        Ok(())
    }

    #[test]
    fn test_image_file_create_default() -> Result<()> {
        let tmp = TempDir::new("image-file-tests")?;
        let path = tmp.path().join("test");

        let image = ImageFile::create(path.clone(), None, false)?;
        assert!(path.is_file());
        assert_eq!(image.size()?, ImageFile::MIN_IMAGE_SIZE);
        assert!(image.resizable());

        Ok(())
    }

    #[test]
    fn test_image_file_delete() -> Result<()> {
        let tmp = TempDir::new("image-file-tests")?;
        let path = tmp.path().join("test");

        let image = ImageFile::create(path.clone(), None, false)?;
        assert!(path.is_file());
        image.delete()?;
        assert!(!path.exists());

        Ok(())
    }

    #[test]
    fn test_image_file_resize() -> Result<()> {
        let tmp = TempDir::new("image-file-tests")?;
        let path = tmp.path().join("test");
        let extend = 5;

        let image = ImageFile::create(path.clone(), None, false)?;
        image.extend(extend)?;
        assert_eq!(image.size()?, ImageFile::MIN_IMAGE_SIZE + extend);

        Ok(())
    }
}

// TODO: clarify what's going on with `resizable`
