use anyhow::{anyhow, Context, Result};
use log::{debug, info, trace};
use mia_installer::{runtime_config, RuntimeConfig};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::builders::linux_vm::rootfs::RootFS;
use crate::builders::linux_vm::utils::run_command;
use crate::builders::Step;

use super::{ContainerBackend, LinuxVMBuildContext, LinuxVMBuilderError};

/// Container image reference.
#[derive(Clone, Debug)]
pub struct ContainerImage {
    backend: ContainerBackend,
    id: String,
    remove_on_drop: bool,
}

impl ContainerImage {
    /// Create new container image reference (do not build anything).
    ///
    /// If `remove_on_drop` is set to `true`, image will be removed when this object is dropped.
    pub const fn new(backend: ContainerBackend, id: String, remove_on_drop: bool) -> Self {
        Self {
            backend,
            id,
            remove_on_drop,
        }
    }

    /// Build container image using given Containerfile and current directory as build context
    /// and return created reference.
    ///
    /// `remove_on_drop` will be set to `true`.
    pub fn build(backend: ContainerBackend, containerfile: &Path) -> Result<Self> {
        let (out, _) = run_command([
            OsStr::new(backend.exe()),
            OsStr::new("build"),
            OsStr::new("--file"),
            containerfile.as_os_str(),
            OsStr::new("."),
        ])?;
        let id = out
            .lines()
            .last()
            .ok_or(anyhow!("failed to obtain container image ID"))?
            .to_string();
        Ok(Self {
            backend,
            id,
            remove_on_drop: true,
        })
    }

    /// Remove container image.
    pub fn remove(&self) -> Result<()> {
        run_command([self.backend.exe(), "rmi", &self.id])?;
        Ok(())
    }

    /// Get execution parameters of the container.
    pub fn get_config(&self) -> Result<oci_spec::image::Config> {
        let (out, _) = run_command([self.backend.exe(), "image", "inspect", &self.id])?;
        let manifest_json: serde_json::Value =
            serde_json::from_str(&out).context("failed to parse image manifest JSON")?;
        let config_json = manifest_json
            .as_array()
            .ok_or(anyhow!("invalid image manifest"))?
            .get(0)
            .ok_or(anyhow!("invalid image manifest"))?
            .as_object()
            .ok_or(anyhow!("invalid image manifest"))?
            .get("Config")
            .ok_or(anyhow!("invalid image manifest"))?
            .clone();
        let config: oci_spec::image::Config = serde_json::from_value(config_json)?;
        Ok(config)
    }
}

impl Drop for ContainerImage {
    fn drop(&mut self) {
        if self.remove_on_drop {
            // Ignore errors
            let _ = self.remove();
        }
    }
}

#[derive(Clone, Debug)]
pub struct Container {
    backend: ContainerBackend,
    id: String,
}

impl Container {
    /// Create container.
    pub fn create(image: &ContainerImage) -> Result<Self> {
        let (id, _) = run_command([image.backend.exe(), "create", image.id.as_str()])
            .context("failed to create container")?;
        Ok(Self {
            backend: image.backend,
            id,
        })
    }

    /// Remove container.
    pub fn remove(&self) -> Result<()> {
        run_command([self.backend.exe(), "rm", self.id.as_str()])
            .context("failed to remove container")?;
        Ok(())
    }

    /// Export and unpack filesystem. Temporary archive is deleted.
    pub fn export(&self, path: &Path) -> Result<()> {
        let archive_path = path.join(format!("{}.tar", &self.id));
        run_command([
            OsStr::new(self.backend.exe()),
            OsStr::new("export"),
            OsStr::new(self.id.as_str()),
            OsStr::new("--output"),
            archive_path.as_os_str(),
        ])
        .context("failed to export container filesystem")?;

        trace!(
            "unpacking archive {} into {}",
            archive_path.display(),
            path.display()
        );
        let archive_file = fs::File::open(&archive_path)?;
        let mut archive = tar::Archive::new(archive_file);
        archive
            .unpack(path)
            .context("failed to unpack container filesystem archive")?;

        trace!("removing file {}", archive_path.display());
        fs::remove_file(&archive_path).context("failed to remove container filesystem archive")?;

        Ok(())
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        // Ignore errors
        let _ = self.remove();
    }
}

