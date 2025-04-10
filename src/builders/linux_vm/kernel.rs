use anyhow::{Context, Result};
use base64::Engine;
use bytesize::ByteSize;
use log::{debug, info, trace};
use std::ffi::OsStr;
use std::path::{self, Path, PathBuf};
use std::{fmt, fs, io};

use crate::builders::linux_vm::filesystem::fat32::Fat32;
use crate::builders::linux_vm::image_file::ImageFile;
use crate::builders::linux_vm::mbr::{try_resize_to_fit_into, Mbr};
use crate::builders::Step;

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
        #[allow(unused)]
        git_url: String,

        /// Git version to checkout (e.g. `v6.10.11`).
        #[allow(unused)]
        version: String,

        /// Path to sources.
        source_path: PathBuf,

        /// Path to compiled binary.
        path: PathBuf,

        /// Size of the kernel file.
        size: u64,

        /// Kernel release string (e.g. 6.12.0).
        kernel_release: String,
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
    #[allow(unused)]
    pub fn source_path(&self) -> Option<&Path> {
        match self {
            Self::Precompiled { .. } => None,
            Self::Sources { source_path, .. } => Some(source_path.as_path()),
        }
    }

    /// Kernel release string if some.
    pub fn kernel_release(&self) -> Option<&str> {
        match self {
            Kernel::Precompiled { .. } => None,
            Kernel::Sources { kernel_release, .. } => Some(kernel_release),
        }
    }

    /// Size of kernel binary.
    pub fn size(&self) -> u64 {
        match self {
            Self::Precompiled { size, .. } => *size,
            Self::Sources { size, .. } => *size,
        }
    }

    /// Whether kernel was precompiled or not.
    #[allow(unused)]
    pub fn is_precompiled(&self) -> bool {
        matches!(self, Self::Precompiled { .. })
    }

    // TODO: maybe use libgit instead of executable?
    /// Clone Linux kernel repository into `path`.
    fn clone(git_url: &str, version: &str, path: &Path) -> Result<()> {
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
        command.push(path.as_os_str());
        run_command(&command).context("failed to clone kernel repository")?;
        Ok(())
    }

    /// Configure kernel.
    ///
    /// Assumes that CWD is kernel directory.
    fn configure() -> Result<()> {
        run_command(["make", "x86_64_defconfig"]).context("Failed to configure kernel")?;
        Self::configure_squashfs()?;
        Self::configure_cpu_nb(1024)?;
        Ok(())
    }

    /// Configure the kernel to allow a specific number of CPU
    ///
    /// Assumes that CWD is kernel directory.
    fn configure_cpu_nb(cpu: usize) -> Result<()> {
        let kernel_config_path = Path::new(".config");
        let config_content =
            fs::read_to_string(kernel_config_path).context("Failed to read kernel .config file")?;

        let new_content = config_content
            .lines()
            .map(|line| {
                if line.starts_with("CONFIG_NR_CPUS=") {
                    format!("CONFIG_NR_CPUS={}", cpu)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("\n");

        fs::write(kernel_config_path, new_content)
            .context("Failed to write modified kernel .config file")?;
        Ok(())
    }

    /// Configure SquashFS support in kernel.
    ///
    /// Assumes that CWD is kernel directory.
    fn configure_squashfs() -> Result<()> {
        // TODO: ensure stability of this configuration.
        // Kernel options can be removed/renamed. This configuration is used for v6.12,
        // but it may not work with other versions.

        const ENABLE: &[&str] = &[
            "CONFIG_SQUASHFS",
            "CONFIG_SQUASHFS_FILE_DIRECT",
            "CONFIG_SQUASHFS_DECOMP_SINGLE",
            "CONFIG_SQUASHFS_DECOMP_MULTI",
            "CONFIG_SQUASHFS_DECOMP_MULTI_PERCPU",
            "CONFIG_SQUASHFS_CHOICE_DECOMP_BY_MOUNT",
            "CONFIG_SQUASHFS_MOUNT_DECOMP_THREADS",
            "CONFIG_SQUASHFS_XATTR",
            "CONFIG_SQUASHFS_ZLIB",
            "CONFIG_SQUASHFS_LZ4",
            "CONFIG_SQUASHFS_LZO",
            "CONFIG_SQUASHFS_XZ",
            "CONFIG_SQUASHFS_ZSTD",
            "CONFIG_SQUASHFS_4K_DEVBLK_SIZE",
        ];

        const DISABLE: &[&str] = &["CONFIG_SQUASHFS_FILE_CACHE", "CONFIG_SQUASHFS_EMBEDDED"];

        const SET_VAL: &[(&str, &str)] = &[("3", "CONFIG_SQUASHFS_FRAGMENT_CACHE_SIZE")];

        for flag in ENABLE {
            run_command(["scripts/config", "--enable", flag])
                .context(format!("failed to enable {} flag to kernel config", flag))?;
        }

        for flag in DISABLE {
            run_command(["scripts/config", "--disable", flag])
                .context(format!("failed to disable {} flag to kernel config", flag))?;
        }

        for (val, flag) in SET_VAL {
            run_command(["scripts/config", "--set-val", val, flag])
                .context(format!("failed to set {} value to kernel config", flag))?;
        }

        Ok(())
    }

    /// Build kernel from sources.
    pub fn build(git_url: &str, version: &str, kernel_dir: &Path) -> Result<Self> {
        // TODO: check required tools are available: git, make, gcc

        Self::clone(git_url, version, kernel_dir)?;

        debug!("Building sources");
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        std::env::set_current_dir(kernel_dir).context("Failed to change to kernel directory")?;

        Self::configure()?;

        // Build the kernel
        run_command(["make", &format!("-j{}", num_cpus::get())])
            .context("Failed to build kernel")?;

        std::env::set_current_dir(current_dir)
            .context("Failed to change back to original directory")?;

        Self::use_existing(git_url, version, kernel_dir)
    }

    /// Use existing kernel compiled from sources.
    pub fn use_existing(git_url: &str, version: &str, kernel_dir: &Path) -> Result<Self> {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        std::env::set_current_dir(kernel_dir).context("Failed to change to kernel directory")?;

        let kernel_release = run_command(["make", "-s", "kernelrelease"])
            .context("failed to get kernel release string")?
            .0
            .trim()
            .to_string();

        let bzimage_path = kernel_dir.join(
            run_command(["make", "-s", "image_name"])
                .context("failed to get kernel image name")?
                .0
                .trim(),
        );
        debug!("kernel file: {}", bzimage_path.display());

        std::env::set_current_dir(current_dir)
            .context("Failed to change back to original directory")?;

        let metadata = fs::metadata(&bzimage_path).context("get kernel file metadata")?;

        Ok(Self::Sources {
            git_url: git_url.to_string(),
            version: version.to_string(),
            source_path: kernel_dir.to_path_buf(),
            path: PathBuf::from(&bzimage_path),
            size: metadata.len(),
            kernel_release,
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
        let build_dir = ctx
            .cache()
            .join("linux-build")
            .join(base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(&self.repository_url))
            .join(&self.version);
        debug!("kernel build directory: {}", build_dir.display());

        // Check if the cache entry (kernel directory) already exists
        let kernel = if build_dir.is_dir() {
            debug!("using cached kernel");
            Kernel::use_existing(&self.repository_url, &self.version, &build_dir)
                .context("failed to use kernel from cache")?
        } else {
            debug!("Clonings kernel sources");
            fs::create_dir_all(&build_dir).context("failed to create kernel directory")?;
            Kernel::build(&self.repository_url, &self.version, &build_dir)
                .context("failed to build Linux kernel")?
        };

        info!(
            "kernel ready: {} ({})",
            kernel
                .kernel_release()
                .expect("kernel is expected to be compiled from sources"),
            ByteSize::b(kernel.size()).display()
        );
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
        let kernel = Kernel::precompiled(self.file.clone())?;
        info!(
            "using precompiled Linux kernel: {} ({})",
            &kernel,
            ByteSize::b(kernel.size()).display()
        );
        ctx.set("kernel", Box::new(kernel));
        Ok(())
    }
}

/// Install Linux kernel into VM boot filesystem.
///
/// Only [`Fat32`] is supported now.
///
/// # Context variables required
/// - `kernel`
/// - `image-file`
/// - `boot-partition-number`
///
/// # Context variables defined
/// - `installed-kernel`: [`PathBuf`] - an absolute path **inside VM** of installed kernel,
///   e.g. `/boot/bzImage`
pub struct Install;

impl Step<LinuxVMBuildContext> for Install {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        let kernel = ctx.get::<Kernel>("kernel").expect("kernel");
        info!("installing kernel: {}", kernel.path().display());

        let image_file = ctx.get::<ImageFile>("image-file").expect("image-file");
        let boot_partition_number = *ctx
            .get::<usize>("boot-partition-number")
            .expect("boot-partition-number");

        let mbr_adapter = Mbr::read_from(image_file.path()).context("failed to read MBR")?;
        let mbr = mbr_adapter.mbr().context("failed to read MBR")?;
        let current_partition_size = mbr[boot_partition_number].sectors;
        let sector_size = mbr.sector_size;

        // Number of sectors required to store kernel
        let kernel_size = u32::try_from(kernel.size() / sector_size as u64)
            .context("kernel file is too big for MBR")?
            + 1;
        trace!("sectors required for kernel: {}s", kernel_size);

        // We add current partition size to preserve this, because it may already be
        // used to store bootloader and filesystem metadata.
        let partition_size = Mbr::round_up(kernel_size + current_partition_size);

        try_resize_to_fit_into(
            boot_partition_number,
            partition_size,
            image_file,
            mbr_adapter,
        )
        .context("failed to resize boot partition")?;
        for line in mbr_adapter.pretty_print()?.lines() {
            debug!("{}", line);
        }

        let (start, end) = mbr_adapter
            .partition_limits(boot_partition_number)
            .context("failed to get partition info")?;
        let fat32_adapter = Fat32::read_from(image_file.path(), start, end)
            .context("failed to read boot filesystem")?;

        let fs = fat32_adapter
            .fs()
            .context("failed to read boot filesystem")?;
        let stats = fs.stats().context("failed to get FAT32 stats")?;
        trace!("FAT32 cluster size: {}", stats.cluster_size());
        trace!("FAT32 free clusters: {}", stats.free_clusters());

        if (stats.free_clusters() as u64 * stats.cluster_size() as u64) < kernel.size() {
            trace!("attempt to resize FAT32 filesystem");
            Fat32::resize()?;
        }

        // Just hardcoded for now
        let installed_kernel_relative = "bzImage";

        let mut source = fs::File::open(kernel.path()).context("failed to open kernel file")?;
        let mut file = fs
            .root_dir()
            .create_file(installed_kernel_relative)
            .context("failed to create kernel file")?;

        trace!(
            "copying {} into {}:/{}",
            kernel.path().display(),
            image_file.path().display(),
            installed_kernel_relative
        );
        io::copy(&mut source, &mut file).context("kernel installation failed")?;

        let installed_kernel = Path::new(path::MAIN_SEPARATOR_STR).join(installed_kernel_relative);
        info!(
            "kernel installed to {}:{}",
            image_file.path().display(),
            installed_kernel.display()
        );
        ctx.0.set("installed-kernel", Box::new(installed_kernel));

        Ok(())
    }
}
