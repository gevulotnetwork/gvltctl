use patharg::InputArg;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use gevulot_rs::builders::{
    ByteSize, ByteUnit, MsgAcceptTaskBuilder, MsgCreateTaskBuilder, MsgDeclineTaskBuilder,
    MsgFinishTaskBuilder, MsgRescheduleTaskBuilder,
};

use crate::{connect_to_gevulot, print_object, read_file, ChainArgs, OutputFormat};

/// Tasks command.
#[derive(Clone, Debug, clap::Parser)]
pub struct Command {
    #[command(flatten)]
    chain_args: ChainArgs,

    #[command(subcommand)]
    subcommand: Subcommand,
}

impl Command {
    /// Match task subcommand and run it.
    pub async fn run(&self, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        let value = match &self.subcommand {
            Subcommand::List => list_tasks(&self.chain_args).await,
            Subcommand::Get { id } => get_task(&self.chain_args, id).await,
            Subcommand::Create { file } => {
                create_task(&self.chain_args, file.path_ref().map(|v| &**v)).await
            }
            Subcommand::Accept { id, worker_id } => {
                accept_task(&self.chain_args, id, worker_id).await
            }
            Subcommand::Decline { id, worker_id } => {
                decline_task(&self.chain_args, id, worker_id).await
            }
            Subcommand::Finish {
                id,
                exit_code,
                stdout,
                stderr,
                error,
                output_contexts,
            } => {
                finish_task(
                    &self.chain_args,
                    id,
                    *exit_code,
                    stdout.as_ref(),
                    stderr.as_ref(),
                    error.as_ref(),
                    output_contexts.as_ref(),
                )
                .await
            }
            Subcommand::Reschedule { id } => reschedule_task(&self.chain_args, id).await,
            Subcommand::Delete { id } => delete_task(&self.chain_args, id).await,
        }?;
        print_object(format, &value)
    }
}

/// Task subcommand.
#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    /// List all tasks.
    List,

    /// Get a specific task.
    Get {
        /// The ID of the task to retrieve.
        id: String,
    },

    /// Create a new task.
    Create {
        /// The file to read the task data from or '-' to read from stdin.
        #[arg(short, long, default_value_t)]
        file: InputArg,
    },

    /// Accept a task (you probably should not use this).
    #[command(hide = true)]
    Accept {
        /// The ID of the task to accept.
        id: String,

        /// The ID of the worker accepting the task.
        worker_id: String,
    },

    /// Decline a task (you probably should not use this).
    #[command(hide = true)]
    Decline {
        /// The ID of the task to decline.
        id: String,

        /// The ID of the worker declining the task.
        worker_id: String,
    },

    /// Finish a task (you probably should not use this).
    #[command(hide = false)]
    Finish {
        /// The ID of the task to finish.
        id: String,

        /// The exit code of the task.
        #[arg(default_value_t)]
        exit_code: i32,

        /// The stdout output of the task.
        stdout: Option<String>,

        /// The stderr output of the task.
        stderr: Option<String>,

        /// Any error message from the task.
        error: Option<String>,

        /// Output contexts produced by the task.
        output_contexts: Option<Vec<String>>,
    },

    /// Reschedule a task.
    Reschedule {
        /// The ID of the task to reschedule.
        id: String,
    },

    /// Delete a task.
    Delete {
        /// The ID of the task to delete.
        id: String,
    },
}

/// Lists all tasks.
async fn list_tasks(chain_args: &ChainArgs) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let tasks = client.tasks.list().await?;
    let tasks: Vec<gevulot_rs::models::Task> = tasks.into_iter().map(Into::into).collect();
    Ok(serde_json::json!(tasks))
}

/// Retrieves and displays information for a specific task.
async fn get_task(
    chain_args: &ChainArgs,
    task_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = crate::connect_to_gevulot(chain_args).await?;
    let task = client.tasks.get(task_id).await?;
    let task: gevulot_rs::models::Task = task.into();
    Ok(serde_json::json!(task))
}

/// Creates a new task based on the provided specification.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to read the task specification file, connect to Gevulot, and determine the output format.
///
/// # Returns
///
/// A Result indicating success or an error if the task creation fails.
pub async fn create_task(
    chain_args: &ChainArgs,
    path: Option<&Path>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let task: gevulot_rs::models::Task = read_file(path).await?;
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    let env: HashMap<String, String> = task
        .spec
        .env
        .iter()
        .map(|e| (e.name.clone(), e.value.clone()))
        .collect();

    let input_contexts: HashMap<String, String> = task
        .spec
        .input_contexts
        .iter()
        .map(|e| (e.source.clone(), e.target.clone()))
        .collect();

    let labels: HashMap<String, String> = task
        .metadata
        .labels
        .into_iter()
        .map(|label| (label.key, label.value))
        .collect();

    let resp = client
        .tasks
        .create(
            MsgCreateTaskBuilder::default()
                .creator(me.clone())
                .image(task.spec.image)
                .command(task.spec.command)
                .args(task.spec.args)
                .env(env)
                .input_contexts(input_contexts)
                .output_contexts(
                    task.spec
                        .output_contexts
                        .into_iter()
                        .map(|oc| (oc.source, oc.retention_period as u64))
                        .collect(),
                )
                .cpus(task.spec.resources.cpus as u64)
                .gpus(task.spec.resources.gpus as u64)
                .memory(ByteSize::new(
                    task.spec.resources.memory as u64,
                    ByteUnit::Byte,
                ))
                .time(task.spec.resources.time as u64)
                .store_stdout(task.spec.store_stdout.unwrap_or(false))
                .store_stderr(task.spec.store_stderr.unwrap_or(false))
                .labels(labels)
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": "Task created successfully",
        "task_id": resp.id
    }))
}

pub async fn accept_task(
    chain_args: &ChainArgs,
    task_id: &str,
    worker_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    client
        .tasks
        .accept(
            MsgAcceptTaskBuilder::default()
                .creator(me.clone())
                .task_id(task_id.to_string())
                .worker_id(worker_id.to_string())
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({}))
}

pub async fn decline_task(
    chain_args: &ChainArgs,
    task_id: &str,
    worker_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    client
        .tasks
        .decline(
            MsgDeclineTaskBuilder::default()
                .creator(me.clone())
                .task_id(task_id.to_string())
                .worker_id(worker_id.to_string())
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({}))
}

pub async fn finish_task(
    chain_args: &ChainArgs,
    task_id: &str,
    exit_code: i32,
    stdout: Option<&String>,
    stderr: Option<&String>,
    error: Option<&String>,
    output_contexts: Option<&Vec<String>>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    client
        .tasks
        .finish(
            MsgFinishTaskBuilder::default()
                .creator(me.clone())
                .task_id(task_id.to_string())
                .exit_code(exit_code)
                .stdout(stdout.cloned())
                .stderr(stderr.cloned())
                .output_contexts(output_contexts.cloned())
                .error(error.cloned())
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({}))
}

pub async fn reschedule_task(
    chain_args: &ChainArgs,
    task_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;
    let resp = client
        .tasks
        .reschedule(
            MsgRescheduleTaskBuilder::default()
                .creator(me.clone())
                .task_id(task_id.to_string())
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": "Task rescheduled successfully",
        "primary": resp.primary,
        "secondary": resp.secondary
    }))
}

pub async fn delete_task(
    chain_args: &ChainArgs,
    task_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    client
        .tasks
        .delete(gevulot_rs::proto::gevulot::gevulot::MsgDeleteTask {
            creator: me.clone(),
            id: task_id.to_string(),
        })
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": "Task deleted successfully"
    }))
}
