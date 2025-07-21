use std::{fs, path::PathBuf};

use cargo_openvm::input::{is_valid_hex_string, Input};
use eyre::{Context, OptionExt, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{authenticated_get, authenticated_post, download_file, send_request_json, AxiomSdk};

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
    /// The type of proof to generate (stark or evm)
    pub proof_type: Option<String>,
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

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to send list proofs request")?;

        // Extract the items array from the response
        if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
            if items.is_empty() {
                println!("No proofs found for program ID: {program_id}");
                return Ok(vec![]);
            }

            let mut proofs = vec![];

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

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to send status request")?;
        let proof_status = serde_json::from_value(body)?;
        Ok(proof_status)
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

        // Determine output file path
        let output_path = match output {
            Some(path) => path,
            None => PathBuf::from(format!("{proof_id}-{proof_type}-proof.json")),
        };

        let request = authenticated_get(&self.config, &url)?;
        download_file(request, &output_path, "Failed to send download request")
    }

    fn get_proof_logs(&self, proof_id: &str) -> Result<()> {
        let url = format!("{}/proofs/{}/logs", self.config.api_url, proof_id);

        println!("Downloading logs for proof ID: {proof_id}");

        let output_path = PathBuf::from(format!("{proof_id}-logs.txt"));
        let request = authenticated_get(&self.config, &url)?;
        download_file(request, &output_path, "Failed to send logs request")
    }

    fn generate_new_proof(&self, args: ProveArgs) -> Result<String> {
        // Get the program_id from args, return error if not provided
        let program_id = args
            .program_id
            .ok_or_eyre("Program ID is required. Use --program-id to specify.")?;

        let proof_type = args.proof_type.unwrap_or_else(|| "stark".to_string());

        println!("Generating {proof_type} proof for program ID: {program_id}");

        let url = format!(
            "{}/proofs?program_id={program_id}&proof_type={proof_type}",
            self.config.api_url
        );

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
                    eyre::bail!("Hex string must start with '01' or '02'");
                }
                json!({ "input": [s] })
            }
            None => json!({ "input": [] }),
        };

        // Make API request
        let request = authenticated_post(&self.config, &url)?
            .header("Content-Type", "application/json")
            .body(body.to_string());

        let response_json: Value = send_request_json(request, "Failed to send proof request")?;
        let proof_id = response_json["id"].as_str().unwrap();
        println!("Proof generation initiated successfully!: {proof_id}");
        Ok(proof_id.to_string())
    }
}

fn validate_input_json(json: &serde_json::Value) -> Result<()> {
    json["input"]
        .as_array()
        .ok_or_eyre("Input must be an array under 'input' key")?
        .iter()
        .try_for_each(|inner| {
            inner
                .as_str()
                .ok_or_eyre("Each value must be a hex string")
                .and_then(|s| {
                    if !is_valid_hex_string(s) {
                        eyre::bail!("Invalid hex string");
                    }
                    if !s.trim_start_matches("0x").starts_with("01")
                        && !s.trim_start_matches("0x").starts_with("02")
                    {
                        eyre::bail!("Hex string must start with '01' or '02'");
                    }
                    Ok(())
                })
        })?;
    Ok(())
}
