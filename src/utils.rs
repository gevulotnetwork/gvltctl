//! Common gevulot utilities.

use clap::ValueEnum as _;
use gevulot_rs::gevulot_client::GevulotClientBuilder;
use gevulot_rs::GevulotClient;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::commands::ChainArgs;

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
/// A Result containing a GevulotClient if successful, or a `Box<dyn std::error::Error>` if an error occurs.
pub async fn connect_to_gevulot(
    chain_args: &ChainArgs,
) -> Result<GevulotClient, Box<dyn std::error::Error>> {
    let mut client_builder = GevulotClientBuilder::default();

    // Set the endpoint if provided
    if let Some(endpoint) = &chain_args.endpoint {
        client_builder = client_builder.endpoint(endpoint);
    }

    // Set the gas price if provided
    if let Some(gas_price) = chain_args.gas_price {
        client_builder = client_builder.gas_price(gas_price);
    }

    // Set the gas multiplier if provided
    if let Some(gas_multiplier) = chain_args.gas_multiplier {
        client_builder = client_builder.gas_multiplier(gas_multiplier);
    }

    // Set the mnemonic if provided
    if let Some(mnemonic) = &chain_args.mnemonic {
        client_builder = client_builder.mnemonic(mnemonic);
    }

    // Set the private key if provided
    if let Some(private_key) = &chain_args.private_key {
        client_builder = client_builder.private_key(private_key);
    }

    // Set the password if provided
    if let Some(password) = &chain_args.password {
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
/// A Result containing the parsed value of type T if successful, or a `Box<dyn std::error::Error>` if an error occurs.
pub async fn read_file<T: DeserializeOwned>(
    path: Option<&Path>,
) -> Result<T, Box<dyn std::error::Error>> {
    let content = match path {
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

/// Possible output formats for Gevulot Control.
#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    Yaml,
    Json,
    PrettyJson,
    Toml,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Yaml
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.to_possible_value()
                .expect("no skipped values")
                .get_name()
        )
    }
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
pub fn print_object<T: Serialize>(
    format: OutputFormat,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    // Match on the format string and serialize/print accordingly
    match format {
        OutputFormat::Yaml => {
            // Serialize to YAML and print
            let yaml = serde_yaml::to_string(value)?;
            println!("{}", yaml);
        }
        OutputFormat::Json => {
            // Serialize to compact JSON and print
            let json = serde_json::to_string(value)?;
            println!("{}", json);
        }
        OutputFormat::PrettyJson => {
            // Serialize to pretty-printed JSON and print
            let prettyjson = serde_json::to_string_pretty(value)?;
            println!("{}", prettyjson);
        }
        OutputFormat::Toml => {
            // Serialize to TOML and print
            let toml = toml::to_string(value)?;
            println!("{}", toml);
        }
    }
    Ok(())
}
