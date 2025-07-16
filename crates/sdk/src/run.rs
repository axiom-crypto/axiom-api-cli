use std::fs;

use cargo_openvm::input::Input;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{AxiomSdk, API_KEY_HEADER};

pub trait RunSdk {
    fn get_execution_status(&self, execution_id: &str) -> Result<ExecutionStatus>;
    fn execute_program(&self, args: RunArgs) -> Result<String>;
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

        println!(
            "Checking execution status for execution ID: {}",
            execution_id
        );

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
            let execution_status = serde_json::from_value(body)?;
            Ok(execution_status)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            println!(
                "Cannot check execution status for this execution: {}",
                error_text
            );
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
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

        println!("Executing program ID: {}", program_id);

        let url = format!("{}/executions", self.config.api_url);
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

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
            println!("Execution initiated successfully!: {}", execution_id);
            Ok(execution_id.to_string())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            println!("Cannot execute this program: {}", error_text);
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            let status = response.status();
            Err(eyre::eyre!(
                "Execute program request failed with status: {}",
                status
            ))
        }
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
