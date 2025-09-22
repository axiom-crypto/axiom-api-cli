use std::path::PathBuf;

use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{API_KEY_HEADER, AxiomSdk, ProgressCallback, add_cli_version_header, get_config_id};

const VERIFICATION_POLLING_INTERVAL_SECS: u64 = 10;

pub trait VerifySdk {
    fn get_evm_verification_result(&self, verify_id: &str) -> Result<VerifyStatus>;
    fn get_stark_verification_result(&self, verify_id: &str) -> Result<VerifyStatus>;
    /// Get verification result for either EVM or STARK proofs - the backend automatically detects the type
    fn get_verification_result(&self, verify_id: &str) -> Result<VerifyStatus>;
    fn verify_evm(&self, config_id: Option<&str>, proof_path: PathBuf) -> Result<String>;
    fn verify_stark(&self, program_id: &str, proof_path: PathBuf) -> Result<String>;
    fn wait_for_evm_verify_completion(&self, verify_id: &str) -> Result<()>;
    fn wait_for_stark_verify_completion(&self, verify_id: &str) -> Result<()>;
    /// Wait for verification completion for either EVM or STARK proofs
    fn wait_for_verify_completion(&self, verify_id: &str) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyStatus {
    pub id: String,
    pub created_at: String,
    pub result: String,
    pub proof_type: String,
}

impl VerifySdk for AxiomSdk {
    fn get_evm_verification_result(&self, verify_id: &str) -> Result<VerifyStatus> {
        // Use unified endpoint - the backend automatically detects EVM vs STARK
        let url = format!("{}/verify/{}", self.config.api_url, verify_id);
        self.get_verification_status(&url)
    }

    fn get_stark_verification_result(&self, verify_id: &str) -> Result<VerifyStatus> {
        // Use unified endpoint - the backend automatically detects EVM vs STARK
        let url = format!("{}/verify/{}", self.config.api_url, verify_id);
        self.get_verification_status(&url)
    }

    fn get_verification_result(&self, verify_id: &str) -> Result<VerifyStatus> {
        let url = format!("{}/verify/{}", self.config.api_url, verify_id);
        self.get_verification_status(&url)
    }

    fn verify_evm(&self, config_id: Option<&str>, proof_path: PathBuf) -> Result<String> {
        self.verify_evm_base(config_id, proof_path, &*self.callback)
    }

    fn verify_stark(&self, program_id: &str, proof_path: PathBuf) -> Result<String> {
        self.verify_stark_base(program_id, proof_path, &*self.callback)
    }

    fn wait_for_evm_verify_completion(&self, verify_id: &str) -> Result<()> {
        self.wait_for_evm_verify_completion_base(verify_id, &*self.callback)
    }

    fn wait_for_stark_verify_completion(&self, verify_id: &str) -> Result<()> {
        self.wait_for_stark_verify_completion_base(verify_id, &*self.callback)
    }

    fn wait_for_verify_completion(&self, verify_id: &str) -> Result<()> {
        self.wait_for_verify_completion_base(verify_id, &*self.callback)
    }
}

impl AxiomSdk {
    pub fn verify_evm_base(
        &self,
        config_id: Option<&str>,
        proof_path: PathBuf,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        use crate::config::ConfigSdk;

        // Check if the proof file exists
        if !proof_path.exists() {
            eyre::bail!("Proof file does not exist: {:?}", proof_path);
        }

        // Get config_id, using default if not provided
        let config_id = get_config_id(config_id, &self.config)?;

        // Parse and validate the EVM proof file
        let proof_content = std::fs::read_to_string(&proof_path)?;
        let proof_content = proof_content.replace("0x", "");
        let proof_json: Value = serde_json::from_str(&proof_content)
            .map_err(|e| eyre::eyre!("Invalid JSON in proof file: {}", e))?;

        // Basic schema validation
        proof_json
            .get("version")
            .ok_or_eyre("Missing 'version' field in proof")?;
        proof_json
            .get("user_public_values")
            .ok_or_eyre("Missing 'user_public_values' field")?;
        proof_json
            .get("app_exe_commit")
            .ok_or_eyre("Missing 'app_exe_commit' field")?;
        proof_json
            .get("app_vm_commit")
            .ok_or_eyre("Missing 'app_vm_commit' field")?;
        proof_json
            .get("proof_data")
            .ok_or_eyre("Missing 'proof_data' field")?;
        let proof_data = proof_json.get("proof_data").unwrap();
        proof_data
            .get("accumulator")
            .ok_or_eyre("Missing 'accumulator' in proof_data")?;
        proof_data
            .get("proof")
            .ok_or_eyre("Missing 'proof' in proof_data")?;

        // Get config metadata for additional information
        let config_metadata = self.get_vm_config_metadata(Some(&config_id))?;

        // Print information about what we're verifying
        callback.on_header("EVM Proof Verification");
        callback.on_field("Proof File", &proof_path.display().to_string());
        callback.on_field("Config ID", &config_id);
        callback.on_field("OpenVM Version", &config_metadata.openvm_version);

        let url = format!("{}/verify?config_id={}", self.config.api_url, config_id);
        self.submit_verification_request(&url, &proof_path, callback)
    }

