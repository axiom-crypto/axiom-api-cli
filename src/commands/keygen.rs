use clap::{Args, Subcommand};
use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::{config, config::API_KEY_HEADER};

#[derive(Args, Debug)]
pub struct KeygenCmd {
    #[command(subcommand)]
    command: Option<KeygenSubcommand>,
}

#[derive(Debug, Subcommand)]
enum KeygenSubcommand {
    /// Download public key artifacts
    Download {
        /// The config ID to download public key for
        #[clap(long, value_name = "ID")]
        config_id: String,

        /// The type of key to download
        #[clap(long, value_parser = [
            "app_vm", 
            "leaf_vm", 
            "internal_vm", 
            "root_verifier", 
            "halo2_outer", 
            "halo2_wrapper"
        ])]
        key_type: String,
    },
}

impl KeygenCmd {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(KeygenSubcommand::Download {
                config_id,
                key_type,
            }) => download_key_artifact(config_id, key_type),
            None => Err(eyre::eyre!("A subcommand is required for keygen")),
        }
    }
}

fn download_key_artifact(config_id: String, key_type: String) -> Result<()> {
    // Load configuration
    let config = config::load_config()?;
    let url = format!("{}/configs/{}/pk/{}", config.api_url, config_id, key_type);

    println!(
        "Getting {} proving key for config ID: {}",
        key_type, config_id
    );

    // Make the GET request
    let client = Client::new();
    let api_key = config::get_api_key()?;

    let response = client
        .get(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send download request")?;

    // Check if the request was successful
    if response.status().is_success() {
        // Parse the response to get the download URL
        let response_json: Value = response.json()?;
        println!("{}", response_json);
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Download request failed with status: {}",
            response.status()
        ))
    }
}
