use std::fs;

use cargo_openvm::input::Input;
use eyre::{Context, OptionExt, Result};
use hex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    API_KEY_HEADER, AxiomSdk, ProgressCallback, add_cli_version_header, calculate_duration,
    validate_input_json,
};

const EXECUTION_POLLING_INTERVAL_SECS: u64 = 10;

pub trait RunSdk {
    fn get_execution_status(&self, execution_id: &str) -> Result<ExecutionStatus>;
    fn execute_program(&self, args: RunArgs) -> Result<String>;
    fn wait_for_execution_completion(&self, execution_id: &str) -> Result<()>;
    fn save_execution_results(&self, execution_status: &ExecutionStatus) -> Option<String>;
}

#[derive(Debug, Default)]
pub struct RunArgs {
    pub program_id: Option<String>,
    pub input: Option<Input>,
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
    pub total_cycle: Option<u64>,
    pub total_tick: Option<u64>,
    pub public_values: Option<Value>,
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
            "total_cycles": execution_status.total_cycle,
            "total_ticks": execution_status.total_tick,
            "public_values": execution_status.public_values
        });

        if let Ok(results_json) = serde_json::to_string_pretty(&results) {
            if std::fs::write(&results_path, results_json).is_ok() {
                return Some(results_path);
            }
        }

        None
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
            Some(input) => {
                match input {
                    Input::FilePath(path) => {
                        // Read the file content directly as JSON
                        let file_content = fs::read_to_string(path)
                            .context(format!("Failed to read input file: {}", path.display()))?;
                        let input_json = serde_json::from_str(&file_content).context(format!(
                            "Failed to parse input file as JSON: {}",
                            path.display()
                        ))?;
                        validate_input_json(&input_json)?;
                        input_json
                    }
                    Input::HexBytes(s) => {
                        if !matches!(s.first(), Some(x) if x == &0x01 || x == &0x02) {
                            eyre::bail!(
                                "Hex string must start with '01'(bytes) or '02'(field elements). See the OpenVM book for more details. https://docs.openvm.dev/book/writing-apps/overview/#inputs"
                            );
                        }
                        let hex_string = format!("0x{}", hex::encode(s));
                        json!({ "input": [hex_string] })
                    }
                }
            }
            None => json!({ "input": [] }), // Empty JSON if no input provided
        };

        // Make API request
        let client = Client::new();
        let mut url_with_params = url::Url::parse(&url)?;
        url_with_params
            .query_pairs_mut()
            .append_pair("program_id", &program_id);

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

        loop {
            let execution_status = self.get_execution_status(execution_id)?;

            match execution_status.status.as_str() {
                "Succeeded" => {
                    callback.on_clear_line_and_reset();
                    callback.on_success("Execution completed successfully!");

                    callback.on_section("Execution Summary");
                    callback.on_field("Execution ID", &execution_status.id);
                    if let Some(total_cycle) = execution_status.total_cycle {
                        callback.on_field("Total Cycles", &total_cycle.to_string());
                    }
                    if let Some(total_tick) = execution_status.total_tick {
                        callback.on_field("Total Ticks", &total_tick.to_string());
                    }

                    if let Some(public_values) = &execution_status.public_values {
                        if !public_values.is_null() {
                            callback.on_section("Public Values");
                            if let Ok(compact) = serde_json::to_string(public_values) {
                                callback.on_info(&compact);
                            }
                        }
                    }

                    if let Some(launched_at) = &execution_status.launched_at {
                        if let Some(terminated_at) = &execution_status.terminated_at {
                            callback.on_section("Execution Stats");
                            callback.on_field("Created", &execution_status.created_at);
                            callback.on_field("Initiated", launched_at);
                            callback.on_field("Finished", terminated_at);

                            if let Ok(duration) = calculate_duration(launched_at, terminated_at) {
                                callback.on_field("Duration", &duration);
                            }
                        }
                    }

                    if let Some(results_path) = self.save_execution_results(&execution_status) {
                        callback.on_section("Saving Results");
                        callback.on_success(&format!("✓ {}", results_path));
                    }

                    return Ok(());
                }
                "Failed" => {
                    callback.on_clear_line_and_reset();
                    let error_msg = execution_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Execution failed: {}", error_msg);
                }
                "Queued" => {
                    callback.on_status("Execution queued...");
                    std::thread::sleep(Duration::from_secs(EXECUTION_POLLING_INTERVAL_SECS));
                }
                "InProgress" => {
                    callback.on_status("Execution in progress...");
                    std::thread::sleep(Duration::from_secs(EXECUTION_POLLING_INTERVAL_SECS));
                }
                _ => {
                    callback
                        .on_status(&format!("Execution status: {}...", execution_status.status));
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
            "total_cycles": execution_status.total_cycle,
            "total_ticks": execution_status.total_tick,
            "public_values": execution_status.public_values
        });

        if let Ok(results_json) = serde_json::to_string_pretty(&results) {
            if std::fs::write(&results_path, results_json).is_ok() {
                return Some(results_path);
            }
        }

        None
    }
}
