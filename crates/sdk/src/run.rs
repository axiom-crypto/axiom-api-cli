use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{API_KEY_HEADER, AxiomSdk, ProgressCallback, add_cli_version_header, input::Input};

const EXECUTION_POLLING_INTERVAL_SECS: u64 = 10;

pub trait RunSdk {
    fn get_execution_status(&self, execution_id: &str) -> Result<ExecutionStatus>;
    fn execute_program(&self, args: RunArgs) -> Result<String>;
    fn wait_for_execution_completion(&self, execution_id: &str) -> Result<()>;
    fn save_execution_results(&self, execution_status: &ExecutionStatus) -> Option<String>;
    fn list_executions(
        &self,
        program_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ExecutionListResponse>;
    fn get_execution_logs(&self, execution_id: &str) -> Result<()>;
}

#[derive(Debug)]
pub struct RunArgs {
    pub program_id: Option<String>,
    pub input: Option<Input>,
    pub mode: String,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            program_id: None,
            input: None,
            mode: "pure".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionStatus {
    pub id: String,
    pub created_at: String,
    pub status: String,
    pub program_uuid: String,
    pub error_message: Option<String>,
    pub launched_at: Option<String>,
    pub terminated_at: Option<String>,
    pub created_by: String,
    pub mode: String,
    pub public_values: Option<Value>,
    pub cost: Option<u64>,
    pub num_segments: Option<usize>,
    pub total_cycle: Option<u64>,
    pub total_tick: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionListResponse {
    pub items: Vec<ExecutionStatus>,
    pub pagination: ExecutionPaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionPaginationInfo {
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub pages: u32,
}

impl RunSdk for AxiomSdk {
    fn get_execution_status(&self, execution_id: &str) -> Result<ExecutionStatus> {
        let url = format!("{}/executions/{}", self.config.api_url, execution_id);
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
            .send()
            .context("Failed to send status request")?;

        if response.status().is_success() {
            let body: Value = response.json()?;
            let execution_status = serde_json::from_value(body)?;
            Ok(execution_status)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!(
                "Cannot check execution status: {} (status: {})",
                error_text,
                status
            ))
        } else {
            Err(eyre::eyre!(
                "Status request failed with status: {}",
                response.status()
            ))
        }
    }

    fn execute_program(&self, args: RunArgs) -> Result<String> {
        self.execute_program_base(args, &*self.callback)
    }

    fn wait_for_execution_completion(&self, execution_id: &str) -> Result<()> {
        self.wait_for_execution_completion_base(execution_id, &*self.callback)
    }

    fn save_execution_results(&self, execution_status: &ExecutionStatus) -> Option<String> {
        let run_dir = format!(
            "axiom-artifacts/program-{}/runs/{}",
            execution_status.program_uuid, execution_status.id
        );

        if std::fs::create_dir_all(&run_dir).is_err() {
            return None;
        }

        let results_path = format!("{}/results.json", run_dir);
        let results = serde_json::json!({
            "execution_id": execution_status.id,
            "created_at": execution_status.created_at,
            "launched_at": execution_status.launched_at,
            "terminated_at": execution_status.terminated_at,
            "mode": execution_status.mode,
            "total_cycles": execution_status.total_cycle,
            "total_ticks": execution_status.total_tick,
            "cost": execution_status.cost,
            "num_segments": execution_status.num_segments,
            "public_values": execution_status.public_values
        });

        if let Ok(results_json) = serde_json::to_string_pretty(&results)
            && std::fs::write(&results_path, results_json).is_ok()
        {
            return Some(results_path);
        }

        None
    }

    fn list_executions(
        &self,
        program_id: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<ExecutionListResponse> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let url = format!(
            "{}/executions?program_id={}&page={}&page_size={}",
            self.config.api_url, program_id, page, page_size
        );
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
            .send()
            .context("Failed to send list request")?;

        if response.status().is_success() {
            let body: ExecutionListResponse = response.json()?;
            Ok(body)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!(
                "Cannot list executions: {} (status: {})",
                error_text,
                status
            ))
        } else {
            Err(eyre::eyre!(
                "List request failed with status: {}",
                response.status()
            ))
        }
    }

    fn get_execution_logs(&self, execution_id: &str) -> Result<()> {
        use crate::download_file;

        let url = format!("{}/executions/{}/logs", self.config.api_url, execution_id);
        let request = crate::authenticated_get(&self.config, &url)?;

        let execution_dir =
            std::path::PathBuf::from("axiom-artifacts").join(format!("execution-{}", execution_id));
        std::fs::create_dir_all(&execution_dir).context(format!(
            "Failed to create execution directory: {}",
            execution_dir.display()
        ))?;

        let filename = execution_dir.join("logs.txt");
        download_file(
            request,
            Some(filename.clone()),
            "Failed to download execution logs",
        )?;
        self.callback
            .on_success(&format!("✓ {}", filename.display()));
        Ok(())
    }
}

impl AxiomSdk {
    pub fn execute_program_base(
        &self,
        args: RunArgs,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        let program_id = args
            .program_id
            .ok_or_eyre("Program ID is required. Use --program-id to specify.")?;

        callback.on_header("Executing Program");
        callback.on_field("Program ID", &program_id);

        let url = format!("{}/executions", self.config.api_url);
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        // Create the request body based on input
        let body = match &args.input {
            Some(input) => input.to_input_json()?,
            None => json!({ "input": [] }), // Empty JSON if no input provided
        };

        // Make API request
        let client = Client::new();
        let mut url_with_params = url::Url::parse(&url)?;
        url_with_params
            .query_pairs_mut()
            .append_pair("program_id", &program_id)
            .append_pair("mode", &args.mode);

        let response = add_cli_version_header(
            client
                .post(url_with_params)
                .header("Content-Type", "application/json")
                .header(API_KEY_HEADER, api_key)
                .body(body.to_string()),
        )
        .send()
        .context("Failed to send execution request")?;

        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let execution_id = response_json["id"]
                .as_str()
                .ok_or_eyre("Missing 'id' field in execution response")?;
            callback.on_success(&format!("Execution initiated ({})", execution_id));
            Ok(execution_id.to_string())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Failed to read error response".to_string());

            if status == 400 {
                eyre::bail!("Bad request: {}", error_text);
            } else if status == 401 {
                eyre::bail!("Unauthorized: Please check your API key");
            } else if status == 404 {
                eyre::bail!("Program not found: {}", program_id);
            } else {
                eyre::bail!("Client error {}: {}", status, error_text);
            }
        } else {
            eyre::bail!("Server error: {}", response.status());
        }
    }

