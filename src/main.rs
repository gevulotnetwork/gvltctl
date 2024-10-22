use bip32::{Mnemonic, Prefix, XPrv};
use clap::ArgAction;
use clap::{value_parser, Arg, Command, ValueHint};
use clap_complete::{generate, Shell};
use cosmrs::crypto::secp256k1::SigningKey;
use gevulot_rs::gevulot_client::GevulotClientBuilder;
use gevulot_rs::GevulotClient;
use rand_core::OsRng;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Write};

mod builders;
mod commands;

use commands::{build::*, pins::*, tasks::*, workers::*, sudo::*};

/// Main entry point for the Gevulot Control CLI application.
///
/// This function sets up the command-line interface, parses arguments,
/// and dispatches to the appropriate subcommand handlers.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Parse command-line arguments
    let cmd = setup_command_line_args()?;

    // Handle matches here
    match cmd.get_matches().subcommand() {
        Some(("worker", sub_m)) => match sub_m.subcommand() {
            Some(("list", sub_m)) => list_workers(sub_m).await?,
            Some(("get", sub_m)) => get_worker(sub_m).await?,
            Some(("create", sub_m)) => create_worker(sub_m).await?,
            Some(("delete", sub_m)) => delete_worker(sub_m).await?,
            _ => println!("Unknown worker command"),
        },
        Some(("pin", sub_m)) => match sub_m.subcommand() {
            Some(("list", sub_m)) => list_pins(sub_m).await?,
            Some(("get", sub_m)) => get_pin(sub_m).await?,
            Some(("create", sub_m)) => create_pin(sub_m).await?,
            Some(("delete", sub_m)) => delete_pin(sub_m).await?,
            _ => println!("Unknown pin command"),
        },
        Some(("task", sub_m)) => match sub_m.subcommand() {
            Some(("list", sub_m)) => list_tasks(sub_m).await?,
            Some(("get", sub_m)) => get_task(sub_m).await?,
            Some(("create", sub_m)) => create_task(sub_m).await?,
            _ => println!("Unknown task command"),
        },
        Some(("workflow", sub_m)) => match sub_m.subcommand() {
            Some(("list", sub_m)) => list_workflows(sub_m).await?,
            Some(("get", sub_m)) => get_workflow(sub_m).await?,
            Some(("create", sub_m)) => create_workflow(sub_m).await?,
            Some(("delete", sub_m)) => delete_workflow(sub_m).await?,
            _ => println!("Unknown workflow command"),
        },
        Some(("sudo", sub_m)) => match sub_m.subcommand() {
            Some(("delete-pin", sub_m)) => sudo_delete_pin(sub_m).await?,
            Some(("delete-worker", sub_m)) => sudo_delete_worker(sub_m).await?,
            Some(("delete-task", sub_m)) => sudo_delete_task(sub_m).await?,
            Some(("freeze-account", sub_m)) => sudo_freeze_account(sub_m).await?,
            _ => println!("Unknown sudo command"),
        },
        Some(("keygen", sub_m)) => generate_key(sub_m).await?,
        Some(("compute-key", sub_m)) => compute_key(sub_m).await?,
        Some(("send", sub_m)) => send_tokens(sub_m).await?,
        Some(("account-info", sub_m)) => account_info(sub_m).await?,
        Some(("generate-completion", sub_m)) => generate_completion(sub_m).await?,
        Some(("build", sub_m)) => build(sub_m).await?,
        _ => println!("Unknown command"),
    }

    Ok(())
}

