use std::path::PathBuf;

use clap::{Args, Subcommand};
use eyre::{Context, Result};
use openvm_sdk::types::EvmProof;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::{get_api_key, get_config_id, load_config, API_KEY_HEADER};

#[derive(Args, Debug)]
pub struct VerifyCmd {
    #[command(subcommand)]
    command: Option<VerifySubcommand>,

    /// The config ID to use for verification
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,

    /// Path to the proof file
    #[clap(long, value_name = "FILE")]
    proof: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum VerifySubcommand {
    /// Check the status of a verification
    Status {
        /// The verification ID to check status for
        #[clap(long, value_name = "ID")]
        verify_id: String,
    },
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(VerifySubcommand::Status { verify_id }) => check_verify_status(verify_id),
            None => {
                let proof = self.proof.ok_or_else(|| {
                    eyre::eyre!("Proof file is required. Use --proof to specify.")
                })?;

                verify_proof(self.config_id, proof)
            }
        }
    }
}

fn verify_proof(config_id: Option<String>, proof_path: PathBuf) -> Result<()> {
    config::validate_initialization()?;
    
    // Load configuration
    let config = load_config()?;
    let config_id = get_config_id(config_id, &config)?;
    let url = format!("{}/verify?config_id={}", config.api_url, config_id);

    println!(
        "Verifying proof at {:?} with config ID: {}",
        proof_path, config_id
    );

    // Check if the proof file exists
    if !proof_path.exists() {
        return Err(eyre::eyre!("Proof file does not exist: {:?}", proof_path));
    }

    let proof_content = std::fs::read_to_string(&proof_path)?;
    serde_json::from_str::<EvmProof>(&proof_content)
        .map_err(|e| eyre::eyre!("Invalid evm proof file: {}", e))?;

    // Create a multipart form
    let form = reqwest::blocking::multipart::Form::new()
        .file("proof", &proof_path)
        .context(format!("Failed to read proof file: {:?}", proof_path))?;

    // Make the POST request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .post(url)
        .header(API_KEY_HEADER, api_key)
        .multipart(form)
        .send()
        .context("Failed to send verification request")?;

    // Handle the response
    if response.status().is_success() {
        let response_json: Value = response.json()?;
        println!("Verification request sent: {}", response_json);
        println!(
            "To check the verification status, run: cargo axiom verify status --verify-id {}",
            response_json["id"]
        );
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Verification request failed with status: {}",
            response.status()
        ))
    }
}

fn check_verify_status(verify_id: String) -> Result<()> {
    // Load configuration
    let config = load_config()?;
    let url = format!("{}/verify/{}", config.api_url, verify_id);

    println!("Checking verification status for ID: {}", verify_id);

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send status request")?;

    // Check if the request was successful
    if response.status().is_success() {
        let response_json: Value = response.json()?;
        println!("Verification status: {}", response_json);
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Status request failed with status: {}",
            response.status()
        ))
    }
}
