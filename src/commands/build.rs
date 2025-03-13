use clap::{ValueEnum, ValueHint};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;

use crate::builders::linux_vm;
use crate::{print_object, OutputFormat};

/// Build command.
#[derive(Clone, Debug, clap::Parser)]
pub struct BuildArgs {
    /// Image to use as a source for VM filesystem.
    #[command(flatten)]
    pub image: Image,

    /// Container backend to use.
    #[arg(long, default_value_t)]
    pub container_backend: ContainerBackend,

    /// Size of the disk image (e.g., 10G, 1024M).
    ///
    /// This determines the total capacity of the VM's virtual disk.
    #[arg(long = "size", short = 's', value_name = "SIZE")]
    pub image_size: Option<String>,

    /// Linux kernel version to use (e.g., v6.10).
    ///
    /// Use 'latest' for the most recent version. This kernel will be compiled from source.
    #[arg(
        long = "kernel",
        short = 'k',
        value_name = "VERSION",
        default_value = "v6.12"
    )]
    pub kernel_version: String,

    /// URL of the Linux kernel repository to clone.
    ///
    /// Change this if you want to use a fork or mirror.
    #[arg(
        long,
        value_name = "URL",
        value_hint = ValueHint::Url,
        default_value = "https://github.com/torvalds/linux.git"
    )]
    pub kernel_url: String,

    /// Path to a precompiled kernel file.
    ///
    /// Use this if you have a custom kernel or want to skip kernel compilation.
    /// Example: /path/to/bzImage
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub kernel_file: Option<String>,

    /// Enables building NVIDIA drivers and including them in the VM image.
    #[arg(long)]
    pub nvidia_drivers: bool,

    /// [MIA] Load kernel module. Can be passed multiple times.
    ///
    /// MODULENAME will be passed to modprobe.
    /// This option can't be used together with --init or --init-args.
    #[arg(
        long = "kernel-module",
        value_name = "MODULENAME",
        conflicts_with_all = ["init", "init_args"],
    )]
    pub kernel_modules: Vec<String>,

    /// Mount directory on startup. Can be passed multiple times.
    ///
    /// Example: input:/mnt/input
    ///
    /// These options are passed to MIA to mount before running any commands. Arguments are
    /// corresponding to mount syscall. If no <fstype> is specified, MIA will use 9p by default.
    ///
    /// MIA will mount /proc by default. If you don't want this, use --no-default-mounts.
    ///
    /// This option can't be used together with --init or --init-args.
    #[arg(
        long = "mount",
        value_name = "source:target|source:target:fstype:options",
        conflicts_with_all = ["init", "init_args"]
    )]
    pub mounts: Vec<String>,

    /// [MIA] Install specified MIA version.
    ///
    /// Accepted format is from mia-installer.
    /// Examples:
    /// - latest
    /// - 0.1.0
    /// - file:/path/to/mia/binary
    ///
    /// This option can't be used together with --init or --init-args.
    #[arg(
        long,
        value_name = "STRING",
        default_value = "latest",
        conflicts_with_all = ["init", "init_args"],
        verbatim_doc_comment
    )]
    pub mia_version: String,

    /// [MIA] Don't install Gevulot runtime. Only for debug purposes.
    ///
    /// No following config will be provided to the VM. Only built-in one will be used.
    /// No input/output context directories will be mounted.
    ///
    /// *Note:* Gevulot worker will provide runtime config through gevulot-rt-config.
    /// This means that images with this flag enabled cannot be executed on the network.
    ///
    /// This option can't be used together with --init or --init-args.
    #[arg(hide = true, long, conflicts_with_all = ["init", "init_args"])]
    pub no_gevulot_runtime: bool,

    /// [MIA] Don't mount /proc.
    ///
    /// This option can't be used together with --init or --init-args.
    #[arg(long, conflicts_with_all = ["init", "init_args"])]
    pub no_default_mounts: bool,

    /// Init process to use (e.g., /sbin/init, /lib/systemd/systemd).
    ///
    /// This is the first process started by the kernel.
    #[arg(long, short = 'i', value_name = "INIT", value_hint = ValueHint::FilePath)]
    pub init: Option<String>,

    /// Arguments to pass to the init program.
    ///
    /// Example: '--debug --option=value'
    #[arg(long, value_name = "ARGS", allow_hyphen_values = true)]
    pub init_args: Option<String>,

    /// Mount root filesystem as read-write. Only for debug purposes.
    ///
    /// Root filesystem will be mounted as read-only by default.
    ///
    /// *Note:* Gevulot worker node will execute your disk image in read-only mode.
    /// This means that images with this flag enabled cannot be executed on the network.
    #[arg(hide = true, long)]
    pub rw_root: bool,

    /// Path to MBR file.
    ///
    /// If none provided, a number of default locations will be tried.
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    pub mbr_file: Option<PathBuf>,

    /// Name of the output disk image file.
    ///
    /// This will be a bootable disk image you can use with QEMU or other VM software.
    #[arg(
        long = "output",
        short,
        value_name = "FILE",
        value_hint = ValueHint::FilePath,
        default_value = "disk.img"
    )]
    pub output_file: PathBuf,

    /// Root filesystem type.
    #[arg(long, default_value_t)]
    pub root_fs_type: RootFsType,

    /// Use FUSE to mount target image.
    ///
    /// Use native OS mounts instead. Requires root privileges.
    #[arg(long)]
    pub fuse: bool,

    /// Build VM image from scratch with new filesystem and bootloader.
    ///
    /// By default pre-built VM image with EXT4 filesystem and EXTLINUX bootloader will be used.
    /// During build process filesystem will be expanded to required size.
    /// If this option is set, completely fresh VM image will be created.
    /// Additional dependencies are required: extlinux.
    /// This option implies --fuse disabled.
    #[arg(long)]
    pub from_scratch: bool,

    /// Cache directory.
    ///
    /// Usually defaults to:
    /// - '$HOME/.cache/gvltctl' on Linux
    /// - '$HOME/Library/Caches/gevulot.gvltctl' on MacOS
    #[arg(long, value_name = "DIR", value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    pub cache_dir: Option<PathBuf>,

    /// Generate only base VM image.
    ///
    /// If this option is enabled, only base VM image will be generated.
    /// Base image includes bootloader, partition table and filesystem.
    /// Image created with this command is used by default when building VM image from container.
    /// This option implies --fuse disabled.
    #[arg(hide = true, long)]
    pub generate_base_image: bool,

    /// Force the build and try to fix known problems along the way.
    ///
    /// This will overwrite existing files and attempt to clean up previous build artifacts.
    #[arg(long)]
    pub force: bool,

    /// Do not print any messages.
    #[arg(long, short)]
    pub quiet: bool,
}

