use anyhow::Result;
use std::path::Path;

/// Type implementing gerenal interface for filesystem handler.
pub trait FileSystemHandler: Sized {
    /// Block size of the filesystem.
    const BLOCK_SIZE: u64;

    /// Filesystem partition offset in bytes.
    fn offset(&self) -> u64;

    /// Path to image file.
    fn path(&self) -> &Path;

    /// Check filesystem.
    fn check(&self) -> Result<()>;

    /// Resize filesystem. `new_size` is given in blocks.
    fn resize(&self, new_size: u64) -> Result<()>;
}

#[cfg_attr(target_os = "linux", path = "ext4_linux.rs")]
#[cfg_attr(target_os = "macos", path = "ext4_macos.rs")]
mod ext4;

pub use ext4::*;