    pub fn verify_stark_base(
        &self,
        program_id: &str,
        proof_path: PathBuf,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        // Check if the proof file exists
        if !proof_path.exists() {
            eyre::bail!("Proof file does not exist: {:?}", proof_path);
        }

        // Print information about what we're verifying
        callback.on_header("STARK Proof Verification");
        callback.on_field("Proof File", &proof_path.display().to_string());
        callback.on_field("Program ID", program_id);

        let url = format!(
            "{}/verify/stark?program_id={}",
            self.config.api_url, program_id
        );
        self.submit_verification_request(&url, &proof_path, callback)
    }

    pub fn wait_for_evm_verify_completion_base(
        &self,
        verify_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        self.wait_for_verification_completion(
            || self.get_evm_verification_result(verify_id),
            callback,
        )
    }

    pub fn wait_for_stark_verify_completion_base(
        &self,
        verify_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        self.wait_for_verification_completion(
            || self.get_stark_verification_result(verify_id),
            callback,
        )
    }
    /// Common helper function to get verification status from any URL
    fn get_verification_status(&self, url: &str) -> Result<VerifyStatus> {
        // Make the GET request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
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

    /// Common helper function to submit verification requests
    fn submit_verification_request(
        &self,
        url: &str,
        proof_path: &std::path::Path,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        callback.on_info("Initiating verification...");

        // Read and process the proof file content to remove 0x prefixes
        let proof_content = std::fs::read_to_string(proof_path)
            .context(format!("Failed to read proof file: {proof_path:?}"))?;
        let processed_content = proof_content.replace("0x", "");

        // Create a multipart form with the processed content as a file
        let form = reqwest::blocking::multipart::Form::new().part(
            "proof",
            reqwest::blocking::multipart::Part::text(processed_content)
                .file_name("proof.json")
                .mime_str("application/json")?,
        );

        // Make the POST request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(
            client
                .post(url)
                .header(API_KEY_HEADER, api_key)
                .multipart(form),
        )
        .send()
        .context("Failed to send verification request")?;

        // Handle the response
        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let verify_id = response_json["id"]
                .as_str()
                .ok_or_eyre("Missing 'id' field in verification response")?;
            callback.on_success(&format!("Verification request sent: {verify_id}"));
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

    /// Common helper function for waiting for verification completion
    fn wait_for_verification_completion<F>(
        &self,
        get_status: F,
        callback: &dyn ProgressCallback,
    ) -> Result<()>
    where
        F: Fn() -> Result<VerifyStatus>,
    {
        use std::time::Duration;

        let mut spinner_started = false;

        loop {
            let verify_status = get_status()?;

            match verify_status.result.as_str() {
                "verified" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Verification completed successfully!");
                    } else {
                        callback.on_success("Verification completed successfully!");
                    }

                    // Add spacing before sections
                    println!();

                    // Match the CLI status format - use Summary and correct field labels
                    callback.on_section("Verification Summary");
                    match verify_status.result.as_str() {
                        "verified" => callback.on_field("Verification Result", "✓ VERIFIED"),
                        "failed" => callback.on_field("Verification Result", "✗ FAILED"),
                        _ => callback
                            .on_field("Verification Result", &verify_status.result.to_uppercase()),
                    }
                    callback.on_field("Verification ID", &verify_status.id);
                    callback.on_field("Proof Type", &verify_status.proof_type.to_uppercase());
                    callback.on_field("Created At", &verify_status.created_at);

                    return Ok(());
                }
                "failed" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    callback.on_error("Verification failed!");

                    // Match the CLI status format - use Summary and correct field labels
                    callback.on_section("Verification Summary");
                    match verify_status.result.as_str() {
                        "verified" => callback.on_field("Verification Result", "✓ VERIFIED"),
                        "failed" => callback.on_field("Verification Result", "✗ FAILED"),
                        _ => callback
                            .on_field("Verification Result", &verify_status.result.to_uppercase()),
                    }
                    callback.on_field("Verification ID", &verify_status.id);
                    callback.on_field("Proof Type", &verify_status.proof_type.to_uppercase());
                    callback.on_field("Created At", &verify_status.created_at);

                    eyre::bail!("Proof verification failed");
                }
                "processing" => {
                    if !spinner_started {
                        callback.on_progress_start("Verifying proof", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
                _ => {
                    let status_message = format!("Verification status: {}", verify_status.result);
                    if !spinner_started {
                        callback.on_progress_start(&status_message, None);
                        spinner_started = true;
                    } else {
                        callback.on_progress_update_message(&status_message);
                    }
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
            }
        }
    }

    /// Unified wait for verification completion that works for both EVM and STARK proofs
    pub fn wait_for_verify_completion_base(
        &self,
        verify_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        use std::time::Duration;

        let mut spinner_started = false;

        loop {
            let verify_status = self.get_verification_result(verify_id)?;

            match verify_status.result.as_str() {
                "verified" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Verification completed successfully!");
                    } else {
                        callback.on_success("Verification completed successfully!");
                    }

                    // Add spacing before sections
                    println!();

                    // Match the CLI status format - use Summary and correct field labels
                    callback.on_section("Verification Summary");
                    match verify_status.result.as_str() {
                        "verified" => callback.on_field("Verification Result", "✓ VERIFIED"),
                        "failed" => callback.on_field("Verification Result", "✗ FAILED"),
                        _ => callback
                            .on_field("Verification Result", &verify_status.result.to_uppercase()),
                    }
                    callback.on_field("Verification ID", &verify_status.id);
                    callback.on_field("Proof Type", &verify_status.proof_type.to_uppercase());
                    callback.on_field("Created At", &verify_status.created_at);

                    return Ok(());
                }
                "failed" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    callback.on_error("Verification failed!");

                    // Match the CLI status format - use Summary and correct field labels
                    callback.on_section("Verification Summary");
                    match verify_status.result.as_str() {
                        "verified" => callback.on_field("Verification Result", "✓ VERIFIED"),
                        "failed" => callback.on_field("Verification Result", "✗ FAILED"),
                        _ => callback
                            .on_field("Verification Result", &verify_status.result.to_uppercase()),
                    }
                    callback.on_field("Verification ID", &verify_status.id);
                    callback.on_field("Proof Type", &verify_status.proof_type.to_uppercase());
                    callback.on_field("Created At", &verify_status.created_at);

                    eyre::bail!("Proof verification failed");
                }
                "processing" => {
                    if !spinner_started {
                        callback.on_progress_start("Verifying proof", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
                _ => {
                    let status_message = format!("Verification status: {}", verify_status.result);
                    if !spinner_started {
                        callback.on_progress_start(&status_message, None);
                        spinner_started = true;
                    } else {
                        callback.on_progress_update_message(&status_message);
                    }
                    std::thread::sleep(Duration::from_secs(VERIFICATION_POLLING_INTERVAL_SECS));
                }
            }
        }
    }
}