#[derive(Clone, Debug, clap::Args)]
#[group(required = true)]
pub struct Image {
    /// Container image to use as the source.
    ///
    /// Supports various transport methods:
    /// - docker: Docker registry (e.g., docker://docker.io/debian:latest)
    /// - containers-storage: Local container storage (e.g., containers-storage:localhost/myimage:latest)
    /// - dir: Local directory (e.g., dir:/path/to/image)
    /// - oci: OCI image layout (e.g., oci:/path/to/layout)
    /// - docker-archive: Docker archive (e.g., docker-archive:/path/to/archive.tar)
    ///
    /// Examples:
    /// - docker://docker.io/ubuntu:20.04
    /// - containers-storage:localhost/custom-image:latest
    #[arg(long, short = 'c', value_name = "IMAGE", verbatim_doc_comment)]
    pub container: Option<String>,

    /// Directory containing the root filesystem to use.
    ///
    /// This should be a fully prepared root filesystem, typically extracted from a container or
    /// created manually.
    #[arg(long, value_name = "DIR", value_hint = ValueHint::FilePath)]
    pub rootfs_dir: Option<PathBuf>,

    /// Path to a Containerfile (Dockerfile) to build the container image.
    ///
    /// The file will be used to build a new image which will then be used as the source.
    #[arg(long, short = 'f', value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub containerfile: Option<PathBuf>,
}

/// Container backend (docker or podman).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum ContainerBackend {
    Podman,
    Docker,
}

impl Default for ContainerBackend {
    fn default() -> Self {
        Self::Podman
    }
}

impl fmt::Display for ContainerBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.to_possible_value()
                .expect("no skipped values")
                .get_name()
        )
    }
}

/// Root filesystem type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum RootFsType {
    SquashFs,
    Ext4,
}

