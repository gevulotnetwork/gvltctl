use anyhow::{Context, Result};
use clap::{Arg, ArgGroup, ArgMatches, ValueHint};
use std::path::PathBuf;

use gvltctl::builders::linux_vm::{self, LinuxVMBuildContext};

pub fn get_command() -> clap::Command {
    clap::Command::new("build")
        .about("Build a VM image from a container, rootfs directory, or Containerfile")
        .arg(
            Arg::new("container_source")
                .short('c')
                .long("container")
                .value_name("IMAGE")
                .help("Container image to use as the source. Supports various transport methods:\n\
                       - docker: Docker registry (e.g., docker://docker.io/debian:latest)\n\
                       - containers-storage: Local container storage (e.g., containers-storage:localhost/myimage:latest)\n\
                       - dir: Local directory (e.g., dir:/path/to/image)\n\
                       - oci: OCI image layout (e.g., oci:/path/to/layout)\n\
                       - docker-archive: Docker archive (e.g., docker-archive:/path/to/archive.tar)\n\
                       Examples:\n\
                       - docker://docker.io/ubuntu:20.04\n\
                       - containers-storage:localhost/custom-image:latest")
                .required(false)
        )
        .arg(
            Arg::new("rootfs_dir")
                .long("rootfs-dir")
                .value_name("DIR")
                .value_hint(ValueHint::FilePath)
                .help("Directory containing the root filesystem to use. This should be a fully \
                       prepared root filesystem, typically extracted from a container or created manually.")
                .required(false)
        )
        .arg(
            Arg::new("containerfile")
                .long("containerfile")
                .short('f')
                .value_name("FILE")
                .value_hint(ValueHint::FilePath)
                .help("Path to a Containerfile (Dockerfile) to build the container image. \
                       The file will be used to build a new image which will then be used as the source.")
                .required(false)
        )
        .group(
            ArgGroup::new("image")
                .args(["container_source", "rootfs_dir", "containerfile"])
                .multiple(false)
                .required(true)
        )
        .arg(
            Arg::new("image_size")
                .short('s')
                .long("size")
                .value_name("SIZE")
                .help("Size of the disk image (e.g., 10G, 1024M). This determines the total capacity of the VM's virtual disk.")
                .required(false)
                .default_value("10G"),
        )
        .arg(
            Arg::new("kernel_version")
                .short('k')
                .long("kernel")
                .value_name("VERSION")
                .help("Linux kernel version to use (e.g., v6.10). Use 'latest' for the most recent version. \
                       This kernel will be compiled from source.")
                .required(false)
                .default_value("latest"),
        )
        .arg(
            Arg::new("kernel_url")
                .long("kernel-url")
                .value_name("URL")
                .value_hint(ValueHint::Url)
                .help("URL of the Linux kernel repository to clone. Change this if you want to use a fork or mirror.")
                .required(false)
                .default_value("https://github.com/torvalds/linux.git"),
        )
        .arg(
            Arg::new("kernel_file")
                .long("kernel-file")
                .value_name("FILE")
                .value_hint(ValueHint::FilePath)
                .help("Path to a precompiled kernel file. Use this if you have a custom kernel or want to skip kernel compilation. \
                       Example: /path/to/bzImage")
                .required(false),
        )
        .arg(
            Arg::new("nvidia_drivers")
                .long("nvidia-drivers")
                .help("Enables building NVIDIA drivers and including them in the VM image.")
                .required(false)
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("kernel_module")
                .long("kernel-module")
                .value_name("MODULENAME")
                .help("[MIA] Load kernel module. \
                       MODULENAME will be passed to modprobe. \
                       This option can't be used together with --init or --init-args.")
                .action(clap::ArgAction::Append)
                .conflicts_with_all(["init", "init_args"])
                .required(false)
        )
        .arg(
            Arg::new("mount")
                .long("mount")
                .value_name("source:target|source:target:fstype:options")
                .help("[MIA] Mount directory on startup. Example: input:/mnt/input\n\
                       These options are passed to MIA to mount before running any commands. Arguments are corresponding\n\
                       to mount syscall. If no <fstype> is specified, MIA will use 9p by default.\n\
                       MIA will mount /proc by default. If you don't want this, use --no-default-mounts.\n\
                       This option can't be used together with --init or --init-args.")
                .action(clap::ArgAction::Append)
                .conflicts_with_all(["init", "init_args"])
                .required(false),
        )
        .arg(
            Arg::new("no_gevulot_runtime")
                .long("no-gevulot-runtime")
                .help("[MIA] Don't install Gevulot runtime. Only for debug purposes.")
                .help("[MIA] Don't install Gevulot runtime. Only for debug purposes.\n\
                       No following config will be provided to the VM. Only built-in one will be used.\n\
                       No input/output context directories will be mounted.\n\
                       Note: Gevulot worker will provide runtime config through gevulot-rt-config.\n\
                       This means that images with this flag enabled cannot be executed on the network.\n\
                       This option can't be used together with --init or --init-args.")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with_all(["init", "init_args"])
                .required(false),
        )
        .arg(
            Arg::new("no_default_mounts")
                .long("no-default-mounts")
                .help("[MIA] Don't mount /proc. \
                       This option can't be used together with --init or --init-args.")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with_all(["init", "init_args"])
                .required(false),
        )
        .arg(
            Arg::new("init")
                .short('i')
                .long("init")
                .value_name("INIT")
                .value_hint(ValueHint::FilePath)
                .help("Init process to use (e.g., /sbin/init, /lib/systemd/systemd). This is the first process started by the kernel.")
                .required(false),
        )
        .arg(
            Arg::new("init_args")
                .long("init-args")
                .value_name("ARGS")
                .help("Arguments to pass to the init program. Example: '--debug --option=value'")
                .allow_hyphen_values(true)
                .required(false),
        )
        .arg(
            Arg::new("rw_root")
                .long("rw-root")
                .help("Mount root filesystem as read-write. Only for debug purposes.")
                .long_help("Mount root filesystem as read-write. Only for debug purposes.\n\
                            Root filesystem will be mounted as read-only by default.\n\
                            Note: Gevulot worker node will execute your disk image in read-only mode.\n\
                            This means that images with this flag enabled cannot be executed on the network.")
                .required(false)
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("mbr_file")
                .long("mbr-file")
                .value_name("FILE")
                .value_hint(ValueHint::FilePath)
                .help("Path to MBR file. If none provided, following paths will be tried:\n\
                        - /usr/share/syslinux/mbr.bin\n\
                        - /usr/lib/syslinux/mbr/mbr.bin\n\
                        - /usr/lib/syslinux/bios/mbr.bin")
                .required(false),
        )
        .arg(
            Arg::new("output_file")
                .short('o')
                .long("output")
                .value_name("FILE")
                .value_hint(ValueHint::FilePath)
                .help("Name of the output disk image file. This will be a bootable disk image you can use with QEMU or other VM software.")
                .required(false)
                .default_value("disk.img"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .help("Force the build and try to fix known problems along the way. This will overwrite existing files and attempt to clean up previous build artifacts.")
                .required(false)
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help("Do not print any messages.")
                .required(false)
                .action(clap::ArgAction::SetTrue),
        )
}

/// CLI options for Linux VM builder.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub container_source: Option<String>,
    pub rootfs_dir: Option<String>,
    pub containerfile: Option<String>,
    pub image_size: String,
    pub kernel_version: String,
    pub kernel_url: String,
    pub kernel_file: Option<String>,
    pub nvidia_drivers: bool,
    pub kernel_modules: Vec<String>,
    pub mounts: Vec<String>,
    pub no_gevulot_runtime: bool,
    pub no_default_mounts: bool,
    pub init: Option<String>,
    pub init_args: Option<String>,
    pub rw_root: bool,
    pub mbr_file: Option<String>,
    pub output_file: String,
    pub force: bool,
    pub quiet: bool,
}

