use bip32::{Mnemonic, Prefix, XPrv};
use clap::{CommandFactory as _, Parser as _};
use clap_complete::Shell;
use cosmrs::crypto::secp256k1::SigningKey;
use patharg::OutputArg;
use rand_core::OsRng;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

#[cfg_attr(not(feature = "vm-builder-v2"), path = "./builders/mod.rs")]
#[cfg_attr(feature = "vm-builder-v2", path = "./builders_v2/mod.rs")]
mod builders;
mod commands;
mod utils;
mod version;

use commands::*;
use utils::*;
use version::get_long_version;

/// CLI interface for Gevulot Control.
#[derive(Clone, Debug, clap::Parser)]
#[command(version, long_version = get_long_version(), about)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Sets the output format.
    #[arg(
        global = true,
        short = 'F',
        long,
        default_value_t,
        env = "GEVULOT_FORMAT"
    )]
    format: OutputFormat,
}

impl Cli {
    /// Match the command and run it.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.command {
            Command::Worker(command) => command.run(self.format).await,
            Command::Task(command) => command.run(self.format).await,
            Command::Pin(command) => command.run(self.format).await,
            Command::Workflow(command) => command.run().await,
            Command::Keygen { file, password } => {
                generate_key(file.path_ref(), password, self.format).await
            }
            Command::ComputeKey { mnemonic, password } => {
                compute_key(mnemonic, password, self.format).await
            }
            Command::Send {
                chain_args,
                amount,
                receiver,
            } => send_tokens(chain_args, *amount, receiver, self.format).await,
            Command::AccountInfo {
                chain_args,
                address,
            } => account_info(chain_args, address, self.format).await,
            Command::GenerateCompletion { shell, file } => {
                generate_completion(*shell, file.path_ref()).await
            }
            Command::Sudo(command) => command.run(self.format).await,
            Command::Build(build_args) => build_args.run(self.format).await,
        }
    }
}

/// Main commands of Gevulot Control CLI application.
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Command {
    /// Commands related to workers.
    Worker(workers::Command),

    /// Commands related to pins.
    Pin(pins::Command),

    /// Commands related to tasks.
    Task(tasks::Command),

    /// Commands related to workflows.
    Workflow(workflow::Command),

    /// Generate a new key.
    Keygen {
        /// The file to write the seed to or '-' to write to stdout.
        #[arg(short, long, default_value_t)]
        file: OutputArg,

        /// Sets the password for the Gevulot client.
        #[arg(short, long, default_value_t, hide_default_value = true)]
        password: String,
    },

    /// Compute a key.
    ComputeKey {
        /// The mnemonic to compute the key from.
        #[arg(long, env = "GEVULOT_MNEMONIC", hide_env_values = true)]
        mnemonic: String,

        /// The password to compute the key with.
        #[arg(short, long, default_value_t, hide_default_value = true)]
        password: String,
    },

    /// Send tokens to a receiver on the Gevulot network.
    Send {
        #[command(flatten)]
        chain_args: ChainArgs,

        /// The amount of tokens to send.
        amount: u128,

        /// The receiver address.
        receiver: String,
    },

    /// Get the balance of the given account.
    AccountInfo {
        #[command(flatten)]
        chain_args: ChainArgs,

        /// The address to get the balance of.
        address: String,
    },

    /// Generate shell completion scripts.
    GenerateCompletion {
        /// The shell to generate the completion scripts for.
        shell: clap_complete::Shell,

        /// The file to write the completion scripts to or '-' to write to stdout.
        #[arg(short, long, default_value_t)]
        file: OutputArg,
    },

    /// Perform administrative operations with sudo privileges.
    Sudo(sudo::Command),

    /// Build a VM image from a container, rootfs directory, or Containerfile.
    Build(build::BuildArgs),

}

/// Main entry point for the Gevulot Control CLI application.
///
/// This function sets up the command-line interface, parses arguments,
/// and dispatches to the appropriate subcommand handlers.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Cli::parse().run().await
}

/// Sends tokens to a receiver on the Gevulot network.
async fn send_tokens(
    chain_args: &ChainArgs,
    amount: u128,
    receiver: &str,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = connect_to_gevulot(chain_args).await?;
    client
        .base_client
        .write()
        .await
        .token_transfer(receiver, amount)
        .await?;

    let output = serde_json::json!({
        "success": true,
        "amount": amount,
        "receiver": receiver
    });

    print_object(format, &output)?;

    Ok(())
}