/// Parses command-line arguments and returns the matches.
///
/// This function sets up the entire command-line interface structure,
/// including all subcommands and their respective arguments.
fn setup_command_line_args() -> Result<Command, Box<dyn std::error::Error>> {
    let chain_args = [
        Arg::new("endpoint")
            .short('e')
            .long("endpoint")
            .value_name("URL")
            .env("GEVULOT_ENDPOINT")
            .help("Sets the endpoint for the Gevulot client")
            .value_hint(ValueHint::Url)
            .action(ArgAction::Set),
        Arg::new("gas_price")
            .short('g')
            .long("gas-price")
            .value_name("PRICE")
            .env("GEVULOT_GAS_PRICE")
            .help("Sets the gas price for the Gevulot client")
            .value_hint(ValueHint::Other)
            .action(ArgAction::Set),
        Arg::new("gas_multiplier")
            .short('m')
            .long("gas-multiplier")
            .value_name("MULTIPLIER")
            .env("GEVULOT_GAS_MULTIPLIER")
            .help("Sets the gas multiplier for the Gevulot client")
            .value_hint(ValueHint::Other)
            .action(ArgAction::Set),
        Arg::new("mnemonic")
            .short('n')
            .long("mnemonic")
            .value_name("MNEMONIC")
            .env("GEVULOT_MNEMONIC")
            .help("Sets the mnemonic for the Gevulot client")
            .value_hint(ValueHint::Other)
            .action(ArgAction::Set),
        Arg::new("format")
            .short('F')
            .long("format")
            .value_name("FORMAT")
            .env("GEVULOT_FORMAT")
            .help("Sets the output format (yaml, json, prettyjson, toml)")
            .value_hint(ValueHint::Other)
            .default_value("yaml")
            .action(ArgAction::Set),
    ];

    Ok(Command::new("gvltctl")
        .version("1.0")
        .author("Author Name <author@example.com>")
        .about("Gevulot Control CLI")
        .subcommand_required(true)
        // Worker subcommand
        .subcommand(
            Command::new("worker")
                .about("Commands related to workers")
                .subcommand_required(true)
                .subcommand(
                    Command::new("list")
                        .about("List all workers")
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("get")
                        .about("Get a specific worker")
                        .arg(
                            Arg::new("id")
                                .value_name("ID")
                                .help("The ID of the worker to retrieve")
                                .required(true)
                                .index(1),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("create")
                        .about("Create a new worker")
                        .arg(
                            Arg::new("file")
                                .short('f')
                                .long("file")
                                .value_name("FILE")
                                .value_hint(ValueHint::FilePath)
                                .help("The file to read the worker data from, defaults to stdin")
                                .action(ArgAction::Set),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("delete")
                        .about("Delete a worker")
                        .args(&chain_args),
                ),
        )
        // Pin subcommand
        .subcommand(
            Command::new("pin")
                .about("Commands related to pins")
                .subcommand_required(true)
                .subcommand(
                    Command::new("list")
                        .about("List all pins")
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("get")
                        .about("Get a specific pin")
                        .arg(
                            Arg::new("cid")
                                .value_name("CID")
                                .help("The CID of the pin to retrieve")
                                .value_hint(ValueHint::Other)
                                .required(true)
                                .index(1),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("create")
                        .about("Create a new pin")
                        .arg(
                            Arg::new("file")
                                .short('f')
                                .long("file")
                                .value_name("FILE")
                                .value_hint(ValueHint::FilePath)
                                .help("The file to read the pin data from, defaults to stdin")
                                .action(ArgAction::Set),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("delete")
                        .about("Delete a pin")
                        .args(&chain_args),
                ),
        )
        // Task subcommand
        .subcommand(
            Command::new("task")
                .about("Commands related to tasks")
                .subcommand_required(true)
                .subcommand(
                    Command::new("list")
                        .about("List all tasks")
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("get")
                        .about("Get a specific task")
                        .arg(
                            Arg::new("id")
                                .value_name("ID")
                                .help("The ID of the task to retrieve")
                                .value_hint(ValueHint::Other)
                                .required(true)
                                .index(1),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("create")
                        .about("Create a new task")
                        .arg(
                            Arg::new("file")
                                .short('f')
                                .long("file")
                                .value_name("FILE")
                                .help("The file to read the task data from, defaults to stdin")
                                .value_hint(ValueHint::FilePath)
                                .action(ArgAction::Set),
                        )
                        .args(&chain_args),
                ),
        )
        // Workflow subcommand
        .subcommand(
            Command::new("workflow")
                .about("Commands related to workflows")
                .subcommand_required(true)
                .subcommand(
                    Command::new("list")
                        .about("List all workflows")
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("get")
                        .about("Get a specific workflow")
                        .arg(
                            Arg::new("id")
                                .value_name("ID")
                                .help("The ID of the workflow to retrieve")
                                .value_hint(ValueHint::Other)
                                .required(true)
                                .index(1),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("create")
                        .about("Create a new workflow")
                        .arg(
                            Arg::new("file")
                                .short('f')
                                .long("file")
                                .value_name("FILE")
                                .help("The file to read the workflow data from, defaults to stdin")
                                .value_hint(ValueHint::FilePath)
                                .action(ArgAction::Set),
                        )
                        .args(&chain_args),
                )
                .subcommand(
                    Command::new("delete")
                        .about("Delete a workflow")
                        .args(&chain_args),
                ),
        )
        // Keygen subcommand
        .subcommand(
            Command::new("keygen")
                .about("Generate a new key")
                .arg(
                    Arg::new("file")
                        .short('f')
                        .long("file")
                        .value_name("FILE")
                        .help("The file to write the seed to, defaults to stdout")
                        .value_hint(ValueHint::FilePath)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("password")
                        .short('p')
                        .long("password")
                        .value_name("PASSWORD")
                        .help("Sets the password for the Gevulot client")
                        .value_hint(ValueHint::Other)
                        .action(ArgAction::Set)
                        .global(true),
                ),
        )
        .subcommand(
            Command::new("compute-key")
                .about("Compute a key")
                .arg(
                    Arg::new("mnemonic")
                        .long("mnemonic")
                        .value_name("MNEMONIC")
                        .env("GEVULOT_MNEMONIC")
                        .help("The mnemonic to compute the key from")
                        .required(true)
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("password")
                        .long("password")
                        .value_name("PASSWORD")
                        .help("The password to compute the key with")
                        .value_hint(ValueHint::Other),
                ),
        )
        // Send subcommand
        .subcommand(
            Command::new("send")
                .about("Send tokens to a receiver on the Gevulot network")
                .arg(
                    Arg::new("amount")
                        .value_name("AMOUNT")
                        .help("The amount of tokens to send")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("receiver")
                        .value_name("RECEIVER")
                        .help("The receiver address")
                        .required(true)
                        .index(2)
                        .value_hint(ValueHint::Other),
                )
                .args(&chain_args),
        )
        // Account-info subcommand
        .subcommand(
            Command::new("account-info")
                .about("Get the balance of the given account")
                .arg(
                    Arg::new("address")
                        .value_name("ADDRESS")
                        .help("The address to get the balance of")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                )
                .args(&chain_args),
        )
        .subcommand(
            Command::new("generate-completion")
                .about("Generate shell completion scripts")
                .arg(
                    Arg::new("shell")
                        .value_name("SHELL")
                        .help("The shell to generate the completion scripts for")
                        .required(true)
                        .action(ArgAction::Set)
                        .value_parser(value_parser!(clap_complete::Shell))
                        .index(1)
                        .value_hint(ValueHint::Other),
                )
                .arg(
                    Arg::new("file")
                        .short('f')
                        .long("file")
                        .value_name("FILE")
                        .help("The file to write the completion scripts to, defaults to stdout")
                        .action(ArgAction::Set)
                        .value_hint(ValueHint::FilePath),
                ),
        )
        .subcommand(commands::sudo::get_command())
        .subcommand(commands::build::get_command()))
}

/// Connects to the Gevulot network using the provided command-line arguments.
///
/// This function creates a GevulotClient based on the endpoint, gas price,
/// gas multiplier, and mnemonic provided in the command-line arguments.
///
/// # Arguments
///
/// * `matches` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing a GevulotClient if successful, or a Box<dyn std::error::Error> if an error occurs.
async fn connect_to_gevulot(
    matches: &clap::ArgMatches,
) -> Result<GevulotClient, Box<dyn std::error::Error>> {
    let mut client_builder = GevulotClientBuilder::default();

    // Set the endpoint if provided
    if let Some(endpoint) = matches.get_one::<String>("endpoint") {
        client_builder = client_builder.endpoint(endpoint);
    }

    // Set the gas price if provided
    if let Some(gas_price) = matches.get_one::<String>("gas_price") {
        client_builder = client_builder.gas_price(
            gas_price
                .parse()
                .map_err(|e| format!("Failed to parse gas_price: {}", e))?,
        );
    }

    // Set the gas multiplier if provided
    if let Some(gas_multiplier) = matches.get_one::<String>("gas_multiplier") {
        client_builder = client_builder.gas_multiplier(
            gas_multiplier
                .parse()
                .map_err(|e| format!("Failed to parse gas_multiplier: {}", e))?,
        );
    }

    // Set the mnemonic if provided
    if let Some(mnemonic) = matches.get_one::<String>("mnemonic") {
        client_builder = client_builder.mnemonic(mnemonic);
    }

    // Set the password if provided
    if let Some(password) = matches.get_one::<String>("password") {
        client_builder = client_builder.password(password);
    }

    // Build and return the client
    let client = client_builder.build().await?;

    Ok(client)
}

/// Reads and parses a file or stdin input into a specified type.
///
/// This function is generic over T, which must implement DeserializeOwned.
/// It reads from a file if specified in the command-line arguments,
/// otherwise it reads from stdin.
///
/// # Arguments
///
/// * `matches` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing the parsed value of type T if successful, or a Box<dyn std::error::Error> if an error occurs.
async fn read_file<T: DeserializeOwned>(
    matches: &clap::ArgMatches,
) -> Result<T, Box<dyn std::error::Error>> {
    let content = match matches.get_one::<String>("file") {
        Some(file) => {
            let mut file = File::open(file)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            contents
        }
        None => {
            let mut contents = String::new();
            io::stdin().read_to_string(&mut contents)?;
            contents
        }
    };
    let parsed: T = serde_yaml::from_str(&content)?;
    Ok(parsed)
}

/// Prints an object in the specified format.
///
/// This function takes a reference to command-line arguments and a serializable value,
/// and prints the value in the format specified by the user (yaml, json, prettyjson, or toml).
///
/// # Arguments
///
/// * `matches` - A reference to the ArgMatches struct containing parsed command-line arguments.
/// * `value` - A reference to the value to be printed, which must implement Serialize.
///
/// # Returns
///
/// A Result indicating success or an error if serialization or printing fails.
fn print_object<T: Serialize>(
    matches: &clap::ArgMatches,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get the format from command-line arguments, defaulting to "yaml" if not specified
    let format = matches
        .get_one::<String>("format")
        .expect("format has a default value");

    // Match on the format string and serialize/print accordingly
    match format.as_str() {
        "yaml" => {
            // Serialize to YAML and print
            let yaml = serde_yaml::to_string(value)?;
            println!("{}", yaml);
        }
        "json" => {
            // Serialize to compact JSON and print
            let json = serde_json::to_string(value)?;
            println!("{}", json);
        }
        "prettyjson" => {
            // Serialize to pretty-printed JSON and print
            let prettyjson = serde_json::to_string_pretty(value)?;
            println!("{}", prettyjson);
        }
        "toml" => {
            // Serialize to TOML and print
            let toml = toml::to_string(value)?;
            println!("{}", toml);
        }
        // If an unknown format is specified, print an error message
        _ => println!("Unknown format"),
    }

    Ok(())
}

/// Sends tokens to a receiver on the Gevulot network.
///
/// # Arguments
///
/// * `sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result indicating success or an error if the token transfer fails.
async fn send_tokens(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let client = connect_to_gevulot(_sub_m).await?;
    let amount = _sub_m.get_one::<String>("amount").unwrap();
    let receiver = _sub_m.get_one::<String>("receiver").unwrap();
    client
        .base_client
        .write()
        .await
        .token_transfer(receiver, amount.parse()?)
        .await?;
    Ok(())
}

/// Retrieves and displays account information for a given address.
///
/// # Arguments
///
/// * `sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///https://github.com/gevulotnetwork/platform/pull/60
/// # Returns
///
/// A Result indicating success or an error if the account information retrieval fails.
async fn account_info(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let client = connect_to_gevulot(_sub_m).await?;
    let address = _sub_m.get_one::<String>("address").unwrap();
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
    println!("Account number: {}", account.account_number);
    println!("Account sequence: {}", account.sequence);
    println!("Balance: {:#?}", balance.amount);
    Ok(())
}

/// Generates a new key and optionally saves it to a file.
async fn generate_key(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    // Generate random Mnemonic using the default language (English)
    let mnemonic = Mnemonic::random(OsRng, Default::default());
    let password = _sub_m
        .get_one::<String>("password")
        .cloned()
        .unwrap_or("".to_string());

    // Derive a BIP39 seed value using the given password
    let seed = mnemonic.to_seed(&password);

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

    println!("{}", sk.public_key().account_id("gvlt").unwrap());

    if let Some(file) = _sub_m.get_one::<String>("file") {
        let mut file = File::create(file)?;
        file.write_all(mnemonic.phrase().as_bytes())?;
    } else {
        println!("{}", mnemonic.phrase());
    }

    Ok(())
}

/// Generates a new key and optionally saves it to a file.
async fn compute_key(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mnemonic = Mnemonic::new(
        _sub_m.get_one::<String>("mnemonic").unwrap(),
        bip32::Language::English,
    )?;

    let password = _sub_m
        .get_one::<String>("password")
        .cloned()
        .unwrap_or("".to_string());

    // Derive a BIP39 seed value using the given password
    let seed = mnemonic.to_seed(&password);

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

    println!("{}", sk.public_key().account_id("gvlt").unwrap());

    Ok(())
}

/// Generates shell completion scripts for the gvltctl command-line tool.
///
/// This function generates shell completion scripts for the gvltctl command-line tool
/// and prints the results to the console.
///
/// # Arguments
///
/// * `sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result indicating success or an error if the completion generation fails.
async fn generate_completion(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(generator) = _sub_m.get_one::<Shell>("shell").copied() {
        let mut cmd = setup_command_line_args()?; // Assuming you have a Command::new() function
        eprintln!("Generating completion file for {generator}...");
        if let Some(file) = _sub_m.get_one::<String>("file") {
            let mut file = File::create(file)?;
            generate(generator, &mut cmd, "gvltctl", &mut file);
        } else {
            generate(generator, &mut cmd, "gvltctl", &mut io::stdout());
        }
    } else {
        eprintln!("No shell specified for completion generation");
    }
    Ok(())
}
async fn list_workflows(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    println!("Listing all workflows");
    todo!();
}

async fn get_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    println!("Getting a specific workflow");
    todo!();
}

async fn create_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating a new workflow");
    todo!();
}

async fn delete_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    println!("Deleting a workflow");
    todo!();
}
