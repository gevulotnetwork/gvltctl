use anyhow::Result;

pub mod nvidia;
pub mod skopeo_builder;

pub trait ImageBuilder {
    fn build(&self, options: &BuildOptions) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub container_source: Option<String>,
    pub rootfs_dir: Option<String>,
    pub containerfile: Option<String>,
    pub image_size: String,
    pub kernel_version: String,
    pub kernel_url: Option<String>,
    pub kernel_file: Option<String>,
    pub nvidia_drivers: bool,
    pub kernel_modules: Vec<String>,
    pub mounts: Vec<String>,
    pub no_default_mounts: bool,
    pub init: Option<String>,
    pub init_args: Option<String>,
    pub rw_root: bool,
    pub mbr_file: Option<String>,
    pub output_file: String,
    pub force: bool,
    pub quiet: bool,
}

impl std::fmt::Display for BuildOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "+------------------+--------------------------------------------+"
        )?;
        writeln!(
            f,
            "| Option           | Value                                      |"
        )?;
        writeln!(
            f,
            "+------------------+--------------------------------------------+"
        )?;
        writeln!(
            f,
            "| Container Source | {:<42} |",
            self.container_source
                .as_deref()
                .unwrap_or("None (will use rootfs or Containerfile)")
        )?;
        writeln!(
            f,
            "| Rootfs Directory | {:<42} |",
            self.rootfs_dir
                .as_deref()
                .unwrap_or("None (will use container or Containerfile)")
        )?;
        writeln!(
            f,
            "| Containerfile    | {:<42} |",
            self.containerfile
                .as_deref()
                .unwrap_or("None (will use rootfs or container)")
        )?;
        writeln!(f, "| Image Size       | {:<42} |", self.image_size)?;
        writeln!(f, "| Kernel Version   | {:<42} |", self.kernel_version)?;
        writeln!(
            f,
            "| Kernel URL       | {:<42} |",
            self.kernel_url.as_deref().unwrap_or("None")
        )?;
        writeln!(
            f,
            "| Kernel File      | {:<42} |",
            self.kernel_file
                .as_deref()
                .unwrap_or("None (will download and build)")
        )?;
        writeln!(f, "| NVIDIA drivers   | {:<42} |", self.nvidia_drivers)?;
        writeln!(
            f,
            "| Kernel modules   | {:<42} |",
            if self.kernel_modules.is_empty() {
                "None".to_string()
            } else {
                self.kernel_modules.join(" ")
            }
        )?;
        writeln!(
            f,
            "| Mounts           | {:<42} |",
            if self.mounts.is_empty() {
                "None".to_string()
            } else {
                self.mounts.join(" ")
            }
        )?;
        writeln!(f, "| Default mounts   | {:<42} |", !self.no_default_mounts)?;
        writeln!(
            f,
            "| Init             | {:<42} |",
            self.init
                .as_deref()
                .unwrap_or("None (will use MIA as default)")
        )?;
        writeln!(
            f,
            "| Init Args        | {:<42} |",
            self.init_args
                .as_deref()
                .unwrap_or("None (will use ENTRYPOINT and CMD)")
        )?;
        writeln!(f, "| Read-only root   | {:<42} |", !self.rw_root)?;
        writeln!(
            f,
            "| MBR File         | {:<42} |",
            self.mbr_file
                .as_deref()
                .unwrap_or("None (will check for local one)")
        )?;
        writeln!(f, "| Output File      | {:<42} |", self.output_file)?;
        writeln!(f, "| Force            | {:<42} |", self.force)?;
        writeln!(f, "| Quiet            | {:<42} |", self.quiet)?;
        writeln!(
            f,
            "+------------------+--------------------------------------------+"
        )?;
        Ok(())
    }
}

impl TryFrom<&clap::ArgMatches> for BuildOptions {
    type Error = &'static str;

    fn try_from(matches: &clap::ArgMatches) -> Result<Self, Self::Error> {
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
            kernel_url: matches.get_one::<String>("kernel_url").cloned(),
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