    pub fn wait_for_execution_completion_base(
        &self,
        execution_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        use std::time::Duration;

        let mut spinner_started = false;

        loop {
            let execution_status = self.get_execution_status(execution_id)?;

            match execution_status.status.as_str() {
                "Succeeded" => {
                    if spinner_started {
                        callback.on_progress_finish("✓ Execution completed successfully!");
                    } else {
                        callback.on_success("Execution completed successfully!");
                    }

                    // Add spacing before sections
                    println!();

                    // Match the detailed status format
                    callback.on_section("Execution Status");
                    callback.on_field("ID", &execution_status.id);
                    callback.on_field("Status", &execution_status.status);
                    callback.on_field("Mode", &execution_status.mode);
                    callback.on_field("Program ID", &execution_status.program_uuid);
                    callback.on_field("Created By", &execution_status.created_by);
                    callback.on_field("Created At", &execution_status.created_at);

                    if let Some(launched_at) = &execution_status.launched_at {
                        callback.on_field("Launched At", launched_at);
                    }

                    if let Some(terminated_at) = &execution_status.terminated_at {
                        callback.on_field("Terminated At", terminated_at);
                    }

                    if let Some(error_message) = &execution_status.error_message {
                        callback.on_field("Error", error_message);
                    }

                    // Show mode-specific statistics
                    let mut has_stats = false;
                    match execution_status.mode.as_str() {
                        "meter" => {
                            if execution_status.cost.is_some()
                                || execution_status.total_cycle.is_some()
                            {
                                callback.on_section("Execution Statistics");
                                has_stats = true;
                            }
                            if let Some(cost) = execution_status.cost {
                                callback.on_field("Cost", &cost.to_string());
                            }
                            if let Some(total_cycle) = execution_status.total_cycle {
                                callback.on_field("Total Cycles", &total_cycle.to_string());
                            }
                        }
                        "segment" => {
                            if execution_status.num_segments.is_some()
                                || execution_status.total_cycle.is_some()
                            {
                                callback.on_section("Execution Statistics");
                                has_stats = true;
                            }
                            if let Some(num_segments) = execution_status.num_segments {
                                callback.on_field("Number of Segments", &num_segments.to_string());
                            }
                            if let Some(total_cycle) = execution_status.total_cycle {
                                callback.on_field("Total Cycles", &total_cycle.to_string());
                            }
                        }
                        "pure" => {
                            // Pure mode only shows public values, no statistics
                        }
                        _ => {
                            // For other modes, show cycles if available
                            if let Some(total_cycle) = execution_status.total_cycle {
                                callback.on_section("Execution Statistics");
                                callback.on_field("Total Cycles", &total_cycle.to_string());
                                has_stats = true;
                            }
                        }
                    }

                    // Legacy tick count (keeping for compatibility, but not for pure mode)
                    if execution_status.mode != "pure"
                        && let Some(total_tick) = execution_status.total_tick
                    {
                        if !has_stats {
                            callback.on_section("Execution Statistics");
                        }
                        callback.on_field("Total Ticks", &total_tick.to_string());
                    }

                    // Format public values more nicely (match CLI format)
                    if let Some(public_values) = &execution_status.public_values
                        && !public_values.is_null()
                    {
                        callback.on_section("Public Values");
                        if let Ok(compact) = serde_json::to_string(public_values) {
                            println!("  {}", compact);
                        }
                    }

                    if let Some(results_path) = self.save_execution_results(&execution_status) {
                        callback.on_section("Saving Results");
                        callback.on_success(&results_path);
                    }

                    return Ok(());
                }
                "Failed" => {
                    if spinner_started {
                        callback.on_progress_finish("");
                    }
                    let error_msg = execution_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Execution failed: {}", error_msg);
                }
                "Queued" => {
                    if !spinner_started {
                        callback.on_progress_start("Execution queued", None);
                        spinner_started = true;
                    }
                    std::thread::sleep(Duration::from_secs(EXECUTION_POLLING_INTERVAL_SECS));
                }
                "InProgress" => {
                    if !spinner_started {
                        callback.on_progress_start("Executing program", None);
                        spinner_started = true;
                    } else {
                        // Update message if we were previously in queued state
                        callback.on_progress_update_message("Executing program");
                    }
                    std::thread::sleep(Duration::from_secs(EXECUTION_POLLING_INTERVAL_SECS));
                }
                _ => {
                    let status_message = format!("Execution status: {}", execution_status.status);
                    if !spinner_started {
                        callback.on_progress_start(&status_message, None);
                        spinner_started = true;
                    } else {
                        callback.on_progress_update_message(&status_message);
                    }
                    std::thread::sleep(Duration::from_secs(EXECUTION_POLLING_INTERVAL_SECS));
                }
            }
        }
    }

    fn save_execution_results(&self, execution_status: &ExecutionStatus) -> Option<String> {
        // Save execution results under the program folder using program_uuid
        let run_dir = format!(
            "axiom-artifacts/program-{}/runs/{}",
            execution_status.program_uuid, execution_status.id
        );

        if std::fs::create_dir_all(&run_dir).is_err() {
            return None;
        }

        let results_path = format!("{}/results.json", run_dir);

        // Create a results object with summary and public values
        let results = serde_json::json!({
            "execution_id": execution_status.id,
            "created_at": execution_status.created_at,
            "launched_at": execution_status.launched_at,
            "terminated_at": execution_status.terminated_at,
            "mode": execution_status.mode,
            "total_cycles": execution_status.total_cycle,
            "total_ticks": execution_status.total_tick,
            "cost": execution_status.cost,
            "num_segments": execution_status.num_segments,
            "public_values": execution_status.public_values
        });

        if let Ok(results_json) = serde_json::to_string_pretty(&results)
            && std::fs::write(&results_path, results_json).is_ok()
        {
            return Some(results_path);
        }

        None
    }
}
