use gevulot_rs::builders::{
    ByteSize, ByteUnit, MsgAckPinBuilder, MsgCreatePinBuilder, MsgDeletePinBuilder,
};
use patharg::InputArg;
use serde_json::Value;
use std::path::Path;

use crate::{connect_to_gevulot, print_object, read_file, ChainArgs, OutputFormat};

/// Pins command.
#[derive(Clone, Debug, clap::Parser)]
pub struct Command {
    #[command(flatten)]
    chain_args: ChainArgs,

    #[command(subcommand)]
    subcommand: Subcommand,
}

impl Command {
    /// Match pin subcommand and run it.
    pub async fn run(&self, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
        let value = match &self.subcommand {
            Subcommand::List => list_pins(&self.chain_args).await,
            Subcommand::Get { cid } => get_pin(&self.chain_args, cid).await,
            Subcommand::Ack {
                cid,
                id,
                worker_id,
                success,
            } => ack_pin(&self.chain_args, id, cid, worker_id, *success).await,
            Subcommand::Create { file } => {
                create_pin(&self.chain_args, file.path_ref().map(|v| &**v)).await
            }
            Subcommand::Delete { cid } => delete_pin(&self.chain_args, cid).await,
        }?;
        print_object(format, &value)
    }
}

/// Pin subcommand.
#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    /// List all pins.
    List,

    /// Get a specific pin.
    Get {
        /// The CID of the pin to retrieve.
        cid: String,
    },

    /// Ack a pin
    Ack {
        /// The ID of the pin to ack.
        id: String,

        /// The CID of the pin to ack.
        cid: String,

        /// The ID of the worker.
        worker_id: String,

        /// Success.
        success: bool,
    },

    /// Create a new pin.
    Create {
        /// The file to read the pin data from or '-' to read from stdin.
        #[arg(short, long, default_value_t)]
        file: InputArg,
    },

    /// Delete a pin.
    Delete {
        /// The CID of the pin to delete.
        cid: String,
    },
}

/// Lists all pins in the Gevulot network
async fn list_pins(chain_args: &ChainArgs) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let pins = client.pins.list().await?;
    // Convert the pins to the gevulot_rs::models::Pin type
    let pins: Vec<gevulot_rs::models::Pin> = pins.into_iter().map(Into::into).collect();
    Ok(serde_json::json!(pins))
}

/// Retrieves a specific pin from the Gevulot network
async fn get_pin(
    chain_args: &ChainArgs,
    pin_cid: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;
    let pin = client.pins.get(pin_cid).await?;
    // Convert the pin to the gevulot_rs::models::Pin type
    let pin: gevulot_rs::models::Pin = pin.into();
    Ok(serde_json::json!(pin))
}

/// Ack a specific pin
async fn ack_pin(
    chain_args: &ChainArgs,
    pin_id: &str,
    pin_cid: &str,
    worker_id: &str,
    success: bool,
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
        .pins
        .ack(
            MsgAckPinBuilder::default()
                .id(pin_id.to_string())
                .creator(me.clone())
                .cid(pin_cid.to_string())
                .worker_id(worker_id.to_string())
                .success(success)
                .into_message()?,
        )
        .await?;
    Ok(serde_json::json!({}))
}

/// Creates a new pin in the Gevulot network
async fn create_pin(
    chain_args: &ChainArgs,
    file: Option<&Path>,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Read pin data from file
    let pin: gevulot_rs::models::Pin = read_file(file).await?;
    let mut client = connect_to_gevulot(chain_args).await?;

    // Get the client's address
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    // Create the pin using the MsgCreatePinBuilder
    let resp = client
        .pins
        .create(
            MsgCreatePinBuilder::default()
                .creator(me.clone())
                .cid(pin.spec.cid.clone())
                .fallback_urls(pin.spec.fallback_urls.unwrap_or_default())
                .bytes(ByteSize::new(pin.spec.bytes as u64, ByteUnit::Byte))
                .time(pin.spec.time as u64)
                .redundancy(pin.spec.redundancy as u64)
                .name(pin.metadata.name)
                .description(pin.metadata.description)
                .labels(pin.metadata.labels.into_iter().map(Into::into).collect())
                .tags(pin.metadata.tags)
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": format!("Created pin with id: {}", resp.id),
        "id": resp.id,
    }))
}

/// Deletes a pin from the Gevulot network
async fn delete_pin(
    chain_args: &ChainArgs,
    pin_cid: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(chain_args).await?;

    // Get the client's address
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    // Delete the pin using the MsgDeletePinBuilder
    client
        .pins
        .delete(
            MsgDeletePinBuilder::default()
                .creator(me.clone())
                .cid(pin_cid.to_string())
                .into_message()?,
        )
        .await?;

    Ok(serde_json::json!({
        "status": "success",
        "message": format!("Deleted pin with CID: {}", pin_cid)
    }))
}
