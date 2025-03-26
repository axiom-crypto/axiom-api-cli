use std::{fs, io::copy, path::PathBuf};

use cargo_openvm::input::{is_valid_hex_string, Input};
use clap::{Args, Subcommand};
use eyre::{eyre, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::{config, config::API_KEY_HEADER};

#[derive(Args, Debug)]
pub struct ProveCmd {
    #[command(subcommand)]
    command: Option<ProveSubcommand>,

    #[clap(flatten)]
    prove_args: ProveArgs,
}

#[derive(Debug, Subcommand)]
enum ProveSubcommand {
    /// Check the status of a proof
    Status {
        /// The proof ID to check status for
        #[clap(long, value_name = "ID")]
        proof_id: String,
    },
    /// Download proof artifacts
    Download {
        /// The proof ID to download artifacts for
        #[clap(long, value_name = "ID")]
        proof_id: String,

        /// The type of proof artifact to download (stark, root, or evm)
        #[clap(long, value_parser = ["stark", "root", "evm"])]
        r#type: String,

        /// Output file path (defaults to proof_id-type.json)
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

#[derive(Args, Debug)]
pub struct ProveArgs {
    /// The ID of the program to generate a proof for
    #[arg(long)]
    program_id: Option<String>,

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
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(ProveSubcommand::Status { proof_id }) => check_proof_status(proof_id),
            Some(ProveSubcommand::Download {
                proof_id,
                r#type,
                output,
            }) => download_proof_artifact(proof_id, r#type, output),
            None => execute(self.prove_args),
        }
    }
}

fn execute(args: ProveArgs) -> Result<()> {
    // Get the program_id from args, return error if not provided
    let program_id = args
        .program_id
        .ok_or_else(|| eyre::eyre!("Program ID is required. Use --program-id to specify."))?;

    println!("Generating proof for program ID: {}", program_id);

    // Load config
    let config = config::load_config()?;
    let url = format!("{}/proofs?program_id={}", config.api_url, program_id);
    let api_key = config::get_api_key()?;

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
                        return Err(eyre::eyre!("Hex string must start with '01' or '02'"));
                    }
                    json!({ "input": [s] })
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

fn check_proof_status(proof_id: String) -> Result<()> {
    // Load configuration
    let config = config::load_config()?;
    let url = format!("{}/proofs/{}", config.api_url, proof_id);

    println!("Checking proof status for proof ID: {}", proof_id);

    // Make the GET request
    let client = Client::new();
    let api_key = config::get_api_key()?;

    let response = client
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send status request")?;

    // Check if the request was successful
    if response.status().is_success() {
        println!("Proof status: {}", response.text().unwrap());
        Ok(())
    } else {
        Err(eyre::eyre!(
            "Status request failed with status: {}",
            response.status()
        ))
    }
}

fn download_proof_artifact(
    proof_id: String,
    artifact_type: String,
    output: Option<PathBuf>,
) -> Result<()> {
    // Load configuration
    let config = config::load_config()?;
    let url = format!("{}/proofs/{}/{}", config.api_url, proof_id, artifact_type);

    println!(
        "Downloading {} proof for proof ID: {}",
        artifact_type, proof_id
    );

    // Make the GET request
    let client = Client::new();
    let api_key = config::get_api_key()?;

    let response = client
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send download request")?;

    // Check if the request was successful
    if response.status().is_success() {
        // Determine output file path
        let output_path = match output {
            Some(path) => path,
            None => PathBuf::from(format!("{}-{}.proof", proof_id, artifact_type)),
        };

        // Create file and stream the response body to it
        let mut file = fs::File::create(&output_path)
            .context(format!("Failed to create output file: {:?}", output_path))?;

        copy(
            &mut response
                .bytes()
                .context("Failed to read response body")?
                .as_ref(),
            &mut file,
        )
        .context("Failed to write response to file")?;

        println!("Successfully downloaded to: {:?}", output_path);
        Ok(())
    } else {
        Err(eyre::eyre!(
            "Download request failed with status: {}",
            response.status()
        ))
    }
}
