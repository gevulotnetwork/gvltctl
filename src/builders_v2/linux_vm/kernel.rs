use anyhow::{Context, Result};
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::ffi::OsStr;
use std::path::{self, Path, PathBuf};
use std::{fmt, fs};

use crate::builders::Step;

use super::utils::run_command;
use super::{LinuxVMBuildContext, LinuxVMBuilderError};

/// Linux kernel.
#[derive(Debug)]
pub enum Kernel {
    /// Precompiled kernel.
    Precompiled {
        /// Path to binary file.
        path: PathBuf,

        /// Size of the kernel file.
        size: ByteSize,
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
        size: ByteSize,
    },
}

impl fmt::Display for Kernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = match self {
            Self::Precompiled { path, .. } => path,
            Self::Sources { path, .. } => path,
        };
        f.write_str(&format!("{}", path.display()))
    }
}

impl Kernel {
    /// Path to the kernel binary.
    pub fn path(&self) -> &Path {
        match self {
            Self::Precompiled { path, .. } => path.as_path(),
            Self::Sources { path, .. } => path.as_path(),
        }
    }

    /// Return path to sources if some.
    pub fn source_path(&self) -> Option<&Path> {
        match self {
            Self::Precompiled { .. } => None,
            Self::Sources { source_path, .. } => Some(source_path.as_path()),
        }
    }

    /// Size of kernel binary.
    pub fn size(&self) -> ByteSize {
        match self {
            Self::Precompiled { size, .. } => *size,
            Self::Sources { size, .. } => *size,
        }
    }

    /// Whether kernel was precompiled or not.
    pub fn is_precompiled(&self) -> bool {
        matches!(self, Self::Precompiled { .. })
    }

    // TODO: use this function.
    // TODO: maybe use libgit instead of executable?
    /// Clone Linux kernel repository into `path/version` returning path to resulting directory.
    fn clone(git_url: &str, version: &str, path: &Path) -> Result<PathBuf> {
        let target_path = path.join(version);
        let mut command = vec![
            OsStr::new("git"),
            OsStr::new("clone"),
            OsStr::new("--depth"),
            OsStr::new("1"),
        ];
        if version != "latest" {
            command.push(OsStr::new("--branch"));
            command.push(OsStr::new(version));
        }
        command.push(OsStr::new(git_url));
        command.push(target_path.as_os_str());
        run_command(&command).context("failed to clone kernel repository")?;
        Ok(target_path)
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
                run_command(&clone_args).context("Failed to clone kernel repository")?;
            }

            debug!("Building sources");
            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            std::env::set_current_dir(&kernel_dir)
                .context("Failed to change to kernel directory")?;

            // Configure the kernel
            run_command(&["make", "x86_64_defconfig"]).context("Failed to configure kernel")?;

            // SQUASHFS support
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS"])
                .context("Failed to enable CONFIG_SQUASHFS flag to kernel config ")?;

            run_command(&["scripts/config", "--disable", "CONFIG_SQUASHFS_FILE_CACHE"])
                .context("Failed to disable CONFIG_SQUASHFS_FILE_CACHE flag to kernel config ")?;

            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_FILE_DIRECT"])
                .context("Failed to enable CONFIG_SQUASHFS_FILE_DIRECT flag to kernel config")?;

            run_command(&[
                "scripts/config",
                "--enable",
                "CONFIG_SQUASHFS_DECOMP_SINGLE",
            ])
            .context("Failed to enable CONFIG_SQUASHFS_DECOMP_SINGLE flag to kernel config")?;

            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_DECOMP_MULTI"])
                .context("Failed to enable CONFIG_SQUASHFS_DECOMP_MULTI flag to kernel config")?;

            run_command(&[
                "scripts/config",
                "--enable",
                "CONFIG_SQUASHFS_DECOMP_MULTI_PERCPU",
            ])
            .context(
                "Failed to enable CONFIG_SQUASHFS_DECOMP_MULTI_PERCPU flag to kernel config",
            )?;