/// Export filesystem from container into VM.
///
/// # Context variables required
/// - `container-image`
/// - `rootfs`
#[derive(Clone, Debug)]
pub struct CopyFilesystem;

impl Step<LinuxVMBuildContext> for CopyFilesystem {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("creating filesystem from container image");
        let image = ctx.get::<ContainerImage>("container-image").ok_or(
            LinuxVMBuilderError::invalid_context("build root filesystem", "image reference"),
        )?;

        let container = Container::create(image).context("failed to create container")?;
        debug!("created container: {}", &container.id);

        let rootfs = ctx
            .get::<RootFS>("rootfs")
            .ok_or(LinuxVMBuilderError::invalid_context(
                "build root filesystem",
                "root filesystem handler",
            ))?;

        debug!(
            "exporting container filesystem to {}",
            rootfs.path().display()
        );
        container
            .export(rootfs.path())
            .context("failed to export container filesystem")?;

        Ok(())
    }
}

/// Build container image from Dockerfile.
///
/// # Context variables defined:
/// - `container-image`
pub struct BuildContainerImage {
    backend: ContainerBackend,
    containerfile: PathBuf,
}

impl BuildContainerImage {
    pub fn new(backend: ContainerBackend, containerfile: PathBuf) -> Self {
        Self {
            backend,
            containerfile,
        }
    }
}

impl Step<LinuxVMBuildContext> for BuildContainerImage {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        info!("building container image");
        let image = ContainerImage::build(self.backend, &self.containerfile)
            .context("failed to build container image")?;
        debug!("image built: {}", &image.id);
        ctx.set("container-image", Box::new(image));
        Ok(())
    }
}

/// Extract runtime config from the container and turn it into [`gevulot_rs::runtime_config`].
///
/// # Context variables required
/// - `container-image`
///
/// # Context variables defined
/// - `container-rt-config`
pub struct GetContainerRuntime;

impl Step<LinuxVMBuildContext> for GetContainerRuntime {
    fn run(&mut self, ctx: &mut LinuxVMBuildContext) -> Result<()> {
        if let Some(image) = ctx.get::<ContainerImage>("container-image") {
            info!("exctracting runtime configugation");
            let config = image.get_config().context("failed to get runtime config")?;
            let mut rt_config = RuntimeConfig::default();
            // Add enviromnental variables
            if let Some(env_vars) = config.env() {
                for var in env_vars {
                    trace!("runtime env: {}", var);
                    let (key, value) = var
                        .split_once('=')
                        .ok_or(anyhow!("invalid environment variable"))?;
                    rt_config.env.push(runtime_config::EnvVar {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            }

            rt_config.working_dir = config.working_dir().clone();
            trace!("runtime working directory: {:?}", &rt_config.working_dir);

            let mut exec_string = Vec::new();
            // Try to get the ENTRYPOINT execution params
            if let Some(entrypoint) = config.entrypoint() {
                exec_string.append(&mut entrypoint.clone());
            }
            // Try to get CMD from execution params
            if let Some(cmd) = config.cmd() {
                exec_string.append(&mut cmd.clone());
            }

            if exec_string.is_empty() {
                // Do nothing, image have no default commands.
            } else {
                rt_config.command = Some(exec_string[0].clone());
                rt_config.args = exec_string[1..].to_vec();
            }
            trace!("runtime command: {:?}", &rt_config.command);
            trace!("runtime args: {:?}", &rt_config.args);

            ctx.set("container-rt-config", Box::new(rt_config));
        }

        Ok(())
    }
}
