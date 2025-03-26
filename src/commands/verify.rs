use std::path::PathBuf;

use clap::Args;
use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::{config, config::API_KEY_HEADER};

#[derive(Args, Debug)]
pub struct VerifyCmd {
    /// The config ID to use for verification
    #[clap(long, value_name = "ID")]
    config_id: String,

    /// Path to the proof file
    #[clap(long, value_name = "FILE")]
    proof: PathBuf,
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        verify_proof(self.config_id, self.proof)
    }
}

fn verify_proof(config_id: String, proof_path: PathBuf) -> Result<()> {
    // Load configuration
    let config = config::load_config()?;
    let url = format!("{}/verify?config_id={}", config.api_url, config_id);

    println!(
        "Verifying proof at {:?} with config ID: {}",
        proof_path, config_id
    );

    // Check if the proof file exists
    if !proof_path.exists() {
        return Err(eyre::eyre!("Proof file does not exist: {:?}", proof_path));
    }

    // Create a multipart form
    let form = reqwest::blocking::multipart::Form::new()
        .file("proof", &proof_path)
        .context(format!("Failed to read proof file: {:?}", proof_path))?;

    // Make the POST request
    let client = Client::new();
    let api_key = config::get_api_key()?;

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
