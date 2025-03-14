use anyhow::{bail, Context, Result};
use bytesize::ByteSize;
use log::{debug, error, info, trace};
use std::fs;
use std::path::{Path, PathBuf};
use tempdir::TempDir;
use thiserror::Error;

use crate::builders::linux_vm::kernel::Kernel;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::Step;

use super::LinuxVMBuildContext;

#[derive(Error, Debug)]
pub enum NvidiaError {
    #[error("Failed to copy kernel source: {source}")]
    KernelSourceCopy { source: std::io::Error },

    #[error("Failed to run container for driver compilation: {source}")]
    ContainerRun { source: anyhow::Error },

    #[error("Failed to build module dependencies: {source}")]
    Depmod { source: anyhow::Error },

    #[error("Invalid UTF-8 sequence in path: {path:?}")]
    PathConversion { path: PathBuf },

    #[error(transparent)]
    Io(#[from] std::io::Error), // Fallback error for generic I/O errors
}

// hardcoded for now - we always build this version of driver
/// Version of the installed driver (constant because we only use this version now).
const DRIVER_VERSION: &str = "550.120";

/// Represents NVIDIA dumped NVIDIA drivers.
#[derive(Debug)]
pub struct NvidiaDriversFs {
    target_dir: PathBuf,
    kernel_release: String,
}

impl NvidiaDriversFs {
    /// Use existing (already built) NVIDIA drivers.
    pub fn use_existing(target_dir: PathBuf, kernel_release: String) -> Self {
        Self {
            target_dir,
            kernel_release,
        }
    }

    /// Path to drivers directory.
    pub fn path(&self) -> &Path {
        self.target_dir.as_path()
    }

    /// Kernel release string.
    pub fn kernel_release(&self) -> &str {
        self.kernel_release.as_str()
    }

    /// Build and dump drivers.
    pub fn build<P: AsRef<Path>>(
        kernel_source_dir: P,
        kernel_release: String,
        target_dir: PathBuf,
    ) -> Result<Self> {
        // TIP: We could do extra initial checks here:
        // - Checking for kernel source access permissions.
        // - Checking for dangling symlinks that will not be accessible insider container volume mount.
        // - Checking for host requierements.

        // Copy kernel sources, as we do NOT want them to be directly modified by NVIDIA installer.
        debug!("Copying kernel source.");
        let kernel_source_copy = copy_kernel_source(&kernel_source_dir)?;

        // Run container for compiling and dumping NVIDIA drivers.
        // NOTE: We use it to reduce number of host dependencies **only**, not for isolation.
        debug!("Running container that prepares custom drivers.");
        run_driver_container(&kernel_source_copy, &target_dir)?;

        Ok(Self {
            target_dir,
            kernel_release,
        })
    }

    /// Get size of all files to install.
    pub fn size(&self) -> Result<u64> {
        fs_extra::dir::get_size(self.path()).map_err(Into::into)
    }

