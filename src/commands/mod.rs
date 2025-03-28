//! Gevulot Control commands definition.

/// Arguments for chain-related commands.
#[derive(Clone, Debug, clap::Args)]
pub struct ChainArgs {
    /// Sets the endpoint for the Gevulot client.
    #[arg(
        global = true,
        long,
        short = 'e',
        env = "GEVULOT_ENDPOINT",
        value_name = "URL",
        value_hint = clap::ValueHint::Url,
    )]
    pub endpoint: Option<String>,

    /// Sets the chain ID for the Gevulot client.
    #[arg(
        global = true,
        long = "chain-id",
        short = 'c',
        env = "CHAIN_ID",
        value_name = "CHAIN_ID"
    )]
    pub chain_id: Option<String>,

    /// Sets the gas price for the Gevulot client.
    #[arg(
        global = true,
        long = "gas-price",
        short = 'g',
        env = "GEVULOT_GAS_PRICE",
        value_name = "PRICE"
    )]
    pub gas_price: Option<f64>,

    /// Sets the gas multiplier for the Gevulot client.
    #[arg(
        global = true,
        long = "gas-multiplier",
        short = 'm',
        env = "GEVULOT_GAS_MULTIPLIER",
        value_name = "MULTIPLIER"
    )]
    pub gas_multiplier: Option<f64>,

    /// Sets the mnemonic for the Gevulot client.
    #[arg(
        global = true,
        long,
        short = 'n',
        env = "GEVULOT_MNEMONIC",
        hide_env_values = true
    )]
    pub mnemonic: Option<String>,

    /// Sets the password for the Gevulot client.
    #[arg(
        global = true,
        long,
        short = 'p',
        env = "GEVULOT_PASSWORD",
        hide_env_values = true
    )]
    pub password: Option<String>,
}

pub mod build;
pub mod local_run;
pub mod pins;
pub mod sudo;
pub mod tasks;
pub mod workers;
pub mod workflow;
