use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fmt, fs};
use unix_path::{Path as UnixPath, PathBuf as UnixPathBuf};

use crate::builders::Step;

use super::filesystem::FileSystem;
use super::utils::run_command;
use super::LinuxVMBuildContext;

/// Linux kernel.
#[derive(Debug)]
pub enum Kernel {
    /// Precompiled kernel.
    Precompiled {
        /// Path to binary file.
        path: PathBuf,

        /// Size of the kernel file.
        size: u64,
    },

    /// Kernel compiled from sources.
    Sources {
        /// URL used to fetch source code.
        git_url: String,

        /// Git version to checkout (e.g. `v6.10.11`).
        version: String,

        /// Path to sources.
        source_path: PathBuf,

        /// Path to compiled binary.
        path: PathBuf,

        /// Size of the kernel file.
        size: u64,
    },
}

impl fmt::Display for Kernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = match self {
            Kernel::Precompiled { path, .. } => path,
            Kernel::Sources { path, .. } => path,
        };
        f.write_str(&format!("{}", path.display()))
    }
}

impl Kernel {
    /// Path to the kernel binary.
    pub fn path(&self) -> &Path {
        match self {
            Kernel::Precompiled { path, .. } => path.as_path(),
            Kernel::Sources { path, .. } => path.as_path(),
        }
    }

    /// Size of kernel binary.
    pub fn size(&self) -> u64 {
        match self {
            Kernel::Precompiled { size, .. } => *size,
            Kernel::Sources { size, .. } => *size,
        }
    }

    /// Whether kernel was precompiled or not.
    pub fn is_precompiled(&self) -> bool {
        matches!(self, Self::Precompiled { .. })
    }

    /// Build kernel from sources.
    pub fn build(git_url: &str, version: &str) -> Result<Self> {
        // TODO: check required tools are available: git, make, gcc
        let home_dir = std::env::var("HOME").context("Failed to get HOME environment variable")?;
        let kernel_dir = format!("{}/.linux-builds/{}", home_dir, version);
        let bzimage_path = format!("{}/arch/x86/boot/bzImage", kernel_dir);

        // Check if the bzImage already exists
        if Path::new(&bzimage_path).exists() {
            debug!("Kernel bzImage already exists, skipping build");
        } else {
            // Clone the specific version from the remote repository

            // Check if the kernel directory already exists
            if Path::new(&kernel_dir).exists() {
                // If it exists, do a git pull
                debug!("Kernel directory already exists");
            } else {
                debug!("Clonings kernel sources");
                // If it doesn't exist, clone the repository
                let clone_args = if version == "latest" {
                    vec!["git", "clone", "--depth", "1", git_url, &kernel_dir]
                } else {
                    vec![
                        "git",
                        "clone",
                        "--depth",
                        "1",
                        "--branch",
                        version,
                        git_url,
                        &kernel_dir,
                    ]
                };
                run_command(&clone_args, false).context("Failed to clone kernel repository")?;
            }

            debug!("Building sources");
            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            std::env::set_current_dir(&kernel_dir)
                .context("Failed to change to kernel directory")?;

            // Configure and build the kernel
            run_command(&["make", "x86_64_defconfig"], false)
                .context("Failed to configure kernel")?;
            run_command(&["make", &format!("-j{}", num_cpus::get())], false)
                .context("Failed to build kernel")?;

            std::env::set_current_dir(current_dir)
                .context("Failed to change back to original directory")?;
        }

        let metadata = fs::metadata(&bzimage_path).context("get kernel file metadata")?;

        Ok(Self::Sources {
            git_url: git_url.to_string(),
            version: version.to_string(),
            source_path: PathBuf::from(&kernel_dir),
            path: PathBuf::from(&bzimage_path),
            size: metadata.len(),
        })
    }

    /// Use precompiled kernel.
    pub fn precompiled(path: PathBuf) -> Result<Self> {
        let metadata = fs::metadata(&path).context("get kernel file metadata")?;
        let size = metadata.len();
        Ok(Self::Precompiled { path, size })
    }
}

/// Build Linux kernel from sources.
pub struct Build;

impl Step<LinuxVMBuildContext> for Build {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("building Linux kernel");
        let kernel = Kernel::build(&ctx.opts().kernel_url, &ctx.opts().kernel_version)?;
        debug!("kernel built: {} ({} bytes)", &kernel, kernel.size());
        ctx.0.set("kernel", Box::new(kernel));
        Ok(())
    }
}

/// Use precompiled Linux kernel.
pub struct Precompiled;

impl Step<LinuxVMBuildContext> for Precompiled {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using precompiled Linux kernel");
        let kernel = Kernel::precompiled(
            ctx.opts()
                .kernel_file
                .as_ref()
                .ok_or(anyhow!("cannot use precompiled kernel: path is required"))?
                .clone(),
        )?;
        debug!("precompiled kernel: {} ({} bytes)", &kernel, kernel.size());
        ctx.0.set("kernel", Box::new(kernel));
        Ok(())
    }
}

pub struct Install;

impl Step<LinuxVMBuildContext> for Install {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing kernel");

        let kernel = ctx
            .0
            .get::<Kernel>("kernel")
            .ok_or(anyhow!("cannot install kernel: kernel not found"))?;

        let fs = ctx
            .0
            .get::<FileSystem>("fs")
            .ok_or(anyhow!("cannot install kernel: filesystem not found"))?;

        let mut kfile = fs::File::open(kernel.path()).context("open kernel file")?;
        let mut kernel_bytes = Vec::new();
        kfile
            .read_to_end(&mut kernel_bytes)
            .context("read kernel file")?;
        drop(kfile);

        let path = UnixPath::new("bzImage");
        fs.write_file(&path, &kernel_bytes)?;

        ctx.0.set(
            "installed-kernel",
            Box::new(UnixPathBuf::from("/").join(path)),
        );

        Ok(())
    }
}
