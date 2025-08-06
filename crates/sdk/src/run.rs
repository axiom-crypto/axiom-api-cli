use std::fs;

use cargo_openvm::input::Input;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{API_KEY_HEADER, AxiomSdk};

const EXECUTION_POLLING_INTERVAL_SECS: u64 = 10;

pub trait RunSdk {
    fn get_execution_status(&self, execution_id: &str) -> Result<ExecutionStatus>;
    fn execute_program(&self, args: RunArgs) -> Result<String>;
    fn wait_for_execution_completion(&self, execution_id: &str) -> Result<()>;
    fn save_execution_results(&self, execution_status: &ExecutionStatus) -> Option<String>;
}

#[derive(Debug, Default)]
pub struct RunArgs {
    /// The ID of the program to execute
    pub program_id: Option<String>,
    /// Input data for the execution (file path or hex string)
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
        // Get the program_id from args, return error if not provided
        let program_id = args
            .program_id
            .ok_or_eyre("Program ID is required. Use --program-id to specify.")?;

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
                        if !s.trim_start_matches("0x").starts_with("01")
                            && !s.trim_start_matches("0x").starts_with("02")
                        {
                            eyre::bail!("Hex string must start with '01' or '02'");
                        }
                        json!({ "input": [s] })
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

        let response = client
            .post(url_with_params)
            .header("Content-Type", "application/json")
            .header(API_KEY_HEADER, api_key)
            .body(body.to_string())
            .send()
            .context("Failed to send execution request")?;

        // Handle response
        if response.status().is_success() {
            let response_json: Value = response.json()?;
            let execution_id = response_json["id"].as_str().unwrap();
            Ok(execution_id.to_string())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!(
                "Cannot execute this program: {} (status: {})",
                error_text,
                status
            ))
        } else {
            let status = response.status();
            Err(eyre::eyre!(
                "Execute program request failed with status: {}",
                status
            ))
        }
    }

    fn wait_for_execution_completion(&self, execution_id: &str) -> Result<()> {
        use std::time::Duration;

        loop {
            let execution_status = self.get_execution_status(execution_id)?;

            match execution_status.status.as_str() {
                "Succeeded" => {
                    return Ok(());
                }
                "Failed" => {
                    let error_msg = execution_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Execution failed: {}", error_msg);
                }
                "Queued" | "InProgress" | _ => {
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

fn validate_input_json(json: &serde_json::Value) -> Result<()> {
    use cargo_openvm::input::is_valid_hex_string;

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