    /// Install drivers returning list of names of installed drivers.
    ///
    /// Requires `depmod` binary to be available.
    pub fn install<P: AsRef<Path>>(&self, mountpoint: P) -> Result<Vec<String>> {
        // We want to prepare '/lib/module/[kernel_release]' directory, that will be storage for modules.
        debug!("Preparing modules directory in VM filesystem.");
        prepare_vm_modules_dir(&mountpoint, &self.kernel_release)?;

        // Put main `nvidia.ko` driver, and additional `nvidia-uvm.ko`.
        // NOTE: Target path is **quite** different than source one.
        debug!("Copying main NVIDIA driver into VM filesystem.");
        let driver_names = vec!["nvidia".to_string(), "nvidia-uvm".to_string()];
        for driver_name in &driver_names {
            copy_nvidia_driver(
                &self.target_dir,
                &mountpoint,
                &self.kernel_release,
                driver_name,
            )?;
        }

        // Put main `libcuda.so.1` library that enables access to **driver API**.
        // NOTE: I observed this library is available somewhere deep in the cuda-samples filesystem, but
        // not really working. Seems like bug in NVIDIA samples container. Anyway, we MUST ship it here.
        // Related: https://stackoverflow.com/a/67165253.
        debug!("Copying NVIDIA libraries for the driver API");
        copy_driver_libraries(&self.target_dir, &mountpoint)?;

        // NOTE: We could also put all CUDA runtime libraries into VM - for the **runtime API** - but it was
        // decided that they should be shipped as part of user's Containerfile. I am not convinced at all,
        // but we will see later.
        //debug!("Copying NVIDIA libraries for the runtime API");
        //copy_runtime_libraries(&target_dir, &vm_root_path)?;

        // Build `modules.dep`, so modules can be loaded easily with `modprobe` tool.
        debug!("Building modules definition.");
        build_module_dependencies(&mountpoint, &self.kernel_release)?;

        Ok(driver_names)
    }
}

fn copy_kernel_source<P: AsRef<Path>>(kernel_source_dir: P) -> Result<TempDir, NvidiaError> {
    let kernel_source_copy = TempDir::new("kernel-source-copy").map_err(NvidiaError::Io)?;
    copy_dir_all(kernel_source_dir, kernel_source_copy.as_ref())
        .map_err(|e| NvidiaError::KernelSourceCopy { source: e })?;

    Ok(kernel_source_copy)
}

fn run_driver_container(
    kernel_source_copy: &TempDir,
    target_dir: &Path,
) -> Result<(), NvidiaError> {
    let container_image = "docker.io/koxu1996/dump-custom-nvidia-driver:0.3.0"; // TODO: Replace with an official Gmulot image.
    let kernel_source_copy_str = path_to_str(kernel_source_copy.path())?;
    let target_dir_str = path_to_str(target_dir)?;

    run_command([
        "podman",
        "run",
        "--rm",
        "--volume",
        &format!("{}:/kernel_source:Z", kernel_source_copy_str),
        "--volume",
        &format!("{}:/target_dir:Z", target_dir_str),
        container_image,
        "/kernel_source",
        "/target_dir",
    ])
    .map_err(|e| NvidiaError::ContainerRun { source: e })?;

    Ok(())
}

fn prepare_vm_modules_dir<P: AsRef<Path>>(
    vm_root_path: P,
    kernel_release: &str,
) -> Result<(), NvidiaError> {
    let vm_modules_dir = vm_root_path
        .as_ref()
        .join("lib/modules")
        .join(kernel_release);
    fs::create_dir_all(vm_modules_dir)?;
    Ok(())
}

fn copy_nvidia_driver<P: AsRef<Path>>(
    target_dir: &Path,
    vm_root_path: P,
    kernel_release: &str,
    driver_name: &str,
) -> Result<(), NvidiaError> {
    let driver_name = format!("{}.ko", driver_name);
    let source_path = target_dir
        .join("usr/lib/modules")
        .join(kernel_release)
        .join("video")
        .join(&driver_name);
    let target_path = vm_root_path
        .as_ref()
        .join("lib/modules")
        .join(kernel_release)
        .join(&driver_name);

    fs::copy(source_path, target_path)?;

    Ok(())
}

fn copy_driver_libraries<P: AsRef<Path>>(
    target_dir: &Path,
    vm_root_path: P,
) -> Result<(), NvidiaError> {
    let source_path = target_dir
        .join("usr/lib/x86_64-linux-gnu") // CAUTION: This is okay; it should NOT be the `kernel_release`.
        .join("libcuda.so.550.120"); // TODO: Dynamically find the filename.
    let target_path = vm_root_path.as_ref().join("lib").join("libcuda.so.1");

    fs::copy(source_path, target_path)?;

    Ok(())
}

fn build_module_dependencies<P: AsRef<Path>>(
    vm_root_path: P,
    kernel_release: &str,
) -> Result<(), NvidiaError> {
    let vm_root_path_str = path_to_str(vm_root_path.as_ref())?;
    run_command([
        "depmod",
        "--basedir",
        vm_root_path_str,
        "-a",
        kernel_release,
    ])
    .map_err(|e| NvidiaError::Depmod { source: e })?;

    Ok(())
}

// TODO: use `std::ffi::OsStr` instead?
fn path_to_str(path: &Path) -> Result<&str, NvidiaError> {
    path.to_str().ok_or_else(|| NvidiaError::PathConversion {
        path: path.to_path_buf(),
    })
}

// TODO: replace with `fs_extra::dir::copy`?
/// Utility for copying directories.
fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?; // Ensure the destination directory exists
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;

