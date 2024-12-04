//! FUSE-based mounting utils.
//!
//! Implementation differs on Linux-based and MacOS.

pub use super::MountHandler;

#[cfg(target_os = "linux")]
mod linux_fuse;
#[cfg(target_os = "linux")]
pub use linux_fuse::*;

#[cfg(target_os = "macos")]
mod macos_fuse;
#[cfg(target_os = "macos")]
pub use macos_fuse::*;
