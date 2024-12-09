use std::path::Path;
use anyhow::Result;

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

#[cfg(feature = "fat32")]
mod fat32;

#[cfg_attr(target_os = "linux", path = "ext4_linux.rs")]
#[cfg_attr(target_os = "macos", path = "ext4_macos.rs")]
#[cfg(feature = "ext4")]
mod ext4;

#[cfg(feature = "fat32")]
pub use fat32::*;

#[cfg(feature = "ext4")]
pub use ext4::*;
