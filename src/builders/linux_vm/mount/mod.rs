use anyhow::Result;
use std::path::Path;

use crate::builders::linux_vm::filesystem::FileSystemHandler;

/// Type implementing generic interfaces for mount handling.
pub trait MountHandler: Sized {
    /// Path to mounted directory.
    fn path(&self) -> &Path;

    /// Create new mount for the filesystem.
    ///
    /// - `fs` - filesystem to mount
    /// - `source` - path to disk image to use
    fn new<F, P>(fs: &F, source: P) -> Result<Self>
    where
        F: FileSystemHandler,
        P: AsRef<Path>;

    /// Remove mount leaving `self` invalid.
    fn unmount_no_drop(&self) -> Result<()>;

    /// Remove mount destroying `self`.
    fn unmount(self) -> Result<()> {
        self.unmount_no_drop()
    }
}

pub mod fuse;
pub mod native;
