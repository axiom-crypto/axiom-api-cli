use std::{fs, io::copy, path::PathBuf};

use cargo_openvm::input::{Input, is_valid_hex_string};
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    API_KEY_HEADER, AxiomSdk, authenticated_get, authenticated_post, download_file,
    send_request_json,
};

const PROOF_POLLING_INTERVAL_SECS: u64 = 10;

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
    fn save_proof_to_path(
        &self,
        proof_id: &str,
        proof_type: &str,
        output_path: PathBuf,
    ) -> Result<()>;
    fn save_proof_logs_to_path(&self, proof_id: &str, output_path: PathBuf) -> Result<()>;
    fn generate_new_proof(&self, args: ProveArgs) -> Result<String>;
    fn wait_for_proof_completion(&self, proof_id: &str) -> Result<()>;
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
    pub program_uuid: String,
    pub error_message: Option<String>,
    pub launched_at: Option<String>,
    pub terminated_at: Option<String>,
    pub created_by: String,
    pub cells_used: u64,
}

impl ProveSdk for AxiomSdk {
    fn list_proofs(&self, program_id: &str) -> Result<Vec<ProofStatus>> {
        let url = format!("{}/proofs?program_id={}", self.config.api_url, program_id);

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to list proofs")?;

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

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to check proof status")?;
        let proof_status = serde_json::from_value(body)?;
        Ok(proof_status)
    }

    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &str,
        output: Option<PathBuf>,
    ) -> Result<()> {
        // First get proof status to extract program_uuid
        let proof_status = self.get_proof_status(proof_id)?;

        let url = format!(
            "{}/proofs/{}/proof/{}",
            self.config.api_url, proof_id, proof_type
        );

        // Determine output file path
        let output_path = match output {
            Some(path) => path,
            None => {
                // Create organized directory structure using program_uuid from response
                let proof_dir = format!(
                    "axiom-artifacts/program-{}/proofs/{}",
                    proof_status.program_uuid, proof_id
                );
                std::fs::create_dir_all(&proof_dir)
                    .context(format!("Failed to create proof directory: {}", proof_dir))?;
                PathBuf::from(format!("{}/{}-proof.json", proof_dir, proof_type))
            }
        };

        let request = authenticated_get(&self.config, &url)?;
        download_file(request, &output_path, "Failed to download proof")?;
        println!("  ✓ {}", output_path.display());
        Ok(())
    }

    fn get_proof_logs(&self, proof_id: &str) -> Result<()> {
        // First get proof status to extract program_uuid
        let proof_status = self.get_proof_status(proof_id)?;

        let url = format!("{}/proofs/{}/logs", self.config.api_url, proof_id);

        // Create organized directory structure using program_uuid from response
        let proof_dir = format!(
            "axiom-artifacts/program-{}/proofs/{}",
            proof_status.program_uuid, proof_id
        );
        std::fs::create_dir_all(&proof_dir)
            .context(format!("Failed to create proof directory: {}", proof_dir))?;

        // Create file path in the proof directory
        let output_path = PathBuf::from(format!("{}/logs.txt", proof_dir));
        let request = authenticated_get(&self.config, &url)?;
        download_file(request, &output_path, "Failed to download proof logs")?;
        println!("  ✓ {}", output_path.display());
        Ok(())
    }

    fn save_proof_to_path(
        &self,
        proof_id: &str,
        proof_type: &str,
        output_path: PathBuf,
    ) -> Result<()> {
        let url = format!(
            "{}/proofs/{}/proof/{}",
            self.config.api_url, proof_id, proof_type
        );

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

        if response.status().is_success() {
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

            println!("  ✓ {}", output_path.display());
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Download failed ({}): {}", status, error_text))
        }
    }

    fn save_proof_logs_to_path(&self, proof_id: &str, output_path: PathBuf) -> Result<()> {
        let url = format!("{}/proofs/{}/logs", self.config.api_url, proof_id);

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

        if response.status().is_success() {
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

            println!("  ✓ {}", output_path.display());
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!(
                "Logs download failed ({}): {}",
                status,
                error_text
            ))
        }
    }

    fn generate_new_proof(&self, args: ProveArgs) -> Result<String> {
        // Get the program_id from args, return error if not provided
        let program_id = args
            .program_id
            .ok_or_eyre("Program ID is required. Use --program-id to specify.")?;

        let proof_type = args.proof_type.unwrap_or_else(|| "stark".to_string());

        use crate::formatting::Formatter;
        Formatter::print_header(&format!("Generating {} proof", proof_type.to_uppercase()));
        Formatter::print_field("Program ID", &program_id);

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

        let response_json: Value = send_request_json(request, "Failed to generate proof")?;
        let proof_id = response_json["id"].as_str().unwrap();

        Formatter::print_success(&format!("Proof generation initiated ({})", proof_id));
        Ok(proof_id.to_string())
    }

    fn wait_for_proof_completion(&self, proof_id: &str) -> Result<()> {
        use crate::formatting::{Formatter, calculate_duration};
        use std::time::Duration;

        println!();

        loop {
            // Get status without printing repetitive messages
            let url = format!("{}/proofs/{}", self.config.api_url, proof_id);
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

            let proof_status: ProofStatus = if response.status().is_success() {
                let body: Value = response.json()?;
                serde_json::from_value(body)?
            } else {
                return Err(eyre::eyre!(
                    "Failed to get proof status: {}",
                    response.status()
                ));
            };

            match proof_status.state.as_str() {
                "Succeeded" => {
                    Formatter::clear_line_and_reset();
                    Formatter::print_success("Proof generation completed successfully!");

                    // Print completion information
                    Formatter::print_section("Proof Summary");
                    Formatter::print_field("Program ID", &proof_status.program_uuid);
                    Formatter::print_field("Proof ID", &proof_status.id);
                    Formatter::print_field("Usage", &format!("{} cells", proof_status.cells_used));

                    if let Some(launched_at) = &proof_status.launched_at {
                        if let Some(terminated_at) = &proof_status.terminated_at {
                            Formatter::print_section("Job Stats");
                            Formatter::print_field("Created", &proof_status.created_at);
                            Formatter::print_field("Initiated", launched_at);
                            Formatter::print_field("Finished", terminated_at);

                            if let Ok(duration) = calculate_duration(launched_at, terminated_at) {
                                Formatter::print_field("Duration", &duration);
                            }
                        }
                    }

                    // Download artifacts automatically
                    Formatter::print_section("Downloading Artifacts");

                    // Download the specific proof type that was generated
                    let proof_type_name = match proof_status.proof_type.as_str() {
                        "stark" => "STARK",
                        "evm" => "EVM",
                        _ => "Unknown",
                    };
                    Formatter::print_info(&format!("Downloading {} proof...", proof_type_name));

                    // Create organized directory structure using program_uuid
                    let proof_dir = format!(
                        "axiom-artifacts/program-{}/proofs/{}",
                        proof_status.program_uuid, proof_status.id
                    );
                    if let Err(e) = std::fs::create_dir_all(&proof_dir) {
                        println!("Warning: Failed to create proof directory: {}", e);
                    } else {
                        let proof_path = PathBuf::from(format!(
                            "{}/{}-proof.json",
                            proof_dir, proof_status.proof_type
                        ));
                        if let Err(e) = self.save_proof_to_path(
                            &proof_status.id,
                            &proof_status.proof_type,
                            proof_path,
                        ) {
                            println!(
                                "Warning: Failed to download {} proof: {}",
                                proof_type_name, e
                            );
                        }

                        // Download logs
                        Formatter::print_info("Downloading logs...");
                        let logs_path = PathBuf::from(format!("{}/logs.txt", proof_dir));
                        if let Err(e) = self.save_proof_logs_to_path(&proof_status.id, logs_path) {
                            println!("Warning: Failed to download logs: {}", e);
                        }
                    }

                    return Ok(());
                }
                "Failed" => {
                    Formatter::clear_line_and_reset();
                    let error_msg = proof_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Proof generation failed: {}", error_msg);
                }
                "Queued" => {
                    Formatter::print_status("Proof queued...");
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "Executing" => {
                    Formatter::print_status("Executing program...");
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "Executed" => {
                    Formatter::print_status("Program executed, preparing proof...");
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                _ => {
                    Formatter::print_status(&format!("Proof status: {}...", proof_status.state));
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
            }
        }
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
