use std::fs;
use std::path::{Path, PathBuf};

use log::{debug, info};
use tempdir::TempDir;
use thiserror::Error;

use crate::builders::skopeo_builder::SkopeoSyslinuxBuilder;

#[derive(Error, Debug)]
pub enum NvidiaError {
    #[error("Failed to read kernel release file at {path}")]
    KernelReleaseRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to copy kernel source: {source}")]
    KernelSourceCopy { source: std::io::Error },

    #[error("Failed to run container for driver compilation: {source}")]
    ContainerRun { source: anyhow::Error },

    #[error("Failed to create VM modules directory: {source}")]
    VmModulesDirCreation { source: anyhow::Error },

    #[error("Failed to copy {file_desc} from {source_path} to {target}: {source_err}")]
    FileCopy {
        file_desc: String,
        source_path: PathBuf,
        target: PathBuf,
        source_err: anyhow::Error,
    },

    #[error("Failed to build module dependencies: {source}")]
    Depmod { source: anyhow::Error },

    #[error("Invalid UTF-8 sequence in path: {path:?}")]
    PathConversion { path: PathBuf },

    #[error(transparent)]
    Io(#[from] std::io::Error), // Fallback error for generic I/O errors
}

/// Install NVIDIA drivers into `vm_root_path`.
/// Host requirements:
/// - `podman` binary is available and can be used to run containers.
/// - `depmod` binary is available.
pub fn install_drivers<P1, P2>(kernel_source_dir: P1, vm_root_path: P2) -> Result<(), NvidiaError>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    // TIP: We could do extra initial checks here:
    // - Checking for kernel source access permissions.
    // - Checking for dangling symlinks that will not be accessible insider container volume mount.
    // - Checking for host requierements.

    // We need to know kernel name in order to identify location of driver files.
    debug!("Extracting kernel version.");
    let kernel_release = read_kernel_release(&kernel_source_dir)?;
    info!("Detected kernel: '{}'.", kernel_release);

    // Copy kernel sources, as we do NOT want them to be directly modified by NVIDIA installer.
    debug!("Copying kernel source.");
    let kernel_source_copy = copy_kernel_source(&kernel_source_dir)?;

    // Drivers tree will be dumped to temporary directory.
    debug!("Preparing target directory.");
    let target_dir = TempDir::new("driver-dump").map_err(NvidiaError::Io)?;

    // Run container for compiling and dumping NVIDIA drivers.
    // NOTE: We use it to reduce number of host dependencies **only**, not for isolation.
    debug!("Running container that prepares custom drivers.");
    run_driver_container(&kernel_source_copy, &target_dir)?;

    // We want to prepare '/lib/module/[kernel_release]' directory, that will be storage for modules.
    debug!("Preparing modules directory in VM filesystem.");
    prepare_vm_modules_dir(&vm_root_path, &kernel_release)?;

    // Put main `nvidia.ko` driver, and additional `nvidia-uvm.ko`.
    // NOTE: Target path is **quite** different than source one.
    debug!("Copying main NVIDIA driver into VM filesystem.");
    copy_nvidia_driver(&target_dir, &vm_root_path, &kernel_release, "nvidia.ko")?;
    copy_nvidia_driver(&target_dir, &vm_root_path, &kernel_release, "nvidia-uvm.ko")?;

    // Put main `libcuda.so.1` library that enables access to **driver API**.
    // NOTE: I observed this library is available somewhere deep in the cuda-samples filesystem, but
    // not really working. Seems like bug in NVIDIA samples container. Anyway, we MUST ship it here.
    // Related: https://stackoverflow.com/a/67165253.
    debug!("Copying NVIDIA libraries for the driver API");
    copy_driver_libraries(&target_dir, &vm_root_path)?;

    // NOTE: We could also put all CUDA runtime libraries into VM - for the **runtime API** - but it was
    // decided that they should be shipped as part of user's Containerfile. I am not convinced at all,
    // but we will see later.
    //debug!("Copying NVIDIA libraries for the runtime API");
    //copy_runtime_libraries(&target_dir, &vm_root_path)?;

    // Build `modules.dep`, so modules can be loaded easily with `modprobe` tool.
    debug!("Building modules definition.");
    build_module_dependencies(&vm_root_path, &kernel_release)?;

    info!("NVIDIA drivers ready!");

    Ok(())
}

fn read_kernel_release<P: AsRef<Path>>(kernel_source_dir: P) -> Result<String, NvidiaError> {
    let kernel_release_path = kernel_source_dir
        .as_ref()
        .join("include/config/kernel.release");
    let kernel_release_content = fs::read_to_string(kernel_release_path.clone()).map_err(|e| {
        NvidiaError::KernelReleaseRead {
            path: kernel_release_path,
            source: e,
        }
    })?;
    let kernel_release = kernel_release_content.trim().to_string();

    info!("Detected kernel: '{}'.", kernel_release);

    Ok(kernel_release)
}

