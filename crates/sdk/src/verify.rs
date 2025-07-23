use std::path::PathBuf;

use eyre::{Context, OptionExt, Result};
use openvm_sdk::types::EvmProof;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{get_config_id, AxiomSdk, API_KEY_HEADER};

const VERIFICATION_POLLING_INTERVAL_SECS: u64 = 10;

pub trait VerifySdk {
    fn get_verification_result(&self, verify_id: &str) -> Result<VerifyStatus>;
    fn verify_proof(&self, config_id: Option<&str>, proof_path: PathBuf) -> Result<String>;
    fn wait_for_verify_completion(&self, verify_id: &str) -> Result<()>;
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

        // Make the GET request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

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
        use crate::config::ConfigSdk;
        use crate::formatting::Formatter;

        // Load configuration
        let config_id = get_config_id(config_id, &self.config)?;
        let url = format!("{}/verify?config_id={}", self.config.api_url, config_id);

        // Check if the proof file exists
        if !proof_path.exists() {
            eyre::bail!("Proof file does not exist: {:?}", proof_path);
        }

        // Parse and validate the proof file
        let proof_content = std::fs::read_to_string(&proof_path)?;
        let _proof: EvmProof = serde_json::from_str(&proof_content)
            .map_err(|e| eyre::eyre!("Invalid evm proof file: {}", e))?;

        // Get config metadata for additional information
        let config_metadata = self.get_vm_config_metadata(Some(&config_id))?;

        // Print information about what we're verifying
        Formatter::print_header("Proof Verification");
        Formatter::print_field("Proof File", &proof_path.display().to_string());
        Formatter::print_field("Config ID", &config_id);
        Formatter::print_field("OpenVM Version", &config_metadata.openvm_version);

        println!("\nInitiating verification...");

        // Create a multipart form
        let form = reqwest::blocking::multipart::Form::new()
            .file("proof", &proof_path)
            .context(format!("Failed to read proof file: {proof_path:?}"))?;

        // Make the POST request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

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
            Formatter::print_success(&format!("Verification request sent: {verify_id}"));
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

    fn wait_for_verify_completion(&self, verify_id: &str) -> Result<()> {
        use crate::formatting::Formatter;
        use std::time::Duration;

        loop {
            // Get status without printing repetitive messages
            let url = format!("{}/verify/{}", self.config.api_url, verify_id);
            let api_key = self
                .config
                .api_key
                .as_ref()
                .ok_or(eyre::eyre!("API key not set"))?;

            let response = Client::new()
                .get(url)
                .header(API_KEY_HEADER, api_key)
                .send()
                .context("Failed to send status request")?;

            let verify_status: VerifyStatus = if response.status().is_success() {
                let response_json: Value = response.json()?;
                serde_json::from_value(response_json)?
            } else {
                return Err(eyre::eyre!(
                    "Failed to get verification status: {}",
                    response.status()
                ));
            };

            match verify_status.result.as_str() {
                "verified" => {
                    Formatter::clear_line();
                    Formatter::print_success("Verification completed successfully!");

                    // Print completion information
                    Formatter::print_section("Verification Summary");
                    Formatter::print_field("Verification Result", "✓ VERIFIED");
                    Formatter::print_field("Verification ID", &verify_status.id);
                    Formatter::print_field("Completed At", &verify_status.created_at);

                    return Ok(());
                }
                "failed" => {
                    Formatter::clear_line();
                    println!("\nVerification failed!");

                    // Print failure information
                    Formatter::print_section("Verification Summary");
                    Formatter::print_field("Verification Result", "✗ FAILED");
                    Formatter::print_field("Verification ID", &verify_status.id);
                    Formatter::print_field("Completed At", &verify_status.created_at);

                    eyre::bail!("Proof verification failed");
                }
                "processing" => {
                    Formatter::print_status("Verifying proof...");
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
                _ => {
                    Formatter::print_status(&format!(
                        "Verification status: {}...",
                        verify_status.result
                    ));
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
            }
        }
    }
}
