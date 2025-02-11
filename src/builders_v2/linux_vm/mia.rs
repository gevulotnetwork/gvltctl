use anyhow::{Context, Result};
use log::info;
use mia_installer::{runtime_config, RuntimeConfig};
use std::path::PathBuf;

use crate::builders::Step;

use super::LinuxVMBuildContext;

/// Install MIA into root filesystem.
///
/// # Context variables required
/// - `root-fs`
pub struct InstallMia {
    version: String,
    gevulot_runtime: bool,
    kernel_modules: Vec<String>,
    mounts: Vec<String>,
    default_mounts: bool,
}

impl InstallMia {
    pub fn new(
        version: String,
        gevulot_runtime: bool,
        kernel_modules: Vec<String>,
        mounts: Vec<String>,
        default_mounts: bool,
    ) -> Self {
        Self {
            version,
            gevulot_runtime,
            kernel_modules,
            mounts,
            default_mounts,
        }
    }
}

impl Step<LinuxVMBuildContext> for InstallMia {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("installing MIA ({})", &self.version);

        let mut mounts = self
            .mounts
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

        let follow_config = if self.gevulot_runtime {
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

        let container_rt_config = ctx
            .get::<RuntimeConfig>("container-rt-config")
            .cloned()
            .unwrap_or_default();
        let mut kernel_modules = ctx
            .get::<Vec<String>>("kernel-modules")
            .cloned()
            .unwrap_or_default();
        kernel_modules.append(&mut self.kernel_modules);

        let rt_config = RuntimeConfig {
            version: runtime_config::VERSION.to_string(),
            command: container_rt_config.command,
            args: container_rt_config.args,
            env: container_rt_config.env,
            working_dir: container_rt_config.working_dir,
            mounts,
            default_mounts: self.default_mounts,
            kernel_modules,
            follow_config,
            ..Default::default()
        };

        let mut install_config = mia_installer::InstallConfig::default();
        install_config.mia_version = self.version.to_string();
        install_config.mia_platform = "x86_64-unknown-linux-gnu".to_string();

        let rootfs = ctx.get::<PathBuf>("root-fs").expect("root-fs");

        install_config.prefix = rootfs.clone();

        // In case there is an init system installed in the container
        install_config.overwrite_symlink = true;

        install_config.rt_config = Some(rt_config);

        // TODO: add 'fetch' option to utilize cache and avoid re-downloading
        mia_installer::install(&install_config).context("failed to install MIA")?;

        Ok(())
    }
}
