use anyhow::{anyhow, Context, Result};
use fs_extra::dir;
use log::{debug, info, trace};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fmt, fs, path};
use unix_path::Path as UnixPath;
use walkdir::WalkDir;

use crate::builders::Step;

use super::filesystem::FileSystem;
use super::LinuxVMBuildContext;

#[derive(Clone, Debug)]
pub struct RootFS {
    path: PathBuf,
    size: u64,
}

impl RootFS {
    pub fn from_path(path: PathBuf) -> Result<Self> {
        debug_assert!(path.is_dir());
        let size = dir::get_size(&path).context("get root filesystem size")?;
        Ok(Self { size, path })
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn install(&self, fs: &FileSystem) -> Result<()> {
        let abs_path = path::absolute(&self.path).context("make absolute path")?;
        let imgname = fs.path().to_string_lossy();
        trace!("walking {}", abs_path.display());
        let walk = WalkDir::new(&abs_path);

        // Since abs_path is a directory, walkdir will yield this path first
        // That's why we skip it here
        for entry in walk.into_iter().skip(1) {
            let dir_entry = entry.context("walk root filesystem directory")?;
            debug_assert!(dir_entry.path().is_absolute());

            let entry_path = dir_entry
                .path()
                .strip_prefix(&abs_path)
                .context("strip absolute prefix")?;
            debug_assert!(entry_path.is_relative());

            trace!(
                "creating path `{}` in target filesystem",
                entry_path.display()
            );

            let file_type = dir_entry.file_type();
            if file_type.is_dir() {
                trace!("create directory {}:{}", imgname, entry_path.display());
                fs.create_dir(UnixPath::new(
                    entry_path.to_str().ok_or(anyhow!("non-UTF-8 path"))?,
                ))?;
            } else if file_type.is_file() {
                trace!("read file {}", dir_entry.path().display());
                let mut file =
                    fs::File::open(dir_entry.path()).context("open file in root filesystem")?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;

                trace!("write file {}:{}", imgname, entry_path.display());
                fs.write_file(
                    UnixPath::new(entry_path.to_str().ok_or(anyhow!("non-UTF-8 path"))?),
                    &buf,
                )
                .context("write file to root filesystem")?;
            } else if file_type.is_symlink() {
                todo!("symlinks");
            }
        }
        Ok(())
    }
}

impl fmt::Display for RootFS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}", self.path().display()))
    }
}

impl AsRef<Path> for RootFS {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

/// Use ready root filesystem from given path.
pub struct RootFSFromDir;

impl Step<LinuxVMBuildContext> for RootFSFromDir {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("setting root filesystem");
        let rootfs = RootFS::from_path(
            ctx.opts()
                .rootfs_dir
                .as_ref()
                .ok_or(anyhow!("cannot use root filesystem: path not found"))?
                .clone(),
        )
        .context("set root filesystem path")?;
        debug!("root filesystem set: {} ({} bytes)", &rootfs, rootfs.size());
        ctx.0.set("rootfs", Box::new(rootfs));
        Ok(())
    }
}

/// Install root filesystem to disk partition.
pub struct InstallRootFS;

impl Step<LinuxVMBuildContext> for InstallRootFS {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing root filesystem");

        let rootfs = ctx.0.get::<RootFS>("rootfs").ok_or(anyhow!(
            "cannot install root filesystem: root filesystem not found"
        ))?;

        let fs = ctx.0.get::<FileSystem>("fs").ok_or(anyhow!(
            "cannot install root filesystem: filesystem not found"
        ))?;

        rootfs.install(&fs)?;

        Ok(())
    }
}