            run_command(&[
                "scripts/config",
                "--enable",
                "CONFIG_SQUASHFS_CHOICE_DECOMP_BY_MOUNT",
            ])
            .context(
                "Failed to enable CONFIG_SQUASHFS_CHOICE_DECOMP_BY_MOUNT flag to kernel config",
            )?;
            run_command(&[
                "scripts/config",
                "--enable",
                "CONFIG_SQUASHFS_MOUNT_DECOMP_THREADS",
            ])
            .context(
                "Failed to enable CONFIG_SQUASHFS_MOUNT_DECOMP_THREADS flag to kernel config",
            )?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_XATTR"])
                .context("Failed to enable CONFIG_SQUASHFS_XATTR flag to kernel config")?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_ZLIB"])
                .context("Failed to enable CONFIG_SQUASHFS_ZLIB flag to kernel config")?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_LZ4"])
                .context("Failed to enable CONFIG_SQUASHFS_LZ4 flag to kernel config")?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_LZO"])
                .context("Failed to enable CONFIG_SQUASHFS_LZO flag to kernel config")?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_XZ"])
                .context("Failed to enable CONFIG_SQUASHFS_XZ flag to kernel config")?;
            run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_ZSTD"])
                .context("Failed to enable CONFIG_SQUASHFS_ZSTD flag to kernel config")?;
            run_command(&[
                "scripts/config",
                "--enable",
                "CONFIG_SQUASHFS_4K_DEVBLK_SIZE",
            ])
            .context("Failed to enable CONFIG_SQUASHFS_4K_DEVBLK_SIZE flag to kernel config")?;
            run_command(&[
                "scripts/config",
                "--set-val",
                "3",
                "CONFIG_SQUASHFS_FRAGMENT_CACHE_SIZE",
            ])
            .context("Failed to set CONFIG_SQUASHFS_FRAGMENT_CACHE_SIZE value to kernel config")?;
            run_command(&["scripts/config", "--disable", "CONFIG_SQUASHFS_EMBEDDED"])
                .context("Failed to disable CONFIG_SQUASHFS_EMBEDDED flag to kernel config ")?;

            // Build the kernel
            run_command(&["make", &format!("-j{}", num_cpus::get())])
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
            size: ByteSize::b(metadata.len()),
        })
    }

    /// Use precompiled kernel.
    pub fn precompiled(path: PathBuf) -> Result<Self> {
        let metadata = fs::metadata(&path).context("get kernel file metadata")?;
        let size = ByteSize::b(metadata.len());
        Ok(Self::Precompiled { path, size })
    }
}

/// Build Linux kernel from sources.
///
/// # Context variables defined
/// - `kernel`
pub struct Build {
    repository_url: String,
    version: String,
}

impl Build {
    pub fn new(repository_url: String, version: String) -> Self {
        Self {
            repository_url,
            version,
        }
    }
}

impl Step<LinuxVMBuildContext> for Build {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("building Linux kernel");
        let kernel = Kernel::build(&self.repository_url, &self.version)
            .context("failed to build Linux kernel")?;
        debug!("kernel built: {} ({} bytes)", &kernel, kernel.size());
        ctx.set("kernel", Box::new(kernel));
        Ok(())
    }
}

/// Use precompiled Linux kernel.
///
/// # Context variables defined
/// - `kernel`
pub struct Precompiled {
    file: PathBuf,
}

impl Precompiled {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }
}

impl Step<LinuxVMBuildContext> for Precompiled {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("using precompiled Linux kernel");
        let kernel = Kernel::precompiled(self.file.clone())?;
        debug!("precompiled kernel: {} ({} bytes)", &kernel, kernel.size());
        ctx.set("kernel", Box::new(kernel));
        Ok(())
    }
}

/// Install Linux kernel into VM.
///
/// # Context variables required
/// - `kernel`
/// - `mountpoint`
///
/// # Context variables defined
/// - `installed-kernel`
pub struct Install;

impl Step<LinuxVMBuildContext> for Install {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let kernel = ctx
            .0
            .get::<Kernel>("kernel")
            .ok_or(LinuxVMBuilderError::invalid_context(
                "install kernel",
                "kernel handler",
            ))?;
        info!("installing kernel: {}", kernel.path().display());

        let mountpoint =
            ctx.0
                .get::<PathBuf>("mountpoint")
                .ok_or(LinuxVMBuilderError::invalid_context(
                    "install kernel",
                    "mountpoint",
                ))?;

        // Just hardcoded for now
        let installed_kernel_relative = PathBuf::from("bzImage");

        let kernel_path = mountpoint.join(&installed_kernel_relative);
        trace!(
            "copying {} into {}",
            kernel.path().display(),
            kernel_path.display()
        );
        fs::copy(kernel.path(), &kernel_path).context("kernel installation failed")?;

        let installed_kernel = Path::new(path::MAIN_SEPARATOR_STR).join(installed_kernel_relative);
        debug!(
            "kernel installed to {}:{}",
            ctx.opts().image_file_opts.path.display(),
            installed_kernel.display()
        );
        ctx.0.set("installed-kernel", Box::new(installed_kernel));

        Ok(())
    }
}
