use anyhow::Context;
use clap::builder::TypedValueParser;
use downloader::{Download, Downloader};
use gevulot_rs::models::{
    ByteUnit, DefaultFactorOneMegabyte, InputContext, OutputContext, Task, TaskEnv, TaskResources,
    TaskSpec,
};
use gevulot_rs::runtime_config::{self, DebugExit, RuntimeConfig};
use log::debug;
use nix::sys::signal::Signal;
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::os::unix::process::ExitStatusExt;
use std::path::{self, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;
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

    let runtime_cfg = generate_runtime_config(&task_spec).await;
    debug!("runtime config: {:#?}", &runtime_cfg);

    let runtime_dirs = RuntimeDirs::new()
        .await
        .map_err(into_anyhow)
        .context("failed to create runtime directories")?;
    debug!("runtime dirs: {:#?}", &runtime_dirs);

    let mut runtime_file = fs::File::create(runtime_dirs.runtime_config.join("config.yaml"))
        .await
        .context("failed to create runtime configuration")?;
    let runtime_file_content =
        serde_yaml::to_string(&runtime_cfg).context("failed to serialize runtime configuration")?;
    runtime_file
        .write_all(runtime_file_content.as_bytes())
        .await
        .context("failed to write runtime configuration")?;
    drop(runtime_file);

    prepare_inputs(&mut task_spec, &runtime_dirs.input)
        .await
        .map_err(into_anyhow)
        .context("failed to prepare input context")?;

    prepare_vm_image(&mut task_spec, runtime_dirs.root.path())
        .await
        .map_err(into_anyhow)
        .context("failed to prepare VM image")?;

    let qemu_args = run_args
        .qemu_args
        .iter()
        .flat_map(|arg| arg.split(' '))
        .collect::<Vec<_>>();

    debug!("task specification: {:#?}", &task_spec);

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

    if !task_spec.output_contexts.is_empty() || task_spec.store_stdout || task_spec.store_stderr {
        fs::create_dir_all(&run_args.output_dir)
            .await
            .context("failed to create output directory")?;
    }

    let stdout_file = task_spec
        .store_stdout
        .then(|| run_args.output_dir.join("stdout"));
    let stderr_file = task_spec
        .store_stderr
        .then(|| run_args.output_dir.join("stderr"));

    let timestamp = Instant::now();
    run_cmd(
        cmd,
        stdout_file.clone(),
        stderr_file.clone(),
        run_args.stdout,
        run_args.stderr,
    )
    .map_err(into_anyhow)
    .context("QEMU failed")?;
    let execution_time = timestamp.elapsed();

    let output_paths = store_outputs(run_args, &task_spec, &runtime_dirs.output)
        .await
        .map_err(into_anyhow)
        .context("failed to store output context")?;

    Ok(serde_json::json!({
        "message": "VM program exited successfully",
        "execution_time": execution_time.as_secs(),
        "output_contexts": output_paths,
        "stdout": stdout_file,
        "stderr": stderr_file,
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
                cpus: (run_args.smp.unwrap_or(1) as u64).into(),
                gpus: 0.into(),
                memory: (run_args.mem.unwrap_or(512) as u64).into(),
                time: 0.into(),
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
        task_spec.resources.cpus = (smp as u64).into();
    }

    if let Some(mem) = run_args.mem {
        task_spec.resources.memory =
            ByteUnit::Number(ByteUnit::<DefaultFactorOneMegabyte>::Number(mem as u64).bytes()?);
    }

    if run_args.store_stdout {
        task_spec.store_stdout = true;
    }
    if run_args.store_stderr {
        task_spec.store_stderr = true;
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
    root: TempDir,
    runtime_config: PathBuf,
    input: PathBuf,
    output: PathBuf,
}

impl RuntimeDirs {
    const RUNTIME_CONFIG: &str = "runtime_config";
    const INPUT: &str = "input";
    const OUTPUT: &str = "output";

    pub async fn new() -> Result<Self> {
        let root = TempDir::new("gevulot-local-run")?;
        let runtime_config = root.path().join(Self::RUNTIME_CONFIG);
        let input = root.path().join(Self::INPUT);
        let output = root.path().join(Self::OUTPUT);
        fs::create_dir(&runtime_config).await?;
        fs::create_dir(&input).await?;
        fs::create_dir(&output).await?;
        Ok(Self {
            root,
            runtime_config,
            input,
            output,
        })
    }
}

async fn prepare_vm_image(task_spec: &mut TaskSpec, runtime_path: &Path) -> Result<()> {
    if let Some(path) = task_spec.image.strip_prefix("file://") {
        tokio::fs::metadata(path)
            .await
            .map_err::<Error, _>(|_| format!("VM file not found: {}", path).into())?;
    } else {
        // download VM image and update source path
        let mut downloader = Downloader::builder()
            .download_folder(runtime_path)
            .build()?;
        let vm_image = Download::new(&task_spec.image).file_name(Path::new("v.img"));
        let summary = downloader
            .async_download(&[vm_image])
            .await?
            .into_iter()
            .next()
            .ok_or("failed to download VM image".to_string())??;
        let status_code = summary
            .status
            .first()
            .ok_or("failed to download VM image".to_string())?
            .1;
        if status_code != 200 {
            return Err(format!("failed to download VM image: status code {}", status_code).into());
        }
        task_spec.image = format!(
            "file://{}",
            summary
                .file_name
                .as_os_str()
                .to_str()
                .ok_or("non-UTF-8 charecter in path".to_string())?
        );
    }
    Ok(())
}

async fn prepare_inputs(task_spec: &mut TaskSpec, runtime_input: &Path) -> Result<()> {
    let mut inputs_to_download = vec![];
    let mut inputs_sources = vec![];
    let mut downloader = Downloader::builder()
        .download_folder(runtime_input)
        .build()?;

    for input in &mut task_spec.input_contexts {
        if let Some(path) = input.source.strip_prefix("file://") {
            // copy local source into runtime input dir
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
            // schedule remote source for downloading
            inputs_to_download.push(Download::new(&input.source));
            inputs_sources.push(&mut input.source);
        }
    }

    // perform downloading and update source paths
    let results = downloader.async_download(&inputs_to_download).await?;
    debug_assert_eq!(results.len(), inputs_sources.len());
    for (result, source) in results.into_iter().zip(inputs_sources.into_iter()) {
        match result {
            Ok(summary) => {
                let status_code = summary
                    .status
                    .first()
                    .ok_or(format!("failed to download '{}'", source))?
                    .1;
                if status_code != 200 {
                    return Err(format!(
                        "failed to download '{}': status code {}",
                        source, status_code
                    )
                    .into());
                }
                *source = format!(
                    "file://{}",
                    summary
                        .file_name
                        .as_os_str()
                        .to_str()
                        .ok_or("non-UTF-8 charecter in path".to_string())?
                );
            }
            Err(err) => return Err(format!("failed to download '{}': {}", source, err).into()),
        }
    }

    Ok(())
}

async fn store_outputs(
    run_args: &RunArgs,
    task_spec: &TaskSpec,
    runtime_output: &Path,
) -> Result<Vec<PathBuf>> {
    let mut output_paths = vec![];
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
        let output_path = run_args.output_dir.join(&relative);
        fs::copy(local, &output_path).await?;
        output_paths.push(output_path);
    }
    Ok(output_paths)
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

    cmd.args([
        "-smp",
        &task_spec
            .resources
            .cpus
            .millicores()?
            .div_ceil(1000)
            .to_string(),
    ]);

    cmd.args([
        "-m",
        &format!(
            "{}M",
            &task_spec.resources.memory.bytes()?.div_ceil(1000 * 1000)
        ),
    ]);

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
    rt_arg.push(runtime_dirs.runtime_config.as_os_str());
    rt_arg.push(format!(
        ",mount_tag={},security_model=none,multidevs=remap,readonly=on",
        GEVULOT_RT_CONFIG_TAG
    ));
    cmd.args([OsStr::new("-virtfs"), rt_arg.as_os_str()]);

    let mut input_arg = OsString::from("local,path=");
    input_arg.push(runtime_dirs.input.as_os_str());
    input_arg.push(format!(
        ",mount_tag={},security_model=none,multidevs=remap,readonly=on",
        GEVULOT_INPUT_TAG
    ));
    cmd.args([OsStr::new("-virtfs"), input_arg.as_os_str()]);

    let mut output_arg = OsString::from("local,path=");
    output_arg.push(runtime_dirs.output.as_os_str());
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
    } else if let Some(signal) = exit_status.signal() {
        Err(format!(
            "QEMU was terminated by signal: {}{}{}",
            signal,
            if let Some(name) = Signal::try_from(signal).ok().map(Signal::as_str) {
                format!(" ({})", name)
            } else {
                "".to_string()
            },
            if exit_status.core_dumped() {
                " [core dumped]"
            } else {
                ""
            }
        )
        .into())
    } else if let Some(signal) = exit_status.stopped_signal() {
        Err(format!(
            "QEMU was stopped by signal: {}{}",
            signal,
            if let Some(name) = Signal::try_from(signal).ok().map(Signal::as_str) {
                format!(" ({})", name)
            } else {
                "".to_string()
            }
        )
        .into())
    } else {
        Err(format!(
            "unexpected QEMU process event (wait status: {})",
            exit_status.into_raw()
        )
        .into())
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
