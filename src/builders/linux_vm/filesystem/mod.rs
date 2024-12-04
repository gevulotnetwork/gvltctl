use std::path::Path;

/// Type implementing gerenal interface for filesystem handler.
pub trait FileSystemHandler: Sized {
    /// Filesystem partition offset in bytes.
    fn offset(&self) -> u64;

    /// Path to image file.
    fn path(&self) -> &Path;
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
