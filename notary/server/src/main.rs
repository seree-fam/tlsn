use std::env;

use eyre::{eyre, Result};
use structopt::StructOpt;
use tracing::debug;

use notary_server::{
    init_tracing, parse_config_file, run_server, CliFields, NotaryServerError,
    NotaryServerProperties,
};

#[tokio::main]
async fn main() -> Result<(), NotaryServerError> {
    // Load command line arguments which contains the config file location
    let cli_fields: CliFields = CliFields::from_args();
    let mut config: NotaryServerProperties = parse_config_file(&cli_fields.config_file)?;

    if let Ok(port_str) = env::var("PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            config.server.port = port;
        } else {
            eprintln!(
                "Warning: Invalid PORT environment variable value. Using default from config."
            );
        }
    }

    // Set up tracing for logging
    init_tracing(&config).map_err(|err| eyre!("Failed to set up tracing: {err}"))?;

    debug!(?config, "Server config loaded");

    // Run the server
    run_server(&config).await?;

    Ok(())
}
