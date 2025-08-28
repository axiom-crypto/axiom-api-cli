use std::{fs, io::copy, path::PathBuf};

use cargo_openvm::input::Input;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::validate_input_json;
use crate::{
    API_KEY_HEADER, AxiomSdk, ProgressCallback, add_cli_version_header, authenticated_get,
    calculate_duration, download_file, send_request_json,
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

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
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

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
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
        self.generate_new_proof_base(args, &*self.callback)
    }

    fn wait_for_proof_completion(&self, proof_id: &str) -> Result<()> {
        self.wait_for_proof_completion_base(proof_id, &*self.callback)
    }
}

impl AxiomSdk {
    pub fn generate_new_proof_base(
        &self,
        args: ProveArgs,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        let program_id = args.program_id;

        callback.on_header("Generating Proof");
        callback.on_field("Program ID", program_id.as_deref().unwrap_or("N/A"));
        if let Some(ref proof_type) = args.proof_type {
            callback.on_field("Proof Type", proof_type);
        }

        if let Some(Input::FilePath(path)) = &args.input {
            let file_content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read input file: {:?}", path))?;
            let input_json: serde_json::Value = serde_json::from_str(&file_content)
                .with_context(|| format!("Failed to parse input JSON from file: {:?}", path))?;
            validate_input_json(&input_json)?;
        }

        let url = format!("{}/proofs", self.config.api_url);

        let body = match args.input {
            Some(input) => match input {
                Input::FilePath(path) => {
                    let file_content = std::fs::read_to_string(&path)?;
                    let input_json: serde_json::Value = serde_json::from_str(&file_content)?;
                    json!({
                        "program_id": program_id,
                        "input": input_json,
                        "proof_type": args.proof_type
                    })
                }
                Input::HexBytes(hex_bytes) => {
                    json!({
                        "program_id": program_id,
                        "input": hex_bytes,
                        "proof_type": args.proof_type
                    })
                }
            },
            None => json!({
                "program_id": program_id,
                "proof_type": args.proof_type
            }),
        };

        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&url)
            .header(API_KEY_HEADER, api_key)
            .json(&body)
            .send()
            .context("Failed to send proof generation request")?;

        if response.status().is_success() {
            let response_json: serde_json::Value = response.json()?;
            let proof_id = response_json["id"]
                .as_str()
                .ok_or_eyre("Missing 'id' field in proof response")?
                .to_string();

            callback.on_success(&format!("Proof generation initiated ({})", proof_id));

            Ok(proof_id)
        } else {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            eyre::bail!("Failed to generate proof: {}", error_text);
        }
    }

    pub fn wait_for_proof_completion_base(
        &self,
        proof_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        use std::time::Duration;

        loop {
            let response = authenticated_get(
                &self.config,
                &format!("{}/proofs/{}", self.config.api_url, proof_id),
            )?;
            let proof_status: ProofStatus =
                send_request_json(response, "Failed to get proof status")?;

            match proof_status.state.as_str() {
                "Succeeded" => {
                    callback.on_clear_line_and_reset();
                    callback.on_success("Proof generation completed successfully!");

                    callback.on_section("Proof Summary");
                    callback.on_field("Proof ID", &proof_status.id);
                    callback.on_field("Program ID", &proof_status.program_uuid);

                    let proof_type_name = match proof_status.proof_type.as_str() {
                        "stark" => "STARK",
                        "evm" => "EVM",
                        _ => &proof_status.proof_type,
                    };
                    callback.on_field("Proof Type", proof_type_name);

                    let proof_dir = format!(
                        "axiom-artifacts/program-{}/proofs",
                        proof_status.program_uuid
                    );
                    std::fs::create_dir_all(&proof_dir).ok();

                    if proof_status.proof_type == "stark" {
                        let proof_path = format!("{}/{}.stark", proof_dir, proof_status.id);
                        if self
                            .save_proof_to_path(
                                &proof_status.id,
                                &proof_status.proof_type,
                                std::path::PathBuf::from(&proof_path),
                            )
                            .is_ok()
                        {
                            callback.on_success(&format!("✓ STARK proof saved to {}", proof_path));
                        }
                    } else {
                        let proof_type_name = match proof_status.proof_type.as_str() {
                            "evm" => "evm",
                            _ => &proof_status.proof_type,
                        };
                        let proof_path =
                            format!("{}/{}.{}", proof_dir, proof_status.id, proof_type_name);
                        if self
                            .save_proof_to_path(
                                &proof_status.id,
                                &proof_status.proof_type,
                                std::path::PathBuf::from(&proof_path),
                            )
                            .is_ok()
                        {
                            callback.on_success(&format!(
                                "✓ {} proof saved to {}",
                                proof_type_name.to_uppercase(),
                                proof_path
                            ));
                        }
                    }

                    let logs_path = format!("{}/logs.txt", proof_dir);
                    if self
                        .save_proof_logs_to_path(
                            &proof_status.id,
                            std::path::PathBuf::from(&logs_path),
                        )
                        .is_ok()
                    {
                        callback.on_success(&format!("✓ Logs saved to {}", logs_path));
                    }

                    let created_at = &proof_status.created_at;
                    if let Some(terminated_at) = &proof_status.terminated_at {
                        callback.on_section("Proof Stats");
                        callback.on_field("Created", created_at);
                        callback.on_field("Finished", terminated_at);

                        if let Ok(duration) = calculate_duration(created_at, terminated_at) {
                            callback.on_field("Duration", &duration);
                        }
                    }

                    return Ok(());
                }
                "Failed" => {
                    callback.on_clear_line_and_reset();
                    let error_msg = proof_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Proof generation failed: {}", error_msg);
                }
                "Queued" => {
                    callback.on_status("Proof queued...");
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "InProgress" => {
                    callback.on_status("Generating proof...");
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                _ => {
                    callback.on_status(&format!("Proof status: {}...", proof_status.state));
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
            }
        }
    }
}
