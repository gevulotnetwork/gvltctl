use anyhow::Result;

use crate::build::BuildArgs;

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
    pub mia_version: Option<String>,
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
        writeln!(
            f,
            "| MIA Version      | {:<42} |",
            self.mia_version.as_deref().unwrap_or("None")
        )?;
        writeln!(f, "| Gevulot runtime  | {:<42} |", !self.no_gevulot_runtime)?;
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

impl From<&BuildArgs> for BuildOptions {
    fn from(args: &BuildArgs) -> Self {
        BuildOptions {
            container_source: args.image.container.clone(),
            rootfs_dir: args
                .image
                .rootfs_dir
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            containerfile: args
                .image
                .containerfile
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            image_size: args.image_size.clone().unwrap_or("10G".to_string()),
            kernel_version: args.kernel_version.clone(),
            kernel_url: Some(args.kernel_url.clone()),
            kernel_file: args.kernel_file.clone(),
            nvidia_drivers: args.nvidia_drivers,
            kernel_modules: args.kernel_modules.clone(),
            mounts: args.mounts.clone(),
            mia_version: Some(args.mia_version.clone()),
            no_gevulot_runtime: args.no_gevulot_runtime,
            no_default_mounts: args.no_default_mounts,
            init: args.init.clone(),
            init_args: args.init_args.clone(),
            rw_root: args.rw_root,
            mbr_file: args
                .mbr_file
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            output_file: args.output_file.to_string_lossy().to_string(),
            force: args.force,
            quiet: args.quiet,
        }
    }
}
