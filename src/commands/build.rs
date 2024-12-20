use crate::builders::skopeo_builder::SkopeoSyslinuxBuilder;
use crate::builders::{BuildOptions, ImageBuilder};
use crate::OutputFormat;
use clap::ValueHint;
use std::path::PathBuf;

/// Build command.
#[derive(Clone, Debug, clap::Parser)]
pub struct BuildArgs {
    /// Image to use as a source for VM filesystem.
    #[command(flatten)]
    pub image: Image,

    /// Size of the disk image (e.g., 10G, 1024M).
    ///
    /// This determines the total capacity of the VM's virtual disk.
    #[arg(long, short = 's', value_name = "SIZE", default_value = "10G")]
    pub image_size: String,

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
    #[arg(long, value_name = "MODULENAME", conflicts_with_all = ["init", "init_args"])]
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
    /// If none provided, following paths will be tried:
    /// - /usr/share/syslinux/mbr.bin
    /// - /usr/lib/syslinux/mbr/mbr.bin
    /// - /usr/lib/syslinux/bios/mbr.bin
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath, verbatim_doc_comment)]
    pub mbr_file: Option<PathBuf>,

    /// Name of the output disk image file.
    ///
    /// This will be a bootable disk image you can use with QEMU or other VM software.
    #[arg(
        long,
        short,
        value_name = "FILE",
        value_hint = ValueHint::FilePath,
        default_value = "disk.img"
    )]
    pub output_file: PathBuf,

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

impl BuildArgs {
    /// Run build subcommand.
    pub async fn run(&self, _format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        build(self).await
    }
}

async fn build(build_args: &BuildArgs) -> Result<(), Box<dyn std::error::Error>> {
    let options = BuildOptions::from(build_args);
    let builder = SkopeoSyslinuxBuilder {};
    builder.build(&options)?;
    Ok(())
}
