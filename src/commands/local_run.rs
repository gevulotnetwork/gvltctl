use anyhow::Context;
use gevulot_rs::models::{InputContext, OutputContext, Task, TaskEnv, TaskResources, TaskSpec};
use gevulot_rs::runtime_config::{self, DebugExit, RuntimeConfig};
use log::debug;
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::path::{self, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use tempdir::TempDir;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::{print_object, read_file, OutputFormat};

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

/// Local VM run arguments.
#[derive(Clone, Debug, clap::Parser)]
pub struct RunArgs {
    /// Path to QEMU executable.
    ///
    /// If not specified, it will be auto-detected.
    #[arg(long)]
    qemu_path: Option<PathBuf>,

    /// Additional QEMU arguments.
    #[arg(long = "qemu-arg", value_name = "ARGS", allow_hyphen_values = true)]
    qemu_args: Vec<String>,

    /// Task file. Only task specification is taken into account.
    ///
    /// Options passed directly through CLI will overwrite options in file.
    #[arg(short = 'f', long)]
    file: Option<PathBuf>,

    /// VM image to run.
    #[arg(value_name = "FILE")]
    image: Option<PathBuf>,

    /// Number of CPU cores to allocate to VM. If no task file provided, defaults to 1.
    #[arg(short = 's', long, value_name = "NUM")]
    smp: Option<u16>,

    /// Memory in MBs. If no task file provided, defaults to 512.
    #[arg(short = 'm', long)]
    mem: Option<u32>,

    /// PCI device path to GPU device.
    #[arg(short = 'g', long)]
    gpu: Vec<String>,

    /// Environment variables to set for main program. E.g. --env KEY=VALUE.
    ///
    /// Appends to provided in task file.
    #[arg(short = 'e', long)]
    env: Vec<String>,

    /// Command to execute in VM (e.g. /bin/uptime).
    #[arg(long)]
    command: Option<String>,

    /// Arguments for command.
    #[arg(long, requires = "command", allow_hyphen_values = true)]
    args: Vec<String>,

    /// Input for VM. Example: "./file.txt:/mnt/gevulot/input/file.txt"
    ///
    /// Format: <local_path>:<path_in_vm>
    /// All VM paths must start with prefix `/mnt/gevulot/input`.
    #[arg(short = 'i', long = "input", value_name = "FILE:PATH")]
    inputs: Vec<String>,

    /// Output of VM. Example: "/mnt/gevulot/output/file.txt"
    ///
    /// All VM paths must start with prefix `/mnt/output/input`.
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    outputs: Vec<String>,

    /// Print stdout of VM while executing.
    #[arg(long, default_value_t)]
    stdout: bool,

    /// Print stderr of VM while executing.
    #[arg(long, default_value_t)]
    stderr: bool,

    /// Store stdout of VM into output file.
    #[arg(long, default_value_t)]
    store_stdout: bool,

    /// Store stderr of VM into output file.
    #[arg(long, default_value_t)]
    store_stderr: bool,

    /// Directory to store output files if some.
    #[arg(long, value_name = "DIR", default_value = "output")]
    output_dir: PathBuf,
}

const DEFAULT_QEMU: &str = "qemu-system-x86_64";

const GEVULOT_RT_CONFIG_TAG: &str = "gevulot-rt-config";
const GEVULOT_OUTPUT_TAG: &str = "gevulot-output";
const GEVULOT_INPUT_TAG: &str = "gevulot-input";
const GEVULOT_INPUT_MOUNTPOINT: &str = "/mnt/gevulot/input/";
const GEVULOT_OUTPUT_MOUNTPOINT: &str = "/mnt/gevulot/output/";

const DEBUG_EXIT: DebugExit = DebugExit::default_x86();

impl RunArgs {
    /// Run local-run subcommand.
    pub async fn run(&self, format: OutputFormat) -> Result<()> {
        let value = run(self).await?;
        print_object(format, &value)
    }
}

async fn run(run_args: &RunArgs) -> anyhow::Result<Value> {
    let into_anyhow = |err: Error| -> anyhow::Error { anyhow::anyhow!(err.to_string()) };

    let qemu_path = resolve_qemu(run_args.qemu_path.as_ref())
        .map_err(into_anyhow)
        .context("failed to find QEMU executable")?;
    debug!("resolved QEMU: {}", qemu_path.display());

    let mut task_spec = get_task_spec(run_args)
        .await
        .map_err(into_anyhow)
        .context("failed to compile task specification")?;
    debug!("task specification: {:#?}", &task_spec);

    validate_task(&task_spec)
        .await
        .map_err(into_anyhow)
        .context("invalid task specification")?;

    let runtime_cfg = generate_runtime_config(&task_spec).await;
    debug!("runtime config: {:#?}", &runtime_cfg);

    let runtime_dirs = RuntimeDirs::new()
        .await
        .map_err(into_anyhow)
        .context("failed to create runtime directories")?;
    debug!("runtime dirs: {:#?}", &runtime_dirs);

    let mut runtime_file = fs::File::create(runtime_dirs.runtime_config.path().join("config.yaml"))
        .await
        .context("failed to create runtime configuration")?;
    let runtime_file_content =
        serde_yaml::to_string(&runtime_cfg).context("failed to serialize runtime configuration")?;
    runtime_file
        .write_all(runtime_file_content.as_bytes())
        .await
        .context("failed to write runtime configuration")?;
    drop(runtime_file);

    prepare_inputs(&mut task_spec, runtime_dirs.input.path())
        .await
        .map_err(into_anyhow)
        .context("failed to prepare input context")?;

    prepare_vm_image(&mut task_spec)
        .await
        .map_err(into_anyhow)
        .context("failed to prepare VM image")?;

    let qemu_args = run_args
        .qemu_args
        .iter()
        .map(|arg| arg.split(' '))
        .flatten()
        .collect::<Vec<_>>();

    let cmd = build_cmd(
        &qemu_path,
        &task_spec,
        &runtime_dirs,
        &run_args.gpu,
        &qemu_args,
    )
    .map_err(into_anyhow)
    .context("failed to generate QEMU arguments")?;
    debug!("QEMU cmd: {:#?}", &cmd);

    create_output_directory(run_args, &task_spec)
        .await
        .map_err(into_anyhow)
        .context("failed to create output directory")?;

    let stdout_file = task_spec
        .store_stdout
        .unwrap_or_default()
        .then(|| run_args.output_dir.join("stdout"));
    let stderr_file = task_spec
        .store_stderr
        .unwrap_or_default()
        .then(|| run_args.output_dir.join("stderr"));

    run_cmd(
        cmd,
        stdout_file,
        stderr_file,
        run_args.stdout,
        run_args.stderr,
    )
    .map_err(into_anyhow)
    .context("QEMU failed")?;

    store_outputs(run_args, &task_spec, runtime_dirs.output.path())
        .await
        .map_err(into_anyhow)
        .context("failed to store output context")?;

    Ok(serde_json::json!({
        "message": "VM program exited successfully"
    }))
}

async fn get_task_spec(run_args: &RunArgs) -> Result<TaskSpec> {
    let mut task_spec = if let Some(path) = &run_args.file {
        read_file::<Task>(Some(path.as_path())).await?.spec
    } else {
        TaskSpec {
            image: Default::default(),
            command: Default::default(),
            args: Default::default(),
            env: Default::default(),
            input_contexts: Default::default(),
            output_contexts: Default::default(),
            resources: TaskResources {
                cpus: run_args.smp.unwrap_or(1).into(),
                gpus: Default::default(),
                memory: run_args.mem.unwrap_or(512).into(),
                time: Default::default(),
            },
            store_stdout: Default::default(),
            store_stderr: Default::default(),
        }
    };
    if let Some(path) = &run_args.image {
        let absolute_path = path::absolute(path)?;
        let path_str = absolute_path
            .as_os_str()
            .to_str()
            .ok_or::<Error>("non-UTF-8 path".into())?
            .to_string();
        task_spec.image = format!("file://{}", path_str);
    }
    if let Some(command) = &run_args.command {
        task_spec.command = vec![command.clone()];
        task_spec.args = run_args.args.clone();
    }
    for entry in &run_args.env {
        let (k, v) = entry
            .split_once('=')
            .ok_or::<Error>(format!("invalid environment variable format: {}", entry).into())?;
        task_spec.env.push(TaskEnv {
            name: k.to_string(),
            value: v.to_string(),
        });
    }

    for input in &run_args.inputs {
        let (file, path) = input
            .split_once(':')
            .ok_or::<Error>(format!("invalid input format: {}", input).into())?;
        task_spec.input_contexts.push(InputContext {
            source: format!("file://{}", file),
            target: path.to_string(),
        });
    }

    for output in &run_args.outputs {
        task_spec.output_contexts.push(OutputContext {
            source: output.clone(),
            retention_period: -1,
        });
    }

    if let Some(smp) = run_args.smp {
        task_spec.resources.cpus = smp as u64;
    }

    if let Some(mem) = run_args.mem {
        task_spec.resources.memory = mem as u64;
    }

    if run_args.store_stdout {
        task_spec.store_stdout = Some(true);
    }
    if run_args.store_stderr {
        task_spec.store_stderr = Some(true);
    }
    Ok(task_spec)
}

async fn validate_task(task_spec: &TaskSpec) -> Result<()> {
    if let Some(path) = task_spec.image.strip_prefix("file://") {
        tokio::fs::metadata(path)
            .await
            .map_err::<Error, _>(|_| format!("VM file not found: {}", path).into())?;
    }

    for input in &task_spec.input_contexts {
        // if source is local file, check that it exists
        if let Some(file) = input.source.strip_prefix("file://") {
            tokio::fs::metadata(file)
                .await
                .map_err::<Error, _>(|_| format!("input file not found: {}", file).into())?;
        }

        if !input.target.starts_with(GEVULOT_INPUT_MOUNTPOINT) {
            return Err(format!(
                "input path must start with '{}': {}",
                GEVULOT_INPUT_MOUNTPOINT, input.target
            )
            .into());
        }
    }

    for output in &task_spec.output_contexts {
        if !output.source.starts_with(GEVULOT_OUTPUT_MOUNTPOINT) {
            return Err(format!(
                "output path must start with '{}': {}",
                GEVULOT_OUTPUT_MOUNTPOINT, output.source
            )
            .into());
        }
    }

    Ok(())
}

async fn generate_runtime_config(task_spec: &TaskSpec) -> RuntimeConfig {
    let mut env = vec![];
    for entry in &task_spec.env {
        env.push(runtime_config::EnvVar {
            key: entry.name.clone(),
            value: entry.value.clone(),
        });
    }
    let command = task_spec.command.first().cloned();
    let mut args = if task_spec.command.len() > 1 {
        task_spec.command[1..].to_vec()
    } else {
        Vec::new()
    };
    args.extend_from_slice(&task_spec.args);
    RuntimeConfig {
        version: runtime_config::VERSION.to_string(),
        command,
        args,
        env,
        debug_exit: Some(DEBUG_EXIT),
        ..Default::default()
    }
}

#[derive(Debug)]
struct RuntimeDirs {
    runtime_config: TempDir,
    input: TempDir,
    output: TempDir,
}

impl RuntimeDirs {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            runtime_config: TempDir::new("gevulot-runtime-config")?,
            input: TempDir::new("gevulot-input")?,
            output: TempDir::new("gevulot-output")?,
        })
    }
}

