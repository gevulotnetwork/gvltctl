use anyhow::{Context, Result};
use log::debug;
use oci_spec::image::{ImageConfiguration, ImageManifest};
use std::io::{self, BufRead, BufReader, Write};
use std::{env, fs, path::Path, process::Command};
use tempdir::TempDir;

use crate::builders::{BuildOptions, ImageBuilder};

use super::nvidia;

/// `mia` binary.
///
/// Bytes are included directly here during compilation.
// TODO: probably this is not the best idea, because now user can't use different
// version of mia without recompiling the gvltctl.
const THIN_INIT_BIN: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/x86_64-unknown-linux-gnu/release/mia"
));

/// `kmod` statically linked executable and compiled for x86_64.
///
/// It is used by MIA to operate on kernel modules.
const KMOD_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/builders/data/kmod");

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

            let mut init_args = options.init_args.clone();

            if let Some(container_source) = &options.container_source {
                print(&format!("Installing rootfs from container... "))?;
                Self::install_rootfs_from_container(container_source, &mut init_args)?;
                print(&format!("✅\n"))?;
            } else if let Some(rootfs_dir) = &options.rootfs_dir {
                print(&format!("Installing rootfs from directory... "))?;
                Self::install_rootfs_from_directory(rootfs_dir)?;
                print(&format!("✅\n"))?;
            } else if let Some(containerfile) = &options.containerfile {
                print(&format!(
                    "Building and installing rootfs from Containerfile... "
                ))?;
                Self::build_and_install_rootfs_from_containerfile(containerfile, &mut init_args)?;
                print(&format!("✅\n"))?;
            }

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
                )?;
                print(&format!("✅\n"))?;
            }

            // Without explicit init, mia will be used.
            if options.init.is_none() {
                print(&format!("Installing MIA (Minimal Init Application)... "))?;
                Self::install_mia(
                    &mut init_args,
                    &options.kernel_modules,
                    &options.mounts,
                    options.no_default_mounts,
                )?;
                print(&format!("✅\n"))?;
            }

            print(&format!("Installing bootloader... "))?;
            Self::install_bootloader(
                options.init.as_deref(),
                init_args.as_deref(),
                &options.output_file,
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
        init_args: &mut Option<String>,
    ) -> Result<()> {
        // This temp dir will be removed on dropping.
        let target_dir = TempDir::new("").context("Failed to create temporary directory")?;

        // Copy the container image to a directory
        Self::run_command(
            &[
                "skopeo",
                "copy",
                container_source,
                &format!("dir:{}", target_dir.path().display()),
            ],
            false,
        )
        .context("Failed to copy container image")?;

        // Read image manifest
        let manifest = ImageManifest::from_file(target_dir.path().join("manifest.json"))
            .context("Failed to read image manifest")?;

        // Extract all layers of image into target dir
        for layer in manifest.layers() {
            let layer_path = target_dir.path().join(layer.digest().digest());
            log::debug!(
                "unpack layer {} from {}",
                layer.digest(),
                layer_path.display()
            );
            // Unpack with root permissions
            match Self::run_command(
                &[
                    "tar",
                    "-xf",
                    layer_path.to_str().unwrap(),
                    "-C",
                    target_dir.path().to_str().unwrap(),
                ],
                true,
            ) {
                Ok(_) => {
                    log::debug!("remove layer {}", layer_path.display());
                    fs::remove_file(&layer_path).context("Failed to remove layer file")?;
                    log::debug!("removed layer {}", layer_path.display());
                }
                Err(_) => {
                    log::warn!("Failed to unpack layer"); // TODO: Investigate why this happens.
                }
            };
        }

        log::debug!("unpacked all layers");

        let config_path = target_dir.path().join(manifest.config().digest().digest());
        let config = ImageConfiguration::from_file(&config_path)
            .context("Failed to read image configuration")?;
        log::debug!("unpacked config {}", config_path.display());
        fs::remove_file(&config_path).context("Failed to remove config file")?;
        log::debug!("removed config {}", config_path.display());

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

        // Extract init_args from the container manifest if the user didn't provide it.
        if init_args.is_none() {
            *init_args = Some(String::new());
            let init_args = init_args.as_mut().unwrap();
            if let Some(exec_params) = config.config() {
                // Add enviromnental variables
                if let Some(env_vars) = exec_params.env() {
                    for var in env_vars {
                        init_args.push_str(&format!(" --env {}", &var));
                    }
                }

                if let Some(working_dir) = exec_params.working_dir() {
                    init_args.push_str(&format!(" --wd {}", working_dir));
                }

                // Try to get the ENTRYPOINT execution params
                if let Some(entrypoint) = exec_params.entrypoint() {
                    if !entrypoint.is_empty() {
                        let entrypoint_str = entrypoint.join(" ");
                        init_args.push(' ');
                        init_args.push('"');
                        init_args.push_str(&entrypoint_str);
                        init_args.push('"');
                    }
                }
                // Try to get CMD from execution params
                if let Some(cmd) = exec_params.cmd() {
                    for arg in cmd {
                        if !arg.is_empty() {
                            init_args.push(' ');
                            init_args.push('"');
                            init_args.push_str(arg);
                            init_args.push('"');
                        }
                    }
                }
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
        init_args: &mut Option<String>,
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

        Self::install_rootfs_from_container(container_source, init_args)
            .context("Failed to install rootfs from built container")
    }

    // Install the Linux kernel
    fn install_kernel(version: &str, kernel_url: &str, nvidia_drivers: bool) -> Result<()> {
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

            // Configure and build the kernel
            Self::run_command(&["make", "x86_64_defconfig"], false)
                .context("Failed to configure kernel")?;
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

    fn install_init(basepath: impl AsRef<Path>, name: &str, binary: &[u8]) -> Result<()> {
        // Install init binary into root directory
        if basepath.as_ref().has_root() {
            anyhow::bail!("Failed to install init: absolute basepath was provided");
        }
        let init_path = env::temp_dir().join("mnt").join(&basepath).join(name);

        // Write init binary to a file
        let mut child = Command::new("sudo")
            .args(["tee", init_path.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .context("Failed to spawn tee command for init installation")?;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(binary)
            .context("Failed to write init binary")?;
        child.wait().context("Failed to wait for tee command")?;

        // Give execution permissions for init file
        Self::run_command(&["chmod", "755", init_path.to_str().unwrap()], true)
            .context("Failed to set execution permissions for init")?;

        // Create /sbin directory if it doesn't exist
        Self::run_command(
            &[
                "mkdir",
                "-p",
                env::temp_dir().join("mnt").join("sbin").to_str().unwrap(),
            ],
            true,
        )
        .context("Failed to create /sbin directory")?;

        // Create symlink to allow default init path: /sbin/init
        Self::run_command(
            &[
                "ln",
                "-s",
                "-f",
                Path::new("/").join(&basepath).join(name).to_str().unwrap(),
                env::temp_dir()
                    .join("mnt")
                    .join("sbin")
                    .join("init")
                    .to_str()
                    .unwrap(),
            ],
            true,
        )
        .context("Failed to create symlink for init")?;

        // Ensure all changes are written to disk
        Self::run_command(&["sync"], true).context("Failed to sync filesystem")?;

        Ok(())
    }

    fn install_mia(
        init_args: &mut Option<String>,
        kernel_modules: &Vec<String>,
        mounts: &Vec<String>,
        no_default_mounts: bool,
    ) -> Result<()> {
        // Create directory in /usr/lib for mia
        let mia_dir = env::temp_dir()
            .join("mnt")
            .join("usr")
            .join("lib")
            .join("mia");
        Self::run_command(&["mkdir", "-p", mia_dir.to_str().unwrap()], true)
            .context("Failed to create mia directory")?;

        // MIA will use kmod to operate on kernel modules
        Self::install_kmod(&mia_dir)?;

        let mut mounts = mounts.clone();
        if !no_default_mounts {
            mounts.insert(0, "proc:/proc:proc:".to_string());
            // Ensure that /proc exists, otherwise there will be an error at runtime
            Self::run_command(
                &[
                    "mkdir",
                    "-p",
                    env::temp_dir().join("mnt").join("proc").to_str().unwrap(),
                ],
                true,
            )
            .context("Failed to create /proc directory")?;
        }
        let modules_args = kernel_modules
            .iter()
            .map(|module| format!("--module {}", module))
            .collect::<Vec<_>>()
            .join(" ");
        let mount_args = mounts
            .iter()
            .map(|mount| format!("--mount {}", mount))
            .collect::<Vec<_>>()
            .join(" ");
        if !mount_args.is_empty() {
            *init_args = Some(init_args.as_ref().map_or(mount_args.clone(), |args| {
                format!("{} {}", mount_args, args)
            }));
        }
        if !modules_args.is_empty() {
            *init_args = Some(init_args.as_ref().map_or(mount_args.clone(), |args| {
                format!("{} {}", modules_args, args)
            }));
        }
        Self::install_init("usr/lib/mia", "mia", THIN_INIT_BIN).context("Failed to install MIA")
    }

    /// Install kmod and its tools.
    fn install_kmod(mia_dir: &Path) -> Result<()> {
        if !Path::new(KMOD_FILE).exists() {
            anyhow::bail!("kmod was not found. Expected: {}", KMOD_FILE);
        }

        // Install kmod binary
        Self::run_command(&["cp", KMOD_FILE, mia_dir.to_str().unwrap()], true)
            .context("Failed to install kmod")?;
        Self::run_command(
            &["chmod", "755", mia_dir.join("kmod").to_str().unwrap()],
            true,
        )
        .context("Failed to set kmod mode")?;

        // Install symlinks to kmod
        let symlinks = ["depmod", "insmod", "lsmod", "modinfo", "modprobe", "rmmod"];
        for symlink in symlinks {
            Self::run_command(
                &[
                    "ln",
                    "-s",
                    "/usr/lib/mia/kmod",
                    mia_dir.join(symlink).to_str().unwrap(),
                ],
                true,
            )
            .context(format!("Failed to install {}", symlink))?;
        }

        Ok(())
    }

    // Install the bootloader (SYSLINUX)
    fn install_bootloader(
        init: Option<&str>,
        init_args: Option<&str>,
        output_file: &str,
        mbr_file: Option<&str>,
    ) -> Result<()> {
        let init = if let Some(init) = init {
            format!(" init={}", init)
        } else {
            "".to_string()
        };

        let init_args = if let Some(init_args) = init_args {
            format!("-- {}", init_args)
        } else {
            "".to_string()
        };

        // Create SYSLINUX configuration
        let syslinux_cfg = format!(
            r#"DEFAULT linux
PROMPT 0
TIMEOUT 50

LABEL linux
    LINUX /bzImage
    APPEND root=/dev/sda2 rw console=ttyS0{} {}
"#,
            init, init_args
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
            const CANDIDATES: [&str; 2] = [
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
}
