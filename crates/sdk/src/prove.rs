use std::{fs, io::copy, path::PathBuf};

use bytes::Bytes;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    API_KEY_HEADER, AxiomSdk, ProgressCallback, ProofType, add_cli_version_header,
    authenticated_get, authenticated_post, download_file, input::Input, send_request_json,
};

const PROOF_POLLING_INTERVAL_SECS: u64 = 10;

pub trait ProveSdk {
    fn list_proofs(&self, program_id: &str) -> Result<Vec<ProofStatus>>;
    fn get_proof_status(&self, proof_id: &str) -> Result<ProofStatus>;
    fn get_proof_logs(&self, proof_id: &str) -> Result<()>;
    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &ProofType,
        output: Option<PathBuf>,
    ) -> Result<Bytes>;
    fn save_proof_logs_to_path(&self, proof_id: &str, output_path: PathBuf) -> Result<()>;
    fn generate_new_proof(&self, args: ProveArgs) -> Result<String>;
    fn wait_for_proof_completion(&self, proof_id: &str, save: bool) -> Result<ProofStatus>;
    fn cancel_proof(&self, proof_id: &str) -> Result<String>;
    fn wait_for_proof_cancellation(&self, proof_id: &str) -> Result<()>;
}

#[derive(Debug)]
pub struct ProveArgs {
    /// The ID of the program to generate a proof for
    pub program_id: Option<String>,
    /// Input data for the proof (file path or hex string)
    pub input: Option<Input>,
    /// The type of proof to generate (stark or evm)
    pub proof_type: Option<ProofType>,
    /// The num gpus to use for this proof (1-10000)
    pub num_gpus: Option<usize>,
    /// Priority for this proof (1-10, higher = more priority)
    pub priority: Option<u8>,
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
    pub num_instructions: Option<u64>,
    pub num_gpus: usize,
    pub priority: u8,
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

    fn get_proof_logs(&self, proof_id: &str) -> Result<()> {
        // First get proof status to extract program_uuid
        let proof_status = self.get_proof_status(proof_id)?;

        let url = format!("{}/proofs/{}/logs", self.config.api_url, proof_id);

        // Create organized directory structure using program_uuid from response
        let proof_dir = format!(
            "axiom-artifacts/program-{}/proofs/{}",
            proof_status.program_uuid, proof_id
        );
        fs::create_dir_all(&proof_dir)
            .context(format!("Failed to create proof directory: {}", proof_dir))?;

        // Create file path in the proof directory
        let output_path = PathBuf::from(format!("{}/logs.txt", proof_dir));
        let request = authenticated_get(&self.config, &url)?;
        download_file(
            request,
            output_path.clone().into(),
            "Failed to download proof logs",
        )?;
        self.callback
            .on_success(&format!("{}", output_path.display()));
        Ok(())
    }

    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &ProofType,
        output: Option<PathBuf>,
    ) -> Result<Bytes> {
        let url = format!(
            "{}/proofs/{}/proof/{}",
            self.config.api_url, proof_id, proof_type
        );

        let request = authenticated_get(&self.config, &url)?;
        let proof = download_file(request, output.clone(), "Failed to download proof")?;
        if let Some(output_path) = &output {
            self.callback
                .on_success(&format!("{}", output_path.display()));
        }
        Ok(proof)
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

    fn wait_for_proof_completion(&self, proof_id: &str, save: bool) -> Result<ProofStatus> {
        self.wait_for_proof_completion_base(proof_id, save, &*self.callback)
    }

    fn cancel_proof(&self, proof_id: &str) -> Result<String> {
        let url = format!("{}/proofs/{}/cancel", self.config.api_url, proof_id);

        let request = authenticated_post(&self.config, &url)?
            .header("Content-Type", "application/json")
            .body("{}");

        let response = request.send().context("Failed to send cancel request")?;

        if response.status().is_success() {
            // Try to get response message, fallback to default
            let response_text = response.text().unwrap_or_else(|_| "{}".to_string());
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_text) {
                if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                    return Ok(message.to_string());
                }
            }
            Ok("Cancellation request submitted successfully".to_string())
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            eyre::bail!("Failed to cancel proof ({}): {}", status, error_text);
        }
    }

    fn wait_for_proof_cancellation(&self, proof_id: &str) -> Result<()> {
        self.wait_for_proof_cancellation_base(proof_id, &*self.callback)
    }
}

