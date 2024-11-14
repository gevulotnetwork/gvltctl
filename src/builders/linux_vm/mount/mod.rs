#[cfg(feature = "fuse")]
mod fuse;
#[cfg(feature = "fuse")]
pub use fuse::*;

#[cfg(not(feature = "fuse"))]
mod kernel;
#[cfg(not(feature = "fuse"))]
pub use kernel::*;
