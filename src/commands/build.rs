use crate::builders::skopeo_builder::SkopeoSyslinuxBuilder;
use crate::builders::{BuildOptions, ImageBuilder};
use anyhow::Result;
use clap::{Arg, ArgGroup, ValueHint};

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
                .default_value("6.12"),
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

pub async fn build(matches: &clap::ArgMatches) -> Result<()> {
    let options = BuildOptions::try_from(matches).map_err(|e| anyhow::anyhow!(e))?;
    let builder = SkopeoSyslinuxBuilder {};
    builder.build(&options)
}
