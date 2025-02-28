use anyhow::Context;
use clap::builder::TypedValueParser;
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

    /// Environment variables to set for main program. Appends to provided in task file.
    ///
    /// Example: --env KEY=VALUE
    #[arg(
        short = 'e',
        long = "env",
        value_name = "KEY=VALUE",
        value_parser = EnvVarParser,
    )]
    envs: Vec<EnvVar>,

    /// Command to execute in VM (e.g. /bin/uptime).
    #[arg(long)]
    command: Option<String>,

    /// Arguments for command.
    #[arg(long, requires = "command", allow_hyphen_values = true)]
    args: Vec<String>,

    /// Input file passed to VM.
    ///
    /// SOURCE is path to a local file and TARGET is a path inside VM
    /// where this file will be put.
    ///
    /// If TARGET is an absolute path, it must start with prefix /mnt/gevulot/input.
    /// If it is relative, it will be treated as relative to /mnt/gevulot/input direcotry.
    ///
    /// Examples:
    ///
    /// --input file.txt:file.txt
    ///
    /// --input file.txt:/mnt/gevulot/input/file.txt
    #[arg(
        short = 'i',
        long = "input",
        value_name = "SOURCE:TARGET",
        value_parser = RunInputParser,
    )]
    inputs: Vec<RunInput>,

    /// Ouput file in VM to store.
    ///
    /// If PATH is an absolute path, it must start with prefix /mnt/gevulot/output.
    /// If it is relative, it will be treated as relative to /mnt/gevulot/output direcotry.
    ///
    /// Examples:
    ///
    /// --output file.txt
    ///
    /// --output /mnt/gevulot/output/file.txt
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    outputs: Vec<String>,

    /// Print stdout of VM during execution.
    #[arg(long, default_value_t)]
    stdout: bool,

    /// Print stderr of VM during execution.
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

#[derive(Clone, Debug)]
struct EnvVar {
    key: String,
    value: String,
}