        let dest_path = dst.as_ref().join(entry.file_name());

        if ty.is_dir() {
            // Recursively copy the directory
            copy_dir_all(entry.path(), &dest_path)?;
        } else if ty.is_symlink() {
            // Handle symlinks
            let symlink_target = fs::read_link(entry.path())?;
            std::os::unix::fs::symlink(symlink_target, dest_path)?;
        } else {
            // Copy regular file
            fs::copy(entry.path(), &dest_path)?;
        }
    }

    Ok(())
}

/// Build NVIDIA drivers and dump them into temp directory.
///
/// # Context variables required
/// - `kernel`
///
/// # Context variables defined
/// - `nvidia-drivers` (if kernel is not pre-compiled)
pub struct BuildDrivers;

impl Step<LinuxVMBuildContext> for BuildDrivers {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("building NVIDIA drivers");
        let kernel = ctx.get::<Kernel>("kernel").expect("kernel");

        match kernel {
            Kernel::Precompiled { .. } => {
                error!("Building NVIDIA drivers for precompiled kernel is not supported yet!");
                bail!("cannot build NVIDIA drivers without kernel sources");
            }
            Kernel::Sources {
                source_path,
                kernel_release,
                ..
            } => {
                let target_dir = ctx
                    .cache()
                    .join("nvidia")
                    .join(DRIVER_VERSION)
                    .join(kernel_release);

                let nvidia_drivers = if target_dir.is_dir() {
                    // If cache entry (directory) already exists, use cached drivers
                    let nvidia_drivers =
                        NvidiaDriversFs::use_existing(target_dir, kernel_release.clone());
                    info!(
                        "using cached NVIDIA drivers (kernel release: {}, driver version: {}) - {}",
                        nvidia_drivers.kernel_release(),
                        DRIVER_VERSION,
                        ByteSize::b(nvidia_drivers.size()?)
                    );
                    debug!("drivers cache path: {}", nvidia_drivers.path().display());
                    nvidia_drivers
                } else {
                    fs::create_dir_all(&target_dir).map_err(NvidiaError::Io)?;

                    let nvidia_drivers =
                        NvidiaDriversFs::build(source_path, kernel_release.clone(), target_dir)
                            .context("failed to build NVIDIA drivers")?;
                    trace!(
                        "NVIDIA drivers dumped to {}",
                        nvidia_drivers.path().display()
                    );

                    info!(
                        "NVIDIA drivers built (kernel release: {}, driver version: {}) - {}",
                        nvidia_drivers.kernel_release(),
                        DRIVER_VERSION,
                        ByteSize::b(nvidia_drivers.size()?)
                    );
                    nvidia_drivers
                };

                ctx.set("nvidia-drivers", Box::new(nvidia_drivers));
            }
        }

        Ok(())
    }
}

/// Install NVIDIA drivers.
///
/// Requires kernel compiled from sources.
///
/// # Context variables required
/// - `root-fs`
/// - `nvidia-drivers` (if not found, nothing will be installed)
///
/// # Context variables defined
/// - `kernel-modules` (if anything was installed)
pub struct InstallDrivers;

impl Step<LinuxVMBuildContext> for InstallDrivers {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        // If there are no drivers, just skip installation.
        // This happens in case of pre-compiled kernel.
        if let Some(nvidia_drivers) = ctx.get::<NvidiaDriversFs>("nvidia-drivers") {
            info!("installing NVIDIA drivers");
            let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");

            let mut driver_names = nvidia_drivers
                .install(rootfs)
                .context("failed to install NVIDIA drivers")?;

            if let Some(kernel_modules) = ctx.get_mut::<Vec<String>>("kernel-modules") {
                kernel_modules.append(&mut driver_names);
            } else {
                // If no modules were added before, create them
                ctx.set("kernel-modules", Box::new(driver_names));
            }
            info!("NVIDIA drivers ready!");
        }

        Ok(())
    }
}

// TODO: cache built drivers to avoid re-compilation, which takes a lot of time
