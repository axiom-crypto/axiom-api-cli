use std::{fs::File, io::copy, path::PathBuf};

use clap::{Args, Subcommand};
use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::{
    get_api_key, get_config_id, load_config, validate_initialization, API_KEY_HEADER,
};

#[derive(Args, Debug)]
pub struct ConfigCmd {
    #[command(subcommand)]
    command: Option<ConfigSubcommand>,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    /// Download config artifacts: proving keys, evm verifier, leaf committed exe etc.
    Download {
        /// The config ID to download public key for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// The type of key to download
        #[clap(long, value_parser = [
            // These will give a download URL because the files are huge
            "app_vm",
            "leaf_vm",
            "internal_vm",
            "root_verifier",
            "halo2_outer",
            "halo2_wrapper",
            // These will download (stream) the file because they are small
            "config",
            "evm_verifier",
            "app_vm_commit",
        ])]
        key_type: String,

        /// Optional output file path (defaults to key_type name in current directory)
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    Status {
        /// The config ID to check status for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,
    },
}

impl ConfigCmd {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(ConfigSubcommand::Status { config_id }) => check_config_status(config_id),
            Some(ConfigSubcommand::Download {
                config_id,
                key_type,
                output,
            }) => {
                if key_type == "evm_verifier" || key_type == "app_vm_commit" || key_type == "config"
                {
                    // This is a small file, so we'll just download it directly
                    download_small_artifact(config_id, key_type, output)
                } else {
                    download_key_artifact(config_id, key_type)
                }
            }
            None => Err(eyre::eyre!("A subcommand is required for config")),
        }
    }
}

fn check_config_status(config_id: Option<String>) -> Result<()> {
    validate_initialization()?;
    let config = load_config()?;
    let config_id = get_config_id(config_id, &config)?;
    let url = format!("{}/configs/{}", config.api_url, config_id);

    println!("Checking status for config ID: {}", config_id);

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .get(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send status request")?;

    if response.status().is_success() {
        println!("Config status: {}", response.text()?);
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Config status request failed with status: {}",
            response.status()
        ))
    }
}

fn download_small_artifact(
    config_id: Option<String>,
    key_type: String,
    output: Option<PathBuf>,
) -> Result<()> {
    validate_initialization()?;
    // Load configuration
    let config = load_config()?;
    let config_id = get_config_id(config_id, &config)?;
    let url = format!("{}/configs/{}/{}", config.api_url, config_id, key_type);

    println!("Downloading {} for config ID: {}", key_type, config_id);

    // Determine output path
    let output_path = match output {
        Some(path) => path,
        None => {
            if key_type == "evm_verifier" {
                PathBuf::from(format!("./evm_verifier-{}.json", config_id))
            } else if key_type == "config" {
                PathBuf::from(format!("./config-{}.toml", config_id))
            } else {
                PathBuf::from(format!("./{}-{}", key_type, config_id))
            }
        }
    };

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .get(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send download request")?;

    // Check if the request was successful
    if response.status().is_success() {
        // Create the output file
        let mut file = File::create(&output_path)
            .context(format!("Failed to create output file: {:?}", output_path))?;

        // Stream the response body to the file
        copy(&mut response.bytes()?.as_ref(), &mut file)
            .context("Failed to write response to file")?;

        println!("Successfully downloaded to {:?}", output_path);
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

fn download_key_artifact(config_id: Option<String>, key_type: String) -> Result<()> {
    validate_initialization()?;
    // Load configuration
    let config = load_config()?;
    let config_id = get_config_id(config_id, &config)?;
    let url = format!("{}/configs/{}/pk/{}", config.api_url, config_id, key_type);

    println!(
        "Getting {} proving key for config ID: {}",
        key_type, config_id
    );

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

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