/// Retrieves and displays account information for a given address.
async fn account_info(
    chain_args: &ChainArgs,
    address: &str,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = connect_to_gevulot(chain_args).await?;
    let account = client
        .base_client
        .write()
        .await
        .get_account(address)
        .await?;
    let balance = client
        .base_client
        .write()
        .await
        .get_account_balance(address)
        .await?;

    let output = serde_json::json!({
        "account_number": account.account_number,
        "sequence": account.sequence,
        "balance": balance.amount.to_string()
    });

    print_object(format, &output)?;
    Ok(())
}

/// Generates a new key and optionally saves it to a file.
async fn generate_key(
    path: Option<&PathBuf>,
    password: &str,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    // Generate random Mnemonic using the default language (English)
    let mnemonic = Mnemonic::random(OsRng, Default::default());

    // Derive a BIP39 seed value using the given password
    let seed = mnemonic.to_seed(password);

    // Derive a child `XPrv` using the provided BIP32 derivation path
    let child_path = "m/44'/118'/0'/0/0";
    let child_xprv = XPrv::derive_from_path(&seed, &child_path.parse()?)?;

    // Get the `XPub` associated with `child_xprv`.
    let child_xpub = child_xprv.public_key();

    // Serialize `child_xprv` as a string with the `xprv` prefix.
    let child_xprv_str = child_xprv.to_string(Prefix::XPRV);
    assert!(child_xprv_str.starts_with("xprv"));

    // Serialize `child_xpub` as a string with the `xpub` prefix.
    let child_xpub_str = child_xpub.to_string(Prefix::XPUB);
    assert!(child_xpub_str.starts_with("xpub"));

    // Get the ECDSA/secp256k1 signing and verification keys for the xprv and xpub
    let sk = SigningKey::from_slice(&child_xprv.private_key().to_bytes())?;

    let account_id = sk.public_key().account_id("gvlt").unwrap();
    let phrase = mnemonic.phrase();

    let output = serde_json::json!({
        "account_id": account_id,
        "mnemonic": phrase
    });

    if let Some(file) = path {
        let mut file = File::create(file)?;
        file.write_all(phrase.as_bytes())?;
    }

    print_object(format, &output)?;

    Ok(())
}

/// Generates a new key and optionally saves it to a file.
async fn compute_key(
    mnemonic: &str,
    password: &str,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let mnemonic = Mnemonic::new(mnemonic, bip32::Language::English)?;

    // Derive a BIP39 seed value using the given password
    let seed = mnemonic.to_seed(password);

    // Derive a child `XPrv` using the provided BIP32 derivation path
    let child_path = "m/44'/118'/0'/0/0";
    let child_xprv = XPrv::derive_from_path(&seed, &child_path.parse()?)?;

    // Get the `XPub` associated with `child_xprv`.
    let child_xpub = child_xprv.public_key();

    // Serialize `child_xprv` as a string with the `xprv` prefix.
    let child_xprv_str = child_xprv.to_string(Prefix::XPRV);
    assert!(child_xprv_str.starts_with("xprv"));

    // Serialize `child_xpub` as a string with the `xpub` prefix.
    let child_xpub_str = child_xpub.to_string(Prefix::XPUB);
    assert!(child_xpub_str.starts_with("xpub"));

    // Get the ECDSA/secp256k1 signing and verification keys for the xprv and xpub
    let sk = SigningKey::from_slice(&child_xprv.private_key().to_bytes())?;

    let account_id = sk.public_key().account_id("gvlt").unwrap();

    let output = serde_json::json!({ "account_id": account_id });
    print_object(format, &output)?;
    Ok(())
}

/// Generates shell completion scripts for the gvltctl command-line tool.
async fn generate_completion(
    shell: Shell,
    path: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Generating completion file for {shell}...");
    let mut cmd = Cli::command();
    if let Some(file) = path {
        let mut file = File::create(file)?;
        clap_complete::generate(shell, &mut cmd, "gvltctl", &mut file);
    } else {
        clap_complete::generate(shell, &mut cmd, "gvltctl", &mut io::stdout());
    }
    Ok(())
}