impl AxiomSdk {
    pub fn generate_new_proof_base(
        &self,
        args: ProveArgs,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        // Get the program_id from args, return error if not provided
        let program_id = args
            .program_id
            .ok_or_eyre("Program ID is required. Use --program-id to specify.")?;

        let proof_type = args.proof_type.unwrap_or(ProofType::Stark);

        callback.on_header("Generating Proof");
        callback.on_field("Program ID", &program_id);
        callback.on_field("Proof Type", &proof_type.to_string().to_uppercase());

        if let Some(num_gpus) = args.num_gpus {
            callback.on_field("Num GPUs", &num_gpus.to_string());
        }

        if let Some(priority) = args.priority {
            callback.on_field("Priority", &priority.to_string());
        }

        let mut url = format!(
            "{}/proofs?program_id={program_id}&proof_type={proof_type}",
            self.config.api_url
        );

        // Add optional parameters as query parameters
        if let Some(num_gpus) = args.num_gpus {
            url.push_str(&format!("&num_gpus={}", num_gpus));
        }

        if let Some(priority) = args.priority {
            url.push_str(&format!("&priority={}", priority));
        }

        // Create the request body based on input
        let body = match &args.input {
            Some(input) => input.to_input_json()?,
            None => json!({ "input": [] }),
        };

        // Make API request using authenticated_post helper
        let request = authenticated_post(&self.config, &url)?
            .header("Content-Type", "application/json")
            .body(body.to_string());

        let response_json: Value = send_request_json(request, "Failed to generate proof")?;
        let proof_id = response_json["id"].as_str().unwrap();

        callback.on_success(&format!("Proof generation initiated ({})", proof_id));
        Ok(proof_id.to_string())
    }