async fn prepare_vm_image(task_spec: &mut TaskSpec) -> Result<()> {
    if task_spec.image.starts_with("file://") {
        // local file, do nothing
    } else {
        // remove file, need to download
        task_spec.image = "file://".to_string();
        todo!("download VM image from remote");
    }
    Ok(())
}

async fn prepare_inputs(task_spec: &mut TaskSpec, runtime_input: &Path) -> Result<()> {
    for input in &mut task_spec.input_contexts {
        if let Some(path) = input.source.strip_prefix("file://") {
            let relative = PathBuf::from(
                input
                    .target
                    .strip_prefix(GEVULOT_INPUT_MOUNTPOINT)
                    .expect("input target must be valid"),
            );
            if let Some(parent) = relative.parent() {
                fs::create_dir_all(runtime_input.join(parent)).await?;
            }
            fs::copy(path, runtime_input.join(relative)).await?;
        } else {
            input.source = "file://".to_string();
            todo!("download input context from remote");
        }
    }
    Ok(())
}

async fn create_output_directory(run_args: &RunArgs, task_spec: &TaskSpec) -> Result<()> {
    if !task_spec.output_contexts.is_empty()
        || task_spec.store_stdout.unwrap_or_default()
        || task_spec.store_stderr.unwrap_or_default()
    {
        fs::create_dir_all(&run_args.output_dir).await?;
    }
    Ok(())
}

