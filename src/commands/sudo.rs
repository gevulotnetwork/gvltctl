use gevulot_rs::proto::gevulot::gevulot::{
    MsgSudoDeletePin, MsgSudoDeleteTask, MsgSudoDeleteWorker, MsgSudoFreezeAccount,
};
use serde_json::Value;

use crate::{connect_to_gevulot, print_object, ChainArgs, OutputFormat};

/// Sudo command.
#[derive(Clone, Debug, clap::Parser)]
pub struct Command {
    #[command(flatten)]
    chain_args: ChainArgs,

    #[command(subcommand)]
    subcommand: Subcommand,
}

impl Command {
    /// Match sudo subcommand and run it.
    pub async fn run(&self, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        let value = match &self.subcommand {
            Subcommand::DeletePin { id } => sudo_delete_pin(&self.chain_args, id.clone()).await,
            Subcommand::DeleteWorker { id } => {
                sudo_delete_worker(&self.chain_args, id.clone()).await
            }
            Subcommand::DeleteTask { id } => sudo_delete_task(&self.chain_args, id.clone()).await,
            Subcommand::FreezeAccount { address } => {
                sudo_freeze_account(&self.chain_args, address.clone()).await
            }
        }?;
        print_object(format, &value)
    }
}

/// Sudo subcommand.
#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    /// Delete a pin using sudo privileges.
    DeletePin {
        /// The ID of the pin to delete.
        id: String,
    },

    /// Delete a worker using sudo privileges.
    DeleteWorker {
        /// The ID of the worker to delete.
        id: String,
    },

    /// Delete a task using sudo privileges.
    DeleteTask {
        /// The ID of the task to delete.
        id: String,
    },

    /// Freeze an account using sudo privileges.
    FreezeAccount {
        /// The address of the account to freeze.
        address: String,
    },
}

/// Deletes a pin using sudo privileges.
async fn sudo_delete_pin(
    chain_args: &ChainArgs,
    cid: String,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let msg = MsgSudoDeletePin {
        authority: client.base_client.write().await.address.clone().unwrap(),
        cid,
    };
    client.sudo.delete_pin(msg).await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": "Pin deleted successfully"
    }))
}

/// Deletes a worker using sudo privileges.
async fn sudo_delete_worker(
    chain_args: &ChainArgs,
    worker_id: String,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let msg = MsgSudoDeleteWorker {
        authority: client.base_client.write().await.address.clone().unwrap(),
        id: worker_id,
    };
    client.sudo.delete_worker(msg).await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": "Worker deleted successfully"
    }))
}

/// Deletes a task using sudo privileges.
async fn sudo_delete_task(
    chain_args: &ChainArgs,
    task_id: String,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let msg = MsgSudoDeleteTask {
        authority: client.base_client.write().await.address.clone().unwrap(),
        id: task_id,
    };
    client.sudo.delete_task(msg).await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": "Task deleted successfully"
    }))
}

/// Freezes an account using sudo privileges.
async fn sudo_freeze_account(
    chain_args: &ChainArgs,
    account: String,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let msg = MsgSudoFreezeAccount {
        authority: client.base_client.write().await.address.clone().unwrap(),
        account,
    };
    client.sudo.freeze_account(msg).await?;
    Ok(serde_json::json!({
        "status": "success",
        "message": "Account frozen successfully"
    }))
}
