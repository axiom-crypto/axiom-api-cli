use std::{fs, io::copy, path::PathBuf};

use cargo_openvm::input::{is_valid_hex_string, Input};
use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{AxiomSdk, API_KEY_HEADER};

pub trait ProveSdk {
    fn list_proofs(&self, program_id: &str) -> Result<Vec<ProofStatus>>;
    fn get_proof_status(&self, proof_id: &str) -> Result<ProofStatus>;
    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &str,
        output: Option<PathBuf>,
    ) -> Result<()>;
    fn get_proof_logs(&self, proof_id: &str) -> Result<()>;
    fn generate_new_proof(&self, args: ProveArgs) -> Result<String>;
}

#[derive(Debug)]
pub struct ProveArgs {
    /// The ID of the program to generate a proof for
    pub program_id: Option<String>,
    /// Input data for the proof (file path or hex string)
    pub input: Option<Input>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProofStatus {
    pub id: String,
    pub created_at: String,
    pub state: String,
    pub proof_type: String,
    pub error_message: Option<String>,
    pub launched_at: Option<String>,
    pub terminated_at: Option<String>,
    pub created_by: String,
    pub cells_used: u64,
    pub machine_type: String,
}

impl ProveSdk for AxiomSdk {
    fn list_proofs(&self, program_id: &str) -> Result<Vec<ProofStatus>> {
        let url = format!("{}/proofs?program_id={}", self.config.api_url, program_id);
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = Client::new()
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()?;

        let body: Value = response.json()?;

        // Extract the items array from the response
        if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
            if items.is_empty() {
                println!("No proofs found for program ID: {program_id}");
                return Ok(vec![]);
            }

            let mut proofs = vec![];

            // Add rows to the table
            for item in items {
                let proof_status = serde_json::from_value(item.clone())?;
                proofs.push(proof_status);
            }

            Ok(proofs)
        } else {
            Err(eyre::eyre!("Unexpected response format: {}", body))
        }
    }

    fn get_proof_status(&self, proof_id: &str) -> Result<ProofStatus> {
        let url = format!("{}/proofs/{}", self.config.api_url, proof_id);

        println!("Checking proof status for proof ID: {proof_id}");

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
            let body: Value = response.json()?;
            let proof_status = serde_json::from_value(body)?;
            Ok(proof_status)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            println!("Cannot check proof status for this proof: {error_text}");
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Status request failed with status: {}",
                response.status()
            ))
        }
    }

    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &str,
        output: Option<PathBuf>,
    ) -> Result<()> {
        let url = format!(
            "{}/proofs/{}/proof/{}",
            self.config.api_url, proof_id, proof_type
        );

        println!("Downloading {proof_type} proof for proof ID: {proof_id}");

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
            .context("Failed to send download request")?;

        // Check if the request was successful
        if response.status().is_success() {
            // Determine output file path
            let output_path = match output {
                Some(path) => path,
                None => PathBuf::from(format!("{proof_id}-{proof_type}-proof.json")),
            };

            // Create file and stream the response body to it
            let mut file = fs::File::create(&output_path)
                .context(format!("Failed to create output file: {output_path:?}"))?;

            copy(
                &mut response
                    .bytes()
                    .context("Failed to read response body")?
                    .as_ref(),
                &mut file,
            )
            .context("Failed to write response to file")?;

            println!("Successfully downloaded to: {output_path:?}");
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

    fn get_proof_logs(&self, proof_id: &str) -> Result<()> {
        let url = format!("{}/proofs/{}/logs", self.config.api_url, proof_id);

        println!("Downloading logs for proof ID: {proof_id}");

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
            .context("Failed to send logs request")?;

        // Check if the request was successful
        if response.status().is_success() {
            // Create file and stream the response body to it
            let output_path = PathBuf::from(format!("{proof_id}-logs.txt"));
            let mut file = fs::File::create(&output_path)
                .context(format!("Failed to create output file: {output_path:?}"))?;

            copy(
                &mut response
                    .bytes()
                    .context("Failed to read response body")?
                    .as_ref(),
                &mut file,
            )
            .context("Failed to write response to file")?;

            println!("Successfully downloaded logs to: {output_path:?}");
            Ok(())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            println!("Cannot download logs for this proof: {error_text}");
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Logs request failed with status: {}",
                response.status()
            ))
        }
    }

    fn generate_new_proof(&self, args: ProveArgs) -> Result<String> {
        // Get the program_id from args, return error if not provided
        let program_id = args
            .program_id
            .ok_or_else(|| eyre::eyre!("Program ID is required. Use --program-id to specify."))?;

        println!("Generating proof for program ID: {program_id}");

        let url = format!("{}/proofs?program_id={}", self.config.api_url, program_id);
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        // Create the request body based on input
        let body = match &args.input {
            Some(Input::FilePath(path)) => {
                let file_content = fs::read_to_string(path)
                    .context(format!("Failed to read input file: {}", path.display()))?;
                let input_json = serde_json::from_str(&file_content).context(format!(
                    "Failed to parse input file as JSON: {}",
                    path.display()
                ))?;
                validate_input_json(&input_json)?;
                input_json
            }
            Some(Input::HexBytes(s)) => {
                let trimmed = s.trim_start_matches("0x");
                if !trimmed.starts_with("01") && !trimmed.starts_with("02") {
                    return Err(eyre::eyre!("Hex string must start with '01' or '02'"));
                }
                json!({ "input": [s] })
            }
            None => json!({ "input": [] }),
        };

        // Make API request
        let client = Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header(API_KEY_HEADER, api_key)
            .body(body.to_string())
            .send()
            .context("Failed to send proof request")?;

        // Handle response
        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let proof_id = response_json["id"].as_str().unwrap();
            println!("Proof generation initiated successfully!: {proof_id}");
            Ok(proof_id.to_string())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            println!("Cannot generate proof for this program: {error_text}");
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            let status = response.status();
            Err(eyre::eyre!(
                "Generate proof request failed with status: {}",
                status
            ))
        }
    }
}

fn validate_input_json(json: &serde_json::Value) -> Result<()> {
    json["input"]
        .as_array()
        .ok_or_else(|| eyre::eyre!("Input must be an array under 'input' key"))?
        .iter()
        .try_for_each(|inner| {
            inner
                .as_str()
                .ok_or_else(|| eyre::eyre!("Each value must be a hex string"))
                .and_then(|s| {
                    if !is_valid_hex_string(s) {
                        return Err(eyre::eyre!("Invalid hex string"));
                    }
                    if !s.trim_start_matches("0x").starts_with("01")
                        && !s.trim_start_matches("0x").starts_with("02")
                    {
                        return Err(eyre::eyre!("Hex string must start with '01' or '02'"));
                    }
                    Ok(())
                })
        })?;
    Ok(())
}