async fn store_outputs(
    run_args: &RunArgs,
    task_spec: &TaskSpec,
    runtime_output: &Path,
) -> Result<()> {
    for output in &task_spec.output_contexts {
        let relative = PathBuf::from(
            output
                .source
                .strip_prefix(GEVULOT_OUTPUT_MOUNTPOINT)
                .expect("output path must be valid"),
        );
        let local = runtime_output.join(&relative);
        if let Some(parent) = local.parent() {
            fs::create_dir_all(run_args.output_dir.join(&parent)).await?;
        }
        fs::copy(local, run_args.output_dir.join(&relative)).await?;
    }
    Ok(())
}

fn build_cmd(
    qemu_path: &Path,
    task_spec: &TaskSpec,
    runtime_dirs: &RuntimeDirs,
    gpu: &[String],
    qemu_args: &[&str],
) -> Result<Command> {
    let mut cmd = Command::new(qemu_path.as_os_str());

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    cmd.args(["-machine", "q35"]);
    cmd.args([
        "-device",
        "pcie-root-port,port=0x10,chassis=1,id=pci.1,bus=pcie.0,multifunction=on,addr=0x3",
    ]);
    cmd.args([
        "-device",
        "pcie-root-port,port=0x11,chassis=2,id=pci.2,bus=pcie.0,addr=0x3.0x1",
    ]);
    cmd.args([
        "-device",
        "pcie-root-port,port=0x12,chassis=3,id=pci.3,bus=pcie.0,addr=0x3.0x2",
    ]);
    cmd.args(["-device", "virtio-scsi-pci,bus=pci.2,addr=0x0,id=scsi0"]);
    cmd.args(["-device", "scsi-hd,bus=scsi0.0,drive=hd0"]);
    cmd.args(["-vga", "none"]);
    cmd.args(["-device", "virtio-rng-pci"]);
    cmd.args(["-machine", "accel=kvm:tcg"]);
    cmd.args(["-cpu", "max"]);
    cmd.args(["-display", "none"]);
    cmd.args(["-serial", "stdio"]);

    cmd.args(["-smp", &task_spec.resources.cpus.to_string()]);

    cmd.args(["-m", &format!("{}M", &task_spec.resources.memory)]);

    for entry in gpu {
        cmd.args(["-device", &format!("vfio-pci,rombar=0,host={}", entry)]);
    }

    let DebugExit::X86 { iobase, iosize, .. } = DEBUG_EXIT;
    cmd.args([
        "-device",
        &format!("isa-debug-exit,iobase=0x{:x},iosize=0x{:x}", iobase, iosize),
    ]);

    cmd.args([
        "-drive",
        &format!(
            "file={},format=raw,if=none,id=hd0,readonly=on",
            &task_spec
                .image
                .strip_prefix("file://")
                .ok_or::<Error>("task image failed to convert into local file URI".into())?
        ),
    ]);

    let mut rt_arg = OsString::from("local,path=");
    rt_arg.push(runtime_dirs.runtime_config.path().as_os_str());
    rt_arg.push(format!(
        ",mount_tag={},security_model=none,multidevs=remap,readonly=on",
        GEVULOT_RT_CONFIG_TAG
    ));
    cmd.args([OsStr::new("-virtfs"), rt_arg.as_os_str()]);

    let mut input_arg = OsString::from("local,path=");
    input_arg.push(runtime_dirs.input.path().as_os_str());
    input_arg.push(format!(
        ",mount_tag={},security_model=none,multidevs=remap,readonly=on",
        GEVULOT_INPUT_TAG
    ));
    cmd.args([OsStr::new("-virtfs"), input_arg.as_os_str()]);

    let mut output_arg = OsString::from("local,path=");
    output_arg.push(runtime_dirs.output.path().as_os_str());
    output_arg.push(format!(
        ",mount_tag={},security_model=none,multidevs=remap",
        GEVULOT_OUTPUT_TAG
    ));
    cmd.args([OsStr::new("-virtfs"), output_arg.as_os_str()]);

    cmd.args(qemu_args);

    Ok(cmd)
}