fn copy_kernel_source<P: AsRef<Path>>(kernel_source_dir: P) -> Result<TempDir, NvidiaError> {
    let kernel_source_copy = TempDir::new("kernel-source-copy").map_err(NvidiaError::Io)?;
    copy_dir_all(kernel_source_dir, kernel_source_copy.as_ref())
        .map_err(|e| NvidiaError::KernelSourceCopy { source: e })?;

    Ok(kernel_source_copy)
}

fn run_driver_container(
    kernel_source_copy: &TempDir,
    target_dir: &TempDir,
) -> Result<(), NvidiaError> {
    let container_image = "docker.io/koxu1996/dump-custom-nvidia-driver:0.3.0"; // TODO: Replace with an official Gmulot image.
    let kernel_source_copy_str = path_to_str(kernel_source_copy.path())?;
    let target_dir_str = path_to_str(target_dir.path())?;

    SkopeoSyslinuxBuilder::run_command(
        &[
            "podman",
            "run",
            "--rm",
            "--volume",
            &format!("{}:/kernel_source", kernel_source_copy_str),
            "--volume",
            &format!("{}:/target_dir", target_dir_str),
            container_image,
            "/kernel_source",
            "/target_dir",
        ],
        false,
    )
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
    if !vm_modules_dir.exists() {
        let vm_modules_dir_str = path_to_str(&vm_modules_dir)?;
        // NOTE: We cannot use plain `fs::create_dir_all`, as skopeo creates root-owned directory for the VM.
        // This could be fixed in the future with a template + chown.
        SkopeoSyslinuxBuilder::run_command(
            &["sh", "-c", &format!("mkdir -p {}", vm_modules_dir_str)],
            true,
        )
        .map_err(|e| NvidiaError::VmModulesDirCreation { source: e })?;
    }

    Ok(())
}

fn copy_nvidia_driver<P: AsRef<Path>>(
    target_dir: &TempDir,
    vm_root_path: P,
    kernel_release: &str,
    driver_name: &str,
) -> Result<(), NvidiaError> {
    let source_path = target_dir
        .path()
        .join("usr/lib/modules")
        .join(kernel_release)
        .join("video")
        .join(driver_name);
    let target_path = vm_root_path
        .as_ref()
        .join("lib/modules")
        .join(kernel_release)
        .join(driver_name);

    copy_file(source_path, target_path, driver_name)?;

    Ok(())
}

fn copy_driver_libraries<P: AsRef<Path>>(
    target_dir: &TempDir,
    vm_root_path: P,
) -> Result<(), NvidiaError> {
    let source_path = target_dir
        .path()
        .join("usr/lib/x86_64-linux-gnu") // CAUTION: This is okay; it should NOT be the `kernel_release`.
        .join("libcuda.so.550.120"); // TODO: Dynamically find the filename.
    let target_path = vm_root_path.as_ref().join("lib").join("libcuda.so.1");

    copy_file(&source_path, &target_path, "libcuda.so")?;

    Ok(())
}

fn build_module_dependencies<P: AsRef<Path>>(
    vm_root_path: P,
    kernel_release: &str,
) -> Result<(), NvidiaError> {
    let vm_root_path_str = path_to_str(vm_root_path.as_ref())?;
    // NOTE: We have to use sudo, as skopeo creates root-owned directory for the VM.
    // This could be fixed in the future with a template + chown.
    SkopeoSyslinuxBuilder::run_command(
        &[
            "depmod",
            "--basedir",
            vm_root_path_str,
            "-a",
            kernel_release,
        ],
        true,
    )
    .map_err(|e| NvidiaError::Depmod { source: e })?;

    Ok(())
}

fn copy_file<P: AsRef<Path>>(
    source_path: P,
    target_path: P,
    file_desc: &str,
) -> Result<(), NvidiaError> {
    let source_path_str = path_to_str(source_path.as_ref())?;
    let target_path_str = path_to_str(target_path.as_ref())?;

    // NOTE: We cannot use plain `fs::copy`, as skopeo creates root-owned directory for the VM.
    // This could be fixed in the future with a template + chown.
    SkopeoSyslinuxBuilder::run_command(
        &[
            "sh",
            "-c",
            &format!("cp {} {}", source_path_str, target_path_str),
        ],
        true,
    )
    .map_err(|e| NvidiaError::FileCopy {
        file_desc: file_desc.to_string(),
        source_path: source_path.as_ref().to_path_buf(),
        target: target_path.as_ref().to_path_buf(),
        source_err: e,
    })?;

    Ok(())
}

fn path_to_str(path: &Path) -> Result<&str, NvidiaError> {
    path.to_str().ok_or_else(|| NvidiaError::PathConversion {
        path: path.to_path_buf(),
    })
}

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
