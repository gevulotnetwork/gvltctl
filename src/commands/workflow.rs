use patharg::InputArg;
use std::path::Path;

use crate::ChainArgs;

/// Workflow command.
#[derive(Clone, Debug, clap::Parser)]
pub struct Command {
    #[command(subcommand)]
    subcommand: Subcommand,
}

impl Command {
    /// Match workflow subcommand and run it.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.subcommand {
            Subcommand::List { chain_args } => list_workflows(chain_args).await,
            Subcommand::Get { chain_args, id } => get_workflow(chain_args, id).await,
            Subcommand::Create { chain_args, file } => {
                create_workflow(chain_args, file.path_ref().map(|v| &**v)).await
            }
            Subcommand::Delete { chain_args, id } => delete_workflow(chain_args, id).await,
        }
    }
}

/// Workflow subcommand.
#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    /// List all workflows.
    List {
        /// Common chain arguments.
        #[command(flatten)]
        chain_args: ChainArgs,
    },

    /// Get a specific workflow.
    Get {
        /// Common chain arguments.
        #[command(flatten)]
        chain_args: ChainArgs,

        /// The ID of the workflow to retrieve.
        id: String,
    },

    /// Create a new workflow.
    Create {
        /// Common chain arguments.
        #[command(flatten)]
        chain_args: ChainArgs,

        /// The file to read the workflow data from or '-' to read from stdin.
        #[arg(short, long, default_value_t)]
        file: InputArg,
    },

    /// Delete a workflow.
    Delete {
        /// Common chain arguments.
        #[command(flatten)]
        chain_args: ChainArgs,

        /// The ID of the workflow to delete.
        id: String,
    },
}

async fn list_workflows(_chain_args: &ChainArgs) -> Result<(), Box<dyn std::error::Error>> {
    todo!("Listing all workflows");
}

async fn get_workflow(
    _chain_args: &ChainArgs,
    _workflow_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    todo!("Getting a specific workflow");
}

async fn create_workflow(
    _chain_args: &ChainArgs,
    _path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    todo!("Creating a new workflow");
}

async fn delete_workflow(
    _chain_args: &ChainArgs,
    _workflow_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    todo!("Deleting a workflow");
}