fn run_cmd(
    mut cmd: Command,
    stdout_file: Option<PathBuf>,
    stderr_file: Option<PathBuf>,
    print_stdout: bool,
    print_stderr: bool,
) -> Result<()> {
    use std::fs;
    use std::io::{BufRead, BufReader, Write};

    let mut child = cmd.spawn()?;

    // Log stdout and stderr of child process from separate threads
    let stdout = child
        .stdout
        .take()
        .ok_or::<Error>("failed to capture stdout of QEMU".into())?;

    let stderr = child
        .stderr
        .take()
        .ok_or::<Error>("failed to capture stderr of QEMU".into())?;

    let stdout_thread = thread::spawn(move || {
        let mut stdout_writer = stdout_file
            .map(fs::File::create)
            .transpose()
            .expect("failed to open stdout file for writing");
        BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
            .for_each(|line| {
                if print_stdout {
                    println!("{}", line);
                }
                if let Some(writer) = &mut stdout_writer {
                    writer
                        .write_all(format!("{}\n", line).as_bytes())
                        .expect("failed to write stdout to file");
                }
            });
    });

    let stderr_thread = thread::spawn(move || {
        let mut stderr_writer = stderr_file
            .map(fs::File::create)
            .transpose()
            .expect("failed to open stderr file for writing");
        BufReader::new(stderr)
            .lines()
            .map_while(std::io::Result::ok)
            .for_each(|line| {
                if print_stderr {
                    eprintln!("{}", line);
                }
                if let Some(writer) = &mut stderr_writer {
                    writer
                        .write_all(format!("{}\n", line).as_bytes())
                        .expect("failed to write stderr to file");
                }
            });
    });

    let exit_status = child.wait()?;

    stdout_thread.join().expect("failed to join thread");
    stderr_thread.join().expect("failed to join thread");

    let DebugExit::X86 { success_code, .. } = DEBUG_EXIT;
    let success_code = success_code as i32;
    let error_code = 1i32;

    if let Some(code) = exit_status.code() {
        if code == success_code {
            Ok(())
        } else if code == error_code {
            Err("VM program failed".into())
        } else {
            Err(format!(
                "QEMU exited with code: {} (abnormal exit code for VM)",
                code
            )
            .into())
        }
    } else {
        Err("VM terminated unexpectedly".into())
    }
}

fn resolve_qemu<P>(path: Option<P>) -> Result<PathBuf>
where
    P: AsRef<Path>,
{
    if let Some(path) = path {
        if !path.as_ref().is_file() {
            return Err(format!("QEMU path '{}' not found", path.as_ref().display()).into());
        }
        Ok(path.as_ref().to_path_buf())
    } else {
        Ok(which::which(DEFAULT_QEMU)?)
    }
}
