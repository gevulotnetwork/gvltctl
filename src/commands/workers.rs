use gevulot_rs::builders::{
    ByteSize, ByteUnit, MsgAnnounceWorkerExitBuilder, MsgCreateWorkerBuilder,
    MsgUpdateWorkerBuilder,
    MsgDeleteWorkerBuilder,
};
use patharg::InputArg;
use serde_json::Value;
use std::path::Path;

use crate::{connect_to_gevulot, print_object, read_file, ChainArgs, OutputFormat};

/// Workers command.
#[derive(Clone, Debug, clap::Parser)]
pub struct Command {
    #[command(flatten)]
    chain_args: ChainArgs,

    #[command(subcommand)]
    subcommand: Subcommand,
}

impl Command {
    /// Match worker subcommand and run it.
    pub async fn run(&self, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        let value = match &self.subcommand {
            Subcommand::List => list_workers(&self.chain_args).await,
            Subcommand::Get { id } => get_worker(&self.chain_args, id).await,
            Subcommand::Create { file } => {
                create_worker(&self.chain_args, file.path_ref().map(|v| &**v)).await
            }
            Subcommand::Delete { id } => delete_worker(&self.chain_args, id).await,
            Subcommand::AnnounceExit { id } => announce_worker_exit(&self.chain_args, id).await,
            Subcommand::Update { file } => {
                update_worker(&self.chain_args, file.path_ref().map(|v| &**v)).await
            }
        }?;
        print_object(format, &value)
    }
}

/// Worker subcommand.
#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    /// List all workers.
    List,

    /// Get a specific worker.
    Get {
        /// The ID of the worker to retrieve.
        id: String,
    },

    /// Create a new worker.
    Create {
        /// The file to read the worker data from or '-' to read from stdin.
        #[arg(short, long, default_value_t)]
        file: InputArg,
    },

    /// Delete a worker.
    Delete {
        /// The ID of the worker to delete.
        id: String,
    },

    /// Announce a worker's exit.
    AnnounceExit {
        /// The ID of the worker to announce exit.
        id: String,
    },

    /// Update a worker.
    Update {
        /// The file to read the worker data from or '-' to read from stdin.
        #[arg(short, long, default_value_t)]
        file: InputArg,
    },
}

/// Lists all workers.
async fn list_workers(chain_args: &ChainArgs) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let workers = client.workers.list().await?;
    let workers: Vec<gevulot_rs::models::Worker> = workers.into_iter().map(Into::into).collect();
    Ok(serde_json::json!(workers))
}

/// Retrieves a specific worker by ID.
async fn get_worker(
    chain_args: &ChainArgs,
    worker_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let worker = client.workers.get(worker_id).await?;
    let worker: gevulot_rs::models::Worker = worker.into();
    Ok(serde_json::json!(worker))
}

/// Creates a new worker based on the provided configuration.
async fn create_worker(
    chain_args: &ChainArgs,
    path: Option<&Path>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let worker: gevulot_rs::models::Worker = read_file(path).await?;
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;
    let resp = client
        .workers
        .create(
            MsgCreateWorkerBuilder::default()
                .creator(me)
                .name(worker.metadata.name)
                .description(worker.metadata.description)
                .tags(worker.metadata.tags.into_iter().collect())
                .labels(worker.metadata.labels.into_iter().map(Into::into).collect())
                .cpus(worker.spec.cpus.millicores()? as u64)
                .gpus(worker.spec.gpus.millicores()? as u64)
                .memory(ByteSize::new(
                    worker.spec.memory.bytes()? as u64,
                    ByteUnit::Byte,
                ))
                .disk(ByteSize::new(
                    worker.spec.disk.bytes()? as u64,
                    ByteUnit::Byte,
                ))
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": "Worker created successfully",
        "worker_id": resp.id
    }))
}

/// Updates a worker with the specified ID.
async fn update_worker(
    chain_args: &ChainArgs,
    path: Option<&Path>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let worker: gevulot_rs::models::Worker = read_file(path).await?;
    let mut client = connect_to_gevulot(chain_args).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;
    let id = worker.metadata.id.ok_or("Worker ID not found")?;
    client
        .workers
        .update(
            MsgUpdateWorkerBuilder::default()
                .creator(me)
                .id(id.clone())
                .name(worker.metadata.name)
                .description(worker.metadata.description)
                .tags(worker.metadata.tags.into_iter().collect())
                .labels(worker.metadata.labels.into_iter().map(Into::into).collect())
                .cpus(worker.spec.cpus.millicores()? as u64)
                .gpus(worker.spec.gpus.millicores()? as u64)
                .memory(ByteSize::new(
                    worker.spec.memory.bytes()? as u64,
                    ByteUnit::Byte,
                ))
                .disk(ByteSize::new(
                    worker.spec.disk.bytes()? as u64,
                    ByteUnit::Byte,
                ))
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": "Worker updated successfully",
        "worker_id": id,
    }))
}

/// Deletes a worker with the specified ID.
async fn delete_worker(
    chain_args: &ChainArgs,
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
        .workers
        .delete(
            MsgDeleteWorkerBuilder::default()
                .creator(me.clone())
                .id(worker_id.to_string())
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": format!("Worker {} deleted successfully", worker_id)
    }))
}

async fn announce_worker_exit(
    chain_args: &ChainArgs,
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
        .workers
        .announce_exit(
            MsgAnnounceWorkerExitBuilder::default()
                .creator(me)
                .worker_id(worker_id.to_string())
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": format!("Worker {} announced exit successfully", worker_id)
    }))
}
