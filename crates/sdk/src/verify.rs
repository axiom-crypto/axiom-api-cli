use std::path::PathBuf;

use eyre::{Context, Result};
use openvm_sdk::types::EvmProof;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{get_config_id, AxiomSdk, API_KEY_HEADER};

pub trait VerifySdk {
    fn get_verification_result(&self, verify_id: &str) -> Result<VerifyStatus>;
    fn verify_proof(&self, config_id: Option<&str>, proof_path: PathBuf) -> Result<String>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyStatus {
    pub id: String,
    pub created_at: String,
    pub result: String,
}

impl VerifySdk for AxiomSdk {
    fn get_verification_result(&self, verify_id: &str) -> Result<VerifyStatus> {
        // Load configuration
        let url = format!("{}/verify/{}", self.config.api_url, verify_id);

        println!("Checking verification status for ID: {verify_id}");

        // Make the GET request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send status request")?;

        // Check if the request was successful
        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let verify_status = serde_json::from_value(response_json)?;
            Ok(verify_status)
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

    fn verify_proof(&self, config_id: Option<&str>, proof_path: PathBuf) -> Result<String> {
        // Load configuration
        let config_id = get_config_id(config_id, &self.config)?;
        let url = format!("{}/verify?config_id={}", self.config.api_url, config_id);

        println!("Verifying proof at {proof_path:?} with config ID: {config_id}");

        // Check if the proof file exists
        if !proof_path.exists() {
            eyre::bail!("Proof file does not exist: {:?}", proof_path);
        }

        let proof_content = std::fs::read_to_string(&proof_path)?;
        serde_json::from_str::<EvmProof>(&proof_content)
            .map_err(|e| eyre::eyre!("Invalid evm proof file: {}", e))?;

        // Create a multipart form
        let form = reqwest::blocking::multipart::Form::new()
            .file("proof", &proof_path)
            .context(format!("Failed to read proof file: {proof_path:?}"))?;

        // Make the POST request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .post(url)
            .header(API_KEY_HEADER, api_key)
            .multipart(form)
            .send()
            .context("Failed to send verification request")?;

        // Handle the response
        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let verify_id = response_json["id"].as_str().unwrap();
            println!("Verification request sent: {verify_id}");
            Ok(verify_id.to_string())
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
}
