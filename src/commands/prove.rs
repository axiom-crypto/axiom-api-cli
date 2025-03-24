use std::fs;

use cargo_openvm::input::{is_valid_hex_string, Input};
use clap::Args;
use eyre::{eyre, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::{config, config::API_KEY_HEADER};

#[derive(Args, Debug)]
pub struct ProveCmd {
    /// The ID of the program to generate a proof for
    #[arg(long)]
    program_id: String,

    /// Input data for the proof (file path or hex string)
    #[clap(long, value_parser, help = "Input to OpenVM program")]
    input: Option<Input>,
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

impl ProveCmd {
    pub fn run(&self) -> Result<()> {
        println!("Generating proof for program ID: {}", self.program_id);

        // Load config
        let config = config::load_config()?;
        let url = format!("{}/proofs?program_uuid={}", config.api_url, self.program_id);
        let api_key = config
            .api_key
            .ok_or_else(|| eyre!("API key not found. Please run `cargo axiom init` first."))?;

        // Create the request body based on input
        let body = match &self.input {
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
                    Input::HexBytes(hex_str) => {
                        json!({ "input": [hex_str] })
                    }
                }
            }
            None => json!({ "input": [] }), // Empty JSON if no input provided
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
            println!(
                "Proof generation initiated successfully!: {}",
                response_json
            );
        } else {
            let error_text = response.text()?;
            return Err(eyre!("Failed to generate proof: {}", error_text));
        }

        Ok(())
    }
}