impl Default for RootFsType {
    fn default() -> Self {
        Self::SquashFs
    }
}

impl fmt::Display for RootFsType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.to_possible_value()
                .expect("no skipped values")
                .get_name()
        )
    }
}

impl BuildArgs {
    /// Run build subcommand.
    pub async fn run(&self, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        let value = build(self).await?;
        print_object(format, &value)
    }
}

impl TryFrom<&BuildArgs> for linux_vm::LinuxVMBuildContext {
    type Error = anyhow::Error;

    fn try_from(opts: &BuildArgs) -> Result<Self, Self::Error> {
        use anyhow::{anyhow, bail};
        use bytesize::ByteSize;
        use linux_vm::{
            FilesystemSource, ImageFileOpts, InitSystemOpts, KernelOpts, MountType, RootFsOpts,
        };

        let image_file_opts = ImageFileOpts {
            path: PathBuf::from(&opts.output_file),
            size: opts
                .image_size
                .as_ref()
                .map(|size| {
                    size.parse::<ByteSize>()
                        .as_ref()
                        .map(ByteSize::as_u64)
                        .map_err(|_| anyhow!("invalid image size"))
                })
                .transpose()?,
            force: opts.force,
        };

        let kernel_opts = if let Some(file) = &opts.kernel_file {
            KernelOpts::Precompiled {
                file: PathBuf::from(file),
            }
        } else {
            KernelOpts::Source {
                version: opts.kernel_version.clone(),
                repository_url: opts.kernel_url.clone(),
            }
        };

        let nvidia_drivers = opts.nvidia_drivers;

        let init_system_opts = if let Some(init) = &opts.init {
            InitSystemOpts::Custom {
                init: init.clone(),
                init_args: opts.init_args.clone(),
            }
        } else {
            InitSystemOpts::Mia {
                mia_version: opts.mia_version.clone(),
                mounts: opts.mounts.clone(),
                default_mounts: !opts.no_default_mounts,
                kernel_modules: opts.kernel_modules.clone(),
                gevulot_runtime: !opts.no_gevulot_runtime,
            }
        };

        let fs_source = if let Some(path) = &opts.image.rootfs_dir {
            FilesystemSource::Dir(PathBuf::from(path))
        } else if let Some(reference) = &opts.image.container {
            FilesystemSource::Image {
                reference: reference.clone(),
                backend: match opts.container_backend {
                    ContainerBackend::Podman => linux_vm::ContainerBackend::Podman,
                    ContainerBackend::Docker => linux_vm::ContainerBackend::Docker,
                },
            }
        } else if let Some(path) = &opts.image.containerfile {
            FilesystemSource::Containerfile {
                file: PathBuf::from(path),
                backend: match opts.container_backend {
                    ContainerBackend::Podman => linux_vm::ContainerBackend::Podman,
                    ContainerBackend::Docker => linux_vm::ContainerBackend::Docker,
                },
            }
        } else {
            bail!("no source was specified");
        };

        let root_fs_opts = match &opts.root_fs_type {
            RootFsType::SquashFs => RootFsOpts::SquashFs,
            RootFsType::Ext4 => {
                let mount_type = if opts.fuse && !opts.from_scratch {
                    MountType::Fuse
                } else {
                    MountType::Native
                };
                RootFsOpts::Ext4 { mount_type }
            }
        };

        let cache_dir = opts.cache_dir.clone();

        let gen_base_img = opts.generate_base_image;
        let from_scratch = gen_base_img || opts.from_scratch;
        let mbr_file = opts.mbr_file.clone();
        let rw_root = opts.rw_root;

        let opts = linux_vm::BuildOpts {
            image_file_opts,
            kernel_opts,
            nvidia_drivers,
            init_system_opts,
            fs_source,
            from_scratch,
            root_fs_opts,
            mbr_file,
            cache_dir,
            rw_root,
            gen_base_img,
        };

        Ok(Self::from_opts(opts)?)
    }
}

async fn build(build_args: &BuildArgs) -> Result<Value, Box<dyn std::error::Error>> {
    let mut build_context = linux_vm::LinuxVMBuildContext::try_from(build_args)?;
    linux_vm::build(&mut build_context)?;

    Ok(serde_json::json!({
        "message": format!("Created {}", build_args.output_file.display()),
        "image": &build_args.output_file,
    }))
}