impl TryFrom<&ArgMatches> for BuildOptions {
    type Error = &'static str;

    fn try_from(matches: &ArgMatches) -> Result<Self, Self::Error> {
        Ok(BuildOptions {
            container_source: matches.get_one::<String>("container_source").cloned(),
            rootfs_dir: matches.get_one::<String>("rootfs_dir").cloned(),
            containerfile: matches.get_one::<String>("containerfile").cloned(),
            image_size: matches
                .get_one::<String>("image_size")
                .ok_or("need image size")?
                .to_string(),
            kernel_version: matches
                .get_one::<String>("kernel_version")
                .ok_or("need kernel version")?
                .to_string(),
            kernel_url: matches
                .get_one::<String>("kernel_url")
                .ok_or("need kernel URL")?
                .clone(),
            kernel_file: matches.get_one::<String>("kernel_file").cloned(),
            nvidia_drivers: matches.get_flag("nvidia_drivers"),
            kernel_modules: matches
                .get_many::<String>("kernel_module")
                .unwrap_or_default()
                .cloned()
                .collect::<Vec<_>>(),
            mounts: matches
                .get_many::<String>("mount")
                .unwrap_or_default()
                .cloned()
                .collect::<Vec<_>>(),
            no_gevulot_runtime: matches.get_flag("no_gevulot_runtime"),
            no_default_mounts: matches.get_flag("no_default_mounts"),
            init: matches.get_one::<String>("init").cloned(),
            init_args: matches.get_one::<String>("init_args").cloned(),
            rw_root: matches.get_flag("rw_root"),
            mbr_file: matches.get_one::<String>("mbr_file").cloned(),
            output_file: matches
                .get_one::<String>("output_file")
                .unwrap()
                .to_string(),
            force: matches.get_flag("force"),
            quiet: matches.get_flag("quiet"),
        })
    }
}

impl TryFrom<&BuildOptions> for LinuxVMBuildContext {
    type Error = anyhow::Error;

    fn try_from(opts: &BuildOptions) -> Result<Self, Self::Error> {
        let kernel_url = opts.kernel_url.clone();
        let kernel_file = opts.kernel_file.as_ref().map(|path| PathBuf::from(path));
        let kernel_version = opts.kernel_version.clone();
        // TODO: support blocked sizes like 1G and 100M
        let image_size = opts.image_size.parse().context("parse image size")?;
        let image_path = PathBuf::from(&opts.output_file);
        let rootfs_dir = opts.rootfs_dir.as_ref().map(PathBuf::from);
        let force = opts.force;

        let opts = linux_vm::BuildOpts {
            image_size,
            image_path,
            force,
            kernel_file,
            kernel_url,
            kernel_version,
            rootfs_dir,
        };

        Ok(Self::from_opts(opts))
    }
}

pub async fn build(matches: &ArgMatches) -> Result<()> {
    let options = BuildOptions::try_from(matches).map_err(|e| anyhow::anyhow!(e))?;
    let mut build_context = LinuxVMBuildContext::try_from(&options)?;
    linux_vm::build(&mut build_context)
}