    pub fn wait_for_proof_completion_base(
        &self,
        proof_id: &str,
        save: bool,
        callback: &dyn ProgressCallback,
    ) -> Result<ProofStatus> {
        use std::time::Duration;

        let mut spinner_started = false;

        loop {
            let response = authenticated_get(
                &self.config,
                &format!("{}/proofs/{}", self.config.api_url, proof_id),
            )?;
            let proof_status: ProofStatus =
                send_request_json(response, "Failed to get proof status")?;

            match proof_status.state.as_str() {
                "Succeeded" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Proof generation completed successfully!");
                    } else {
                        callback.on_success("Proof generation completed successfully!");
                    }

                    // Add spacing before sections
                    println!();

                    // Match the detailed status format
                    callback.on_section("Proof Status");
                    callback.on_field("ID", &proof_status.id);
                    callback.on_field("State", &proof_status.state);
                    callback.on_field("Proof Type", &proof_status.proof_type);
                    callback.on_field("Program ID", &proof_status.program_uuid);
                    callback.on_field("Created By", &proof_status.created_by);
                    callback.on_field("Created At", &proof_status.created_at);

                    if let Some(launched_at) = &proof_status.launched_at {
                        callback.on_field("Launched At", launched_at);
                    }

                    if let Some(terminated_at) = &proof_status.terminated_at {
                        callback.on_field("Terminated At", terminated_at);
                    }

                    if let Some(error_message) = &proof_status.error_message {
                        callback.on_field("Error", error_message);
                    }

                    callback.on_section("Configuration");
                    callback.on_field("Num GPUs", &proof_status.num_gpus.to_string());
                    callback.on_field("Priority", &proof_status.priority.to_string());

                    callback.on_section("Statistics");
                    callback.on_field("Cells Used", &proof_status.cells_used.to_string());
                    if let Some(num_instructions) = proof_status.num_instructions {
                        callback.on_field("Total Cycles", &num_instructions.to_string());
                    }

                    if save {
                        // Add spacing after statistics and add saving section
                        callback.on_section("Saving Results");

                        // Use same directory structure as download: program-{uuid}/proofs/{proof_id}/
                        let proof_dir = PathBuf::from("axiom-artifacts")
                            .join(format!("program-{}", proof_status.program_uuid))
                            .join("proofs")
                            .join(&proof_status.id);
                        if let Err(e) = fs::create_dir_all(&proof_dir) {
                            callback.on_warning(&format!(
                                "Failed to create directory {}: {}",
                                proof_dir.display(),
                                e
                            ));
                        } else {
                            // Use same naming convention as download: {proof_type}-proof.json
                            let proof_path =
                                proof_dir.join(format!("{}-proof.json", proof_status.proof_type));
                            match self.get_generated_proof(
                                &proof_status.id,
                                &proof_status.proof_type.parse()?,
                                Some(proof_path.clone()),
                            ) {
                                Ok(_) => {
                                    callback.on_success(&format!(
                                        "{} proof saved to {}",
                                        proof_status.proof_type.to_uppercase(),
                                        proof_path.display()
                                    ));
                                }
                                Err(e) => {
                                    callback.on_warning(&format!("Failed to save proof: {}", e));
                                }
                            }

                            let logs_path = proof_dir.join("logs.txt");
                            match self.save_proof_logs_to_path(&proof_status.id, logs_path.clone())
                            {
                                Ok(_) => {
                                    callback.on_success(&format!(
                                        "Logs saved to {}",
                                        logs_path.display()
                                    ));
                                }
                                Err(e) => {
                                    callback.on_warning(&format!("Failed to save logs: {}", e));
                                }
                            }
                        }
                    }

                    return Ok(proof_status);
                }
                "Failed" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    let error_msg = proof_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Proof generation failed: {}", error_msg);
                }
                "Canceled" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Proof generation was canceled");
                    } else {
                        callback.on_info("Proof generation was canceled");
                    }
                    return Ok(proof_status);
                }
                "Canceling" => {
                    if !spinner_started {
                        callback.on_progress_start("Canceling proof", None);
                        spinner_started = true;
                    } else {
                        callback.on_progress_update_message("Canceling proof");
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "Queued" => {
                    if !spinner_started {
                        callback.on_progress_start("Proof queued", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "InProgress" => {
                    if !spinner_started {
                        callback.on_progress_start("Generating proof", None);
                        spinner_started = true;
                    } else {
                        // Update message if we were previously in queued state
                        callback.on_progress_update_message("Generating proof");
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                _ => {
                    let status_message = format!("Proof status: {}", proof_status.state);
                    if !spinner_started {
                        callback.on_progress_start(&status_message, None);
                        spinner_started = true;
                    } else {
                        callback.on_progress_update_message(&status_message);
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
            }
        }
    }

    pub fn wait_for_proof_cancellation_base(
        &self,
        proof_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        use std::time::Duration;

        let mut spinner_started = false;

        loop {
            let response = authenticated_get(
                &self.config,
                &format!("{}/proofs/{}", self.config.api_url, proof_id),
            )?;
            let proof_status: ProofStatus =
                send_request_json(response, "Failed to get proof status")?;

            match proof_status.state.as_str() {
                "Canceled" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Proof successfully canceled");
                    } else {
                        callback.on_success("Proof successfully canceled");
                    }
                    return Ok(());
                }
                "Canceling" => {
                    if !spinner_started {
                        callback.on_progress_start("Canceling proof", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
                "Failed" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    let error_msg = proof_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!(
                        "Proof failed before cancellation could complete: {}",
                        error_msg
                    );
                }
                "Succeeded" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    eyre::bail!(
                        "Proof completed successfully before cancellation could take effect"
                    );
                }
                _ => {
                    // For any other state (Queued, InProgress, etc.), keep waiting for cancellation
                    if !spinner_started {
                        callback.on_progress_start("Waiting for cancellation", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(PROOF_POLLING_INTERVAL_SECS));
                }
            }
        }
    }
}
