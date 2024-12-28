use anyhow::{Context, Result};
use log::debug;
use mia_installer::runtime_config::{self, RuntimeConfig};
use oci_spec::image::ImageConfiguration;
use std::io::{self, BufRead, BufReader, Write};
use std::{env, fs, path::Path, process::Command};
use tempdir::TempDir;

use crate::builders::{BuildOptions, ImageBuilder};

use super::nvidia;

const TEMP_CONTAINER_NAME: &str = "gevulot-temp-container";

pub struct SkopeoSyslinuxBuilder {}

impl ImageBuilder for SkopeoSyslinuxBuilder {
    fn build(&self, options: &BuildOptions) -> Result<()> {
        // Handle printing messages with regard to `quiet` option.
        let print = |line: &str| -> Result<()> {
            if !options.quiet {
                print!("{}", line);
                io::stdout().flush().context("Failed to flush stdout")?;
            }
            Ok(())
        };

        print(&format!(
            "Building image {} with options:\n",
            options.output_file
        ))?;

        print(&format!("{}", options))?;

        // Check if the output file already exists
        if Path::new(&options.output_file).exists() {
            if !options.force {
                anyhow::bail!("Output file '{}' already exists. Please choose a different filename or remove the existing file.", &options.output_file);
            } else {
                fs::remove_file(&options.output_file)
                    .context("Failed to remove existing output file")?;
            }
        }

        if options.force {
            print(&format!("Cleaning up old attempts... "))?;
            if Self::cleanup().is_ok() {
                print(&format!("✅\n"))?;
            } else {
                print(&format!("❌\n"))?;
            }
        }

        // Execute the main steps to create the bootable disk image
        let result = (|| -> Result<()> {
            print(&format!("Creating disk image... "))?;
            Self::create_disk_image(&options.image_size, &options.output_file)?;
            print(&format!("✅\n"))?;

            print(&format!("Creating partitions... "))?;
            Self::create_partitions(&options.output_file)?;
            print(&format!("✅\n"))?;

            print(&format!("Setting up loop device... "))?;
            Self::setup_loop_device(&options.output_file)?;
            print(&format!("✅\n"))?;

            print(&format!("Creating filesystems... "))?;
            Self::create_filesystems(&options.output_file)?;
            print(&format!("✅\n"))?;

            print(&format!("Mounting filesystems... "))?;
            Self::mount_filesystems(&options.output_file)?;
            print(&format!("✅\n"))?;

            let mut container_rt_config = RuntimeConfig::default();
            let mut kernel_modules = options.kernel_modules.clone();

            if let Some(container_source) = &options.container_source {
                print(&format!("Installing rootfs from container... "))?;
                Self::install_rootfs_from_container(container_source, &mut container_rt_config)?;
                print(&format!("✅\n"))?;
            } else if let Some(rootfs_dir) = &options.rootfs_dir {
                print(&format!("Installing rootfs from directory... "))?;
                Self::install_rootfs_from_directory(rootfs_dir)?;
                print(&format!("✅\n"))?;
            } else if let Some(containerfile) = &options.containerfile {
                print(&format!(
                    "Building and installing rootfs from Containerfile... "
                ))?;
                Self::build_and_install_rootfs_from_containerfile(
                    containerfile,
                    &mut container_rt_config,
                )?;
                print(&format!("✅\n"))?;
            }

            print("Creating input/output context directories...")?;
            Self::create_mount_dirs()?;
            print("✅\n")?;

            if let Some(kernel_path) = &options.kernel_file {
                if options.nvidia_drivers {
                    print("WARNING: Installing NVIDIA drivers for precompiled kernel is not supported yet!")?;
                }
                print(&format!("Installing precompiled kernel... "))?;
                Self::install_precompiled_kernel(kernel_path)?;
                print(&format!("✅\n"))?;
            } else {
                print(&format!("Installing kernel... "))?;
                Self::install_kernel(
                    &options.kernel_version,
                    options
                        .kernel_url
                        .as_ref()
                        .context("Kernel URL is required")?,
                    options.nvidia_drivers,
                    &mut kernel_modules,
                )?;
                print(&format!("✅\n"))?;
            }

            // Without explicit init, mia will be used.
            if options.init.is_none() {
                print(&format!("Installing MIA (Minimal Init Application)... "))?;
                Self::install_mia(
                    options
                        .mia_version
                        .as_ref()
                        .context("MIA version is required")?,
                    &container_rt_config,
                    &kernel_modules,
                    &options.mounts,
                    !options.no_gevulot_runtime,
                    !options.no_default_mounts,
                )?;
                print(&format!("✅\n"))?;
            } else {
                print("WARNING: Using custom init system is considered unstable for now!")?;
            }

            print(&format!("Installing bootloader... "))?;
            Self::install_bootloader(
                options.init.as_deref(),
                options.init_args.as_deref(),
                &options.output_file,
                options.rw_root,
                options.mbr_file.as_deref(),
            )?;
            print(&format!("✅\n"))?;

            print(&format!("Setting bootable flag... "))?;
            Self::set_bootable_flag(&options.output_file)?;
            print(&format!("✅\n"))?;

            Ok(())
        })();

        if let Err(e) = &result {
            log::error!("error: {:#}", e);
        }

        // Always call cleanup, even if there was an error
        print(&format!("Cleaning up... "))?;
        Self::cleanup()?;
        print(&format!("✅\n"))?;

        // Check if there was an error and return it
        result?;

        // Print success message and instructions for running the image
        print(&format!("Image created successfully ✅"))?;
        print(&format!("\nYou can run the image with qemu like this:\n"))?;
        print(&format!("qemu-system-x86_64 \\\n"))?;
        print(&format!("   -m 1024 \\\n"))?;
        print(&format!("   -enable-kvm \\\n"))?;
        print(&format!("   -nographic \\\n"))?;
        print(&format!("   --hda ./{}\n", options.output_file))?;
        Ok(())
    }
}

