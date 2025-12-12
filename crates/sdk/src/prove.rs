use std::{fs, io::copy, path::PathBuf};

use crate::input::Input;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    API_KEY_HEADER, AxiomSdk, ProgressCallback, ProofType, add_cli_version_header,
    authenticated_get, authenticated_post, download_file, send_request_json, validate_input_json,
};

const PROOF_POLLING_INTERVAL_SECS: u64 = 10;

pub trait ProveSdk {
    fn list_proofs(
        &self,
        program_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProofListResponse>;
    fn get_proof_status(&self, proof_id: &str) -> Result<ProofStatus>;
    fn get_generated_proof(
        &self,
        proof_id: &str,
        proof_type: &ProofType,
        output: Option<PathBuf>,
    ) -> Result<()>;
    fn get_proof_logs(&self, proof_id: &str) -> Result<()>;
    fn save_proof_to_path(
        &self,
        proof_id: &str,
        proof_type: &ProofType,
        output_path: PathBuf,
    ) -> Result<()>;
    fn save_proof_logs_to_path(&self, proof_id: &str, output_path: PathBuf) -> Result<()>;
    fn generate_new_proof(&self, args: ProveArgs) -> Result<String>;
    fn wait_for_proof_completion(&self, proof_id: &str) -> Result<()>;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ProofListResponse {
    pub items: Vec<ProofStatus>,
    pub pagination: ProofPaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProofPaginationInfo {
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub pages: u32,
}

impl ProveSdk for AxiomSdk {
    fn list_proofs(
        &self,
        program_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ProofListResponse> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let url = format!(
            "{}/proofs?program_id={}&page={}&page_size={}",
            self.config.api_url, program_id, page, page_size
        );

        let request = authenticated_get(&self.config, &url)?;
        send_request_json(request, "Failed to list proofs")
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
        proof_type: &ProofType,
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
        self.callback
            .on_success(&format!("{}", output_path.display()));
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
        self.callback
            .on_success(&format!("{}", output_path.display()));
        Ok(())
    }

    fn save_proof_to_path(
        &self,
        proof_id: &str,
        proof_type: &ProofType,
        output_path: PathBuf,
    ) -> Result<()> {
        let url = format!(
            "{}/proofs/{proof_id}/proof/{proof_type}",
            self.config.api_url,
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

    fn cancel_proof(&self, proof_id: &str) -> Result<String> {
        let url = format!("{}/proofs/{}/cancel", self.config.api_url, proof_id);

        let request = authenticated_post(&self.config, &url)?
            .header("Content-Type", "application/json")
            .body("{}");

        let response = request.send().context("Failed to send cancel request")?;

        if response.status().is_success() {
            // Try to get response message, fallback to default
            let response_text = response.text().unwrap_or_else(|_| "{}".to_string());
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_text)
                && let Some(message) = json.get("message").and_then(|m| m.as_str())
            {
                return Ok(message.to_string());
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
                if !matches!(s.first(), Some(x) if x == &0x01 || x == &0x02) {
                    eyre::bail!(
                        "Hex string must start with '01'(bytes) or '02'(field elements). See the OpenVM book for more details. https://docs.openvm.dev/book/writing-apps/overview/#inputs"
                    );
                }
                let hex_string = format!("0x{}", hex::encode(s));
                json!({ "input": [hex_string] })
            }
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

                    // Add spacing after statistics and add saving section
                    callback.on_section("Saving Results");

                    // Use same directory structure as download: program-{uuid}/proofs/{proof_id}/
                    let proof_dir = format!(
                        "axiom-artifacts/program-{}/proofs/{}",
                        proof_status.program_uuid, proof_status.id
                    );
                    std::fs::create_dir_all(&proof_dir).ok();

                    // Use same naming convention as download: {proof_type}-proof.json
                    let proof_path =
                        format!("{}/{}-proof.json", proof_dir, proof_status.proof_type);
                    if self
                        .save_proof_to_path(
                            &proof_status.id,
                            &proof_status.proof_type.parse()?,
                            std::path::PathBuf::from(&proof_path),
                        )
                        .is_ok()
                    {
                        callback.on_success(&format!(
                            "{} proof saved to {}",
                            proof_status.proof_type.to_uppercase(),
                            proof_path
                        ));
                    }

                    let logs_path = format!("{}/logs.txt", proof_dir);
                    if self
                        .save_proof_logs_to_path(
                            &proof_status.id,
                            std::path::PathBuf::from(&logs_path),
                        )
                        .is_ok()
                    {
                        callback.on_success(&format!("Logs saved to {}", logs_path));
                    }

                    return Ok(());
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
                    return Ok(());
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