impl From<&EnvVar> for TaskEnv {
    fn from(value: &EnvVar) -> Self {
        TaskEnv {
            name: value.key.clone(),
            value: value.value.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct EnvVarParser;

impl TypedValueParser for EnvVarParser {
    type Value = EnvVar;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> std::result::Result<Self::Value, clap::Error> {
        use clap::builder::StyledStr;
        use clap::error::ErrorKind;
        use clap::error::{ContextKind, ContextValue};

        let value = value
            .to_str()
            .ok_or_else(|| clap::Error::new(ErrorKind::InvalidUtf8).with_cmd(cmd))?;

        let (key, value) = value.split_once('=').ok_or_else(|| {
            let mut err = clap::Error::new(ErrorKind::ValueValidation).with_cmd(cmd);
            err.insert(
                ContextKind::Suggested,
                ContextValue::StyledStrs(vec![StyledStr::from(
                    "format of --input argument should be SOURCE:TARGET".to_string(),
                )]),
            );
            err
        })?;

        Ok(EnvVar {
            key: key.to_string(),
            value: value.to_string(),
        })
    }
}

#[derive(Clone, Debug)]
struct RunInput {
    source: String,
    target: String,
}

#[derive(Clone, Debug)]
struct RunInputParser;

impl TypedValueParser for RunInputParser {
    type Value = RunInput;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> std::result::Result<Self::Value, clap::Error> {
        use clap::builder::StyledStr;
        use clap::error::ErrorKind;
        use clap::error::{ContextKind, ContextValue};

        let value = value
            .to_str()
            .ok_or_else(|| clap::Error::new(ErrorKind::InvalidUtf8).with_cmd(cmd))?;

        let (source, target) = value.split_once(':').ok_or_else(|| {
            let mut err = clap::Error::new(ErrorKind::ValueValidation).with_cmd(cmd);
            err.insert(
                ContextKind::Suggested,
                ContextValue::StyledStrs(vec![StyledStr::from(
                    "format of --input argument should be SOURCE:TARGET".to_string(),
                )]),
            );
            err
        })?;

        // FIXME: because ':' is a valid symbol in paths, this options is ambiguous.
        // E.g. how should we treat this input: "foo:bar:baz"?
        // Is it source="foo:bar" and target="baz" or source="foo" and target="bar:baz"?
        // However this is such a corner case, that we will just leave this for now.
        // First occurence of ':' is now treated as a separator.

        Ok(RunInput {
            source: source.to_string(),
            target: target.to_string(),
        })
    }
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
        .flat_map(|arg| arg.split(' '))
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

    if !task_spec.output_contexts.is_empty()
        || task_spec.store_stdout.unwrap_or_default()
        || task_spec.store_stderr.unwrap_or_default()
    {
        fs::create_dir_all(&run_args.output_dir)
            .await
            .context("failed to create output directory")?;
    }

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

fn create_input_context(run_input: &RunInput) -> Result<InputContext> {
    let source = path::absolute(&run_input.source)?
        .to_str()
        .ok_or("non-UTF-8 charecter in path".to_string())?
        .to_string();

    let target_path = PathBuf::from(&run_input.target);
    let target = if target_path.is_absolute() {
        if target_path.starts_with(GEVULOT_INPUT_MOUNTPOINT) {
            target_path
        } else {
            return Err(format!(
                "input target must start with '{}': {}",
                GEVULOT_INPUT_MOUNTPOINT, run_input.target
            )
            .into());
        }
    } else {
        Path::new(GEVULOT_INPUT_MOUNTPOINT).join(&target_path)
    }
    .to_str()
    .ok_or("non-UTF-8 charecter in path".to_string())?
    .to_string();

    Ok(InputContext {
        source: format!("file://{}", source),
        target,
    })
}

fn create_output_context(output: &str) -> Result<OutputContext> {
    let output_path = PathBuf::from(output);
    let output = if output_path.is_absolute() {
        if !output_path.starts_with(GEVULOT_OUTPUT_MOUNTPOINT) {
            output_path
        } else {
            return Err(format!(
                "output path must start with '{}': {}",
                GEVULOT_OUTPUT_MOUNTPOINT, output
            )
            .into());
        }
    } else {
        Path::new(GEVULOT_OUTPUT_MOUNTPOINT).join(output)
    };
    Ok(OutputContext {
        source: output
            .to_str()
            .ok_or("non-UTF-8 charecter in path".to_string())?
            .to_string(),
        retention_period: -1,
    })
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
            .ok_or::<Error>("non-UTF-8 charecter in path".into())?
            .to_string();
        task_spec.image = format!("file://{}", path_str);
    }
    if let Some(command) = &run_args.command {
        task_spec.command = vec![command.clone()];
        task_spec.args = run_args.args.clone();
    }

    for env_var in &run_args.envs {
        task_spec.env.push(env_var.into());
    }

    for input in &run_args.inputs {
        task_spec.input_contexts.push(create_input_context(input)?);
    }

    for output in &run_args.outputs {
        task_spec
            .output_contexts
            .push(create_output_context(output)?);
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
    if let Some(path) = task_spec.image.strip_prefix("file://") {
        tokio::fs::metadata(path)
            .await
            .map_err::<Error, _>(|_| format!("VM file not found: {}", path).into())?;
    } else {
        // need to download file
        task_spec.image = "file://".to_string();
        todo!("download VM image from remote");
    }
    Ok(())
}

async fn prepare_inputs(task_spec: &mut TaskSpec, runtime_input: &Path) -> Result<()> {
    for input in &mut task_spec.input_contexts {
        if let Some(path) = input.source.strip_prefix("file://") {
            tokio::fs::metadata(path)
                .await
                .map_err::<Error, _>(|_| format!("input file not found: {}", path).into())?;
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
            fs::create_dir_all(run_args.output_dir.join(parent)).await?;
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
