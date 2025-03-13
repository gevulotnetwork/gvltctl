use anyhow::{Context, Result};
use log::{info, trace};
use std::path::{Path, PathBuf};

use crate::builders::Step;

use super::LinuxVMBuildContext;

/// Path to gevulot mount directories (relative to root `/`).
const DIRS: &[&str] = &[
    "mnt/gevulot/input",
    "mnt/gevulot/output",
    "mnt/gevulot/rt-config",
];

/// Create dirs from [`DIRS`].
fn create_dirs(base_path: &Path) -> Result<()> {
    for path in DIRS {
        let path_to_create = base_path.join(path);
        trace!("creating {}", path_to_create.display());
        std::fs::create_dir_all(&path_to_create)?;
    }
    Ok(())
}

/// Create gevulot runtime directories: `/mnt/gevulot/{input,output,rt-config}`.
///
/// # Context variables required
/// - `root-fs`
pub struct CreateGevulotRuntimeDirs;

impl Step<LinuxVMBuildContext> for CreateGevulotRuntimeDirs {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating gevulot runtime directories");
        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");
        create_dirs(&rootfs).context("failed to create gevulot runtime directories")?;
        Ok(())
    }
}