impl SkopeoSyslinuxBuilder {
    // Create an empty disk image file of the specified size
    fn create_disk_image(size: &str, output_file: &str) -> Result<()> {
        Self::run_command(&["truncate", "-s", size, output_file], false)
            .context("Failed to create disk image")
    }

    // Create partitions on the disk image using fdisk
    fn create_partitions(output_file: &str) -> Result<()> {
        let fdisk_commands = "o\nn\np\n1\n2048\n+200M\nn\np\n2\n\n\nt\n2\n83\nw\n";
        let mut child = Command::new("fdisk")
            .arg(output_file)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn fdisk command")?;

        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(fdisk_commands.as_bytes())
            .context("Failed to write fdisk commands")?;

        let status = child.wait().context("Failed to wait for fdisk command")?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "fdisk command failed with status {}",
                status
            ))
        }
    }

    // Set the bootable flag on the first partition
    fn set_bootable_flag(output_file: &str) -> Result<()> {
        let fdisk_bootable = "a\n1\nw\n";
        let mut child = Command::new("fdisk")
            .arg(output_file)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn fdisk command for setting bootable flag")?;

        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(fdisk_bootable.as_bytes())
            .context("Failed to write fdisk bootable commands")?;

        let status = child.wait().context("Failed to wait for fdisk command")?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "fdisk command failed with status {}",
                status
            ))
        }
    }

    // Set up a loop device for the disk image
    fn setup_loop_device(output_file: &str) -> Result<()> {
        Self::run_command(&["losetup", "-fP", output_file], true)
            .context("Failed to set up loop device")
    }

    // Create filesystems on the partitions
    fn create_filesystems(output_file: &str) -> Result<()> {
        let loop_device = Self::get_loop_device(output_file)?;
        Self::run_command(
            &["mkfs.vfat", "-n", "BOOT", &format!("{}p1", loop_device)],
            true,
        )
        .context("Failed to create VFAT filesystem")?;
        Self::run_command(
            &["mkfs.ext4", "-L", "ROOTFS", &format!("{}p2", loop_device)],
            true,
        )
        .context("Failed to create EXT4 filesystem")?;
        Ok(())
    }

    // Mount the filesystems
    fn mount_filesystems(output_file: &str) -> Result<()> {
        let loop_device = Self::get_loop_device(output_file)?;
        fs::create_dir_all(env::temp_dir().join("mnt"))
            .context("Failed to create mount directory")?;
        Self::run_command(
            &[
                "mount",
                &format!("{}p2", loop_device),
                env::temp_dir().join("mnt").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to mount root filesystem")?;
        Self::run_command(
            &[
                "mkdir",
                "-p",
                env::temp_dir().join("mnt").join("boot").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to create boot directory")?;
        Self::run_command(
            &[
                "mount",
                &format!("{}p1", loop_device),
                env::temp_dir().join("mnt").join("boot").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to mount boot filesystem")?;
        Ok(())
    }

    // Install the root filesystem from a container image
    fn install_rootfs_from_container(
        container_source: &str,
        rt_config: &mut RuntimeConfig,
    ) -> Result<()> {
        // This temp dir will be removed on dropping.
        let target_dir = TempDir::new("").context("Failed to create temporary directory")?;

        // Create a container to obtain full filesystem, instead of dealing with layers.
        Self::run_command(
            &[
                "podman",
                "create",
                "--replace",
                "--name",
                TEMP_CONTAINER_NAME,
                container_source,
            ],
            false,
        )
        .context("Failed to create container")?;

        // Export archive with the container filesystem
        let output_file = target_dir.path().join("rootfs.tar");
        let output_file_str = output_file.to_str().unwrap();
        Self::run_command(
            &[
                "podman",
                "export",
                TEMP_CONTAINER_NAME,
                "-o",
                output_file_str,
            ],
            false,
        )
        .context("Failed to extract container filesystem")?;

        // Remove container - it is no longer necessary as we got filesystem dump.
        Self::remove_temporary_container().context("Failed to remove container")?;

        // Unpack with root permissions
        Self::run_command(
            &[
                "tar",
                "-xf",
                output_file_str,
                "-C",
                target_dir.path().to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to unpack container filesystem")?;

        log::debug!("unpacked container image");

        // Copy the extracted rootfs to the mounted filesystem
        Self::run_command(
            &[
                "sh",
                "-c",
                &format!(
                    "cp -a {}/. {}", // NOTE: It preserves all attributes, symlinks and includes hidden files.
                    target_dir.path().display(),
                    env::temp_dir().join("mnt").to_str().unwrap()
                ),
            ],
            true,
        )
        .context("Failed to copy rootfs to mounted filesystem")?;

        // Ensure all changes are written to disk
        Self::run_command(&["sync"], true).context("Failed to sync filesystem")?;

        // Read image config.
        let config_path = target_dir.path().join("config.json");
        let config_path_str = config_path.to_str().unwrap();
        Self::run_command(
            &[
                "sh",
                "-c",
                &format!(
                    "skopeo inspect --config {} > {}",
                    container_source, config_path_str
                ),
            ],
            false,
        )
        .context("Failed to extract image config")?;
        let config = ImageConfiguration::from_file(&config_path)
            .context("Failed to read image configuration")?;
        log::debug!("unpacked config {}", config_path.display());

        // Extract runtime config from the container manifest.
        if let Some(exec_params) = config.config() {
            // Add enviromnental variables
            if let Some(env_vars) = exec_params.env() {
                for var in env_vars {
                    let (key, value) = var
                        .split_once('=')
                        .ok_or(anyhow::anyhow!("invalid environment variable"))?;
                    rt_config.env.push(runtime_config::EnvVar {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            }

            rt_config.working_dir = exec_params.working_dir().clone();

            let mut exec_string = Vec::new();
            // Try to get the ENTRYPOINT execution params
            if let Some(entrypoint) = exec_params.entrypoint() {
                exec_string.append(&mut entrypoint.clone());
            }
            // Try to get CMD from execution params
            if let Some(cmd) = exec_params.cmd() {
                exec_string.append(&mut cmd.clone());
            }

            if exec_string.is_empty() {
                // Do nothing, image have no default commands.
            } else {
                rt_config.command = Some(exec_string[0].clone());
                rt_config.args = exec_string[1..].to_vec();
            }
        }

        Ok(())
    }

    // Install the root filesystem from a directory
    fn install_rootfs_from_directory(rootfs_dir: &str) -> Result<()> {
        // Copy the rootfs directory to the mounted filesystem
        Self::run_command(
            &[
                "sh",
                "-c",
                &format!(
                    "cp -r {}/* {}",
                    rootfs_dir,
                    env::temp_dir().join("mnt").to_str().unwrap()
                ),
            ],
            true,
        )
        .context("Failed to copy rootfs from directory")?;

        // Ensure all changes are written to disk
        Self::run_command(&["sync"], true).context("Failed to sync filesystem")?;
        Ok(())
    }

    // Build and install the root filesystem from a Containerfile
    fn build_and_install_rootfs_from_containerfile(
        containerfile: &str,
        rt_config: &mut RuntimeConfig,
    ) -> Result<()> {
        let container_source = "containers-storage:localhost/custom_image:latest";

        // Build the container image from the Containerfile
        Self::run_command(
            &[
                "podman",
                "build",
                "-t",
                "localhost/custom_image:latest",
                "-f",
                containerfile,
            ],
            false,
        )
        .context("Failed to build container image from Containerfile")?;

        Self::install_rootfs_from_container(container_source, rt_config)
            .context("Failed to install rootfs from built container")
    }

    /// Create `/mnt/input` and `/mnt/output` directories in the VM, which will be used as input and
    /// output context mountpoints.
    fn create_mount_dirs() -> Result<()> {
        let mnt = env::temp_dir().join("mnt").join("mnt");
        for path in [mnt.join("input"), mnt.join("output")] {
            if !path.exists() {
                Self::run_command(&["mkdir", "-p", &format!("{}", path.display())], true)
                    .context(format!("Failed to create {} directory", path.display()))?;
            }
        }
        Ok(())
    }

    // Install the Linux kernel
    fn install_kernel(
        version: &str,
        kernel_url: &str,
        nvidia_drivers: bool,
        kernel_modules: &mut Vec<String>,
    ) -> Result<()> {
        let home_dir = std::env::var("HOME").context("Failed to get HOME environment variable")?;
        let kernel_dir = format!("{}/.linux-builds/{}", home_dir, version);
        let bzimage_path = format!("{}/arch/x86/boot/bzImage", kernel_dir);

        // Check if the bzImage already exists
        if Path::new(&bzimage_path).exists() {
            // println!("Kernel bzImage already exists, skipping build");
        } else {
            // Clone the specific version from the remote repository

            // Check if the kernel directory already exists
            if Path::new(&kernel_dir).exists() {
                // If it exists, do a git pull
                debug!("Kernel directory already exists");
            } else {
                // If it doesn't exist, clone the repository
                let clone_args = if version == "latest" {
                    vec!["git", "clone", "--depth", "1", kernel_url, &kernel_dir]
                } else {
                    vec![
                        "git",
                        "clone",
                        "--depth",
                        "1",
                        "--branch",
                        version,
                        kernel_url,
                        &kernel_dir,
                    ]
                };
                Self::run_command(&clone_args, false)
                    .context("Failed to clone kernel repository")?;
            }

            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            std::env::set_current_dir(&kernel_dir)
                .context("Failed to change to kernel directory")?;

            // Configure the kernel
            Self::run_command(&["make", "x86_64_defconfig"], false)
                .context("Failed to configure kernel")?;

            // SQUASHFS support
            Self::run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS"], false)
                .context("Failed to enable CONFIG_SQUASHFS flag to kernel config ")?;

            Self::run_command(
                &["scripts/config", "--disable", "CONFIG_SQUASHFS_FILE_CACHE"],
                false,
            )
            .context("Failed to disable CONFIG_SQUASHFS_FILE_CACHE flag to kernel config ")?;

            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_FILE_DIRECT"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_FILE_DIRECT flag to kernel config")?;

            Self::run_command(
                &[
                    "scripts/config",
                    "--enable",
                    "CONFIG_SQUASHFS_DECOMP_SINGLE",
                ],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_DECOMP_SINGLE flag to kernel config")?;

            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_DECOMP_MULTI"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_DECOMP_MULTI flag to kernel config")?;

            Self::run_command(
                &[
                    "scripts/config",
                    "--enable",
                    "CONFIG_SQUASHFS_DECOMP_MULTI_PERCPU",
                ],
                false,
            )
            .context(
                "Failed to enable CONFIG_SQUASHFS_DECOMP_MULTI_PERCPU flag to kernel config",
            )?;

            Self::run_command(
                &[
                    "scripts/config",
                    "--enable",
                    "CONFIG_SQUASHFS_CHOICE_DECOMP_BY_MOUNT",
                ],
                false,
            )
            .context(
                "Failed to enable CONFIG_SQUASHFS_CHOICE_DECOMP_BY_MOUNT flag to kernel config",
            )?;
            Self::run_command(
                &[
                    "scripts/config",
                    "--enable",
                    "CONFIG_SQUASHFS_MOUNT_DECOMP_THREADS",
                ],
                false,
            )
            .context(
                "Failed to enable CONFIG_SQUASHFS_MOUNT_DECOMP_THREADS flag to kernel config",
            )?;
            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_XATTR"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_XATTR flag to kernel config")?;
            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_ZLIB"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_ZLIB flag to kernel config")?;
            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_LZ4"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_LZ4 flag to kernel config")?;
            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_LZO"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_LZO flag to kernel config")?;
            Self::run_command(&["scripts/config", "--enable", "CONFIG_SQUASHFS_XZ"], false)
                .context("Failed to enable CONFIG_SQUASHFS_XZ flag to kernel config")?;
            Self::run_command(
                &["scripts/config", "--enable", "CONFIG_SQUASHFS_ZSTD"],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_ZSTD flag to kernel config")?;
            Self::run_command(
                &[
                    "scripts/config",
                    "--enable",
                    "CONFIG_SQUASHFS_4K_DEVBLK_SIZE",
                ],
                false,
            )
            .context("Failed to enable CONFIG_SQUASHFS_4K_DEVBLK_SIZE flag to kernel config")?;
            Self::run_command(
                &[
                    "scripts/config",
                    "--set-val",
                    "3",
                    "CONFIG_SQUASHFS_FRAGMENT_CACHE_SIZE",
                ],
                false,
            )
            .context("Failed to set CONFIG_SQUASHFS_FRAGMENT_CACHE_SIZE value to kernel config")?;
            Self::run_command(
                &["scripts/config", "--disable", "CONFIG_SQUASHFS_EMBEDDED"],
                false,
            )
            .context("Failed to disable CONFIG_SQUASHFS_EMBEDDED flag to kernel config ")?;

            // Build the kernel
            Self::run_command(&["make", &format!("-j{}", num_cpus::get())], false)
                .context("Failed to build kernel")?;

            std::env::set_current_dir(current_dir)
                .context("Failed to change back to original directory")?;
        }

        // Copy the built kernel to the boot partition
        Self::run_command(
            &[
                "cp",
                &bzimage_path,
                env::temp_dir().join("mnt").join("boot").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to copy kernel to boot partition")?;

        if nvidia_drivers {
            let kernel_source_dir = std::path::PathBuf::from(&kernel_dir);
            let vm_root_path = env::temp_dir().join("mnt");
            nvidia::install_drivers(kernel_source_dir, vm_root_path)
                .context("Unable to install NVIDIA drivers")?;

            kernel_modules.push("nvidia".to_string());
            kernel_modules.push("nvidia_uvm".to_string());
            // TODO: just hard-coded module names for now
        }

        Ok(())
    }

    // Install a precompiled kernel
    fn install_precompiled_kernel(kernel_path: &str) -> Result<()> {
        Self::run_command(
            &[
                "cp",
                kernel_path,
                env::temp_dir()
                    .join("mnt")
                    .join("boot")
                    .join("bzImage")
                    .to_str()
                    .unwrap(),
            ],
            true,
        )
        .context("Failed to copy precompiled kernel")
    }

    /// Prepare MIA installation config and run installer.
    fn install_mia(
        mia_version: &str,
        container_rt_config: &RuntimeConfig,
        kernel_modules: &Vec<String>,
        mounts: &Vec<String>,
        gevulot_runtime: bool,
        default_mounts: bool,
    ) -> Result<()> {
        let mut mounts = mounts
            .iter()
            .map(|m| {
                let parts: Vec<&str> = m.split(':').collect();
                let source = parts.first().unwrap().to_string();
                let target = parts.get(1).unwrap_or(&"").to_string();
                let fstype = parts.get(2).unwrap_or(&"9p").to_string();
                let data = parts
                    .get(3)
                    .unwrap_or(&"trans=virtio,version=9p2000.L")
                    .to_string();
                runtime_config::Mount {
                    source,
                    target,
                    fstype: Some(fstype),
                    flags: None,
                    data: Some(data),
                }
            })
            .collect::<Vec<_>>();

        let follow_config = if gevulot_runtime {
            let gevulot_mnt_dir = env::temp_dir().join("mnt").join("mnt").join("gevulot");
            for dirname in ["rt-config", "input", "output"] {
                let dirpath = gevulot_mnt_dir.join(dirname);
                Self::run_command(&["mkdir", "-p", dirpath.to_str().unwrap()], true)
                    .context(format!("Failed to create {} directory", dirpath.display()))?;
            }

            // NOTE: Worker node will mount input and output contexts to these tags.
            mounts.push(runtime_config::Mount::virtio9p(
                "gevulot-input".to_string(),
                "/mnt/gevulot/input".to_string(),
            ));
            mounts.push(runtime_config::Mount::virtio9p(
                "gevulot-output".to_string(),
                "/mnt/gevulot/output".to_string(),
            ));

            mounts.push(runtime_config::Mount::virtio9p(
                "gevulot-rt-config".to_string(),
                "/mnt/gevulot/rt-config".to_string(),
            ));
            // NOTE: Worker node will mount runtime config file to tag `gevulot-rt-config`.
            //       This is a convention between VM and node we have now.
            Some("/mnt/gevulot/rt-config/config.yaml".to_string())
        } else {
            None
        };

        let rt_config = RuntimeConfig {
            version: runtime_config::VERSION.to_string(),
            command: container_rt_config.command.clone(),
            args: container_rt_config.args.clone(),
            env: container_rt_config.env.clone(),
            working_dir: container_rt_config.working_dir.clone(),
            mounts,
            default_mounts,
            kernel_modules: kernel_modules.clone(),
            follow_config,
            ..Default::default()
        };

        let mut install_config = mia_installer::InstallConfig::default();
        install_config.mia_version = mia_version.to_string();
        install_config.mia_platform = "x86_64-unknown-linux-gnu".to_string();
        install_config.prefix = env::temp_dir().join("mnt");
        install_config.as_root = true;

        // In case there is an init system installed in the container
        install_config.overwrite_symlink = true;

        install_config.rt_config = Some(rt_config);

        mia_installer::install(&install_config)
    }

    // Install the bootloader (SYSLINUX)
    fn install_bootloader(
        init: Option<&str>,
        init_args: Option<&str>,
        output_file: &str,
        rw_root: bool,
        mbr_file: Option<&str>,
    ) -> Result<()> {
        let init = if let Some(init) = init {
            format!(" init={}", init)
        } else {
            "".to_string()
        };

        let init_args = if let Some(init_args) = init_args {
            format!(" -- {}", init_args)
        } else {
            "".to_string()
        };

        let root_dev_mode = if rw_root { "rw" } else { "ro" };

        // Create SYSLINUX configuration
        let syslinux_cfg = format!(
            r#"DEFAULT linux
PROMPT 0
TIMEOUT 50

LABEL linux
    LINUX /bzImage
    APPEND root=/dev/sda2 {} console=ttyS0{}{}
"#,
            root_dev_mode, init, init_args
        );

        // Write SYSLINUX configuration to file
        let mut child = Command::new("sudo")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .args([
                "tee",
                env::temp_dir()
                    .join("mnt")
                    .join("boot")
                    .join("syslinux.cfg")
                    .to_str()
                    .unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn tee command for SYSLINUX configuration")?;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(syslinux_cfg.as_bytes())
            .context("Failed to write SYSLINUX configuration")?;
        child.wait().context("Failed to wait for tee command")?;

        // Install SYSLINUX
        Self::run_command(
            &[
                "extlinux",
                "--install",
                env::temp_dir().join("mnt").join("boot").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to install SYSLINUX")?;

        // Install MBR
        let mbr_path = if let Some(mbr_file) = mbr_file {
            mbr_file
        } else {
            const CANDIDATES: [&str; 3] = [
                "/usr/share/syslinux/mbr.bin",
                "/usr/lib/syslinux/mbr/mbr.bin",
                "/usr/lib/syslinux/bios/mbr.bin",
            ];
            if let Some(path) = CANDIDATES
                .into_iter()
                .find(|candidate| std::path::Path::new(candidate).exists())
            {
                path
            } else {
                anyhow::bail!("MBR file was not found. Use --mbr-file option to specify it.");
            }
        };
        let loop_device = Self::get_loop_device(output_file)?;
        Self::run_command(
            &[
                "dd",
                "bs=440",
                "count=1",
                "conv=notrunc",
                &format!("if={}", mbr_path),
                &format!("of={}", loop_device),
            ],
            true,
        )
        .context("Failed to install MBR")?;

        // Ensure all changes are written to disk
        Self::run_command(&["sync"], true).context("Failed to sync filesystem")?;

        Ok(())
    }

    // Clean up: unmount filesystems and detach loop device
    fn cleanup() -> Result<()> {
        _ = Self::run_command(
            &[
                "umount",
                "-R",
                env::temp_dir().join("mnt").to_str().unwrap(),
            ],
            true,
        );
        // Remove pending container if any.
        _ = Self::remove_temporary_container();
        // Detach all unused loop devices
        _ = Self::run_command(&["losetup", "-D"], true);
        log::debug!(
            "removing temp dir {}",
            env::temp_dir().join("mnt").display()
        );
        _ = fs::remove_dir(env::temp_dir().join("mnt"));
        log::debug!("removed temp dir {}", env::temp_dir().join("mnt").display());
        Ok(())
    }

    // Helper function to get the current loop device
    fn get_loop_device(output_file: &str) -> Result<String> {
        let loop_device = String::from_utf8(
            Command::new("sh")
                .arg("-c")
                .arg(&format!(
                    "losetup -a | grep {} | awk -F: '{{print $1}}' | head -1",
                    output_file
                ))
                .output()
                .context("Failed to execute losetup command")?
                .stdout,
        )
        .context("Failed to parse losetup output")?
        .trim()
        .to_string();
        Ok(loop_device)
    }

    pub fn run_command(commands: &[&str], as_root: bool) -> Result<()> {
        let program = if as_root { "sudo" } else { commands[0] };
        let args = if as_root { commands } else { &commands[1..] };

        log::debug!("running command: {program} {:?}", args);

        let mut child = Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn command")?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Could not capture stdout."))?;

        let reader = BufReader::new(stdout);
        reader
            .lines()
            .filter_map(|line| line.ok())
            .for_each(|line| debug!(target: commands[0], "{}", line));

        let output = child
            .wait_with_output()
            .context("Failed to wait for command")?;
        if output.status.success() {
            Ok(())
        } else {
            String::from_utf8(output.stderr)
                .context("Failed to parse command stderr")?
                .lines()
                .for_each(|line| debug!(target: commands[0], "{}", line));
            Err(anyhow::anyhow!(
                "Command failed with status {}",
                output.status
            ))
        }
    }

    fn remove_temporary_container() -> Result<()> {
        Self::run_command(&["podman", "rm", TEMP_CONTAINER_NAME], false)
    }
}
