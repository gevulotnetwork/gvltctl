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
