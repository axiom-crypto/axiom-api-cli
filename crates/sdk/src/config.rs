use std::{
    fs::File,
    io::{copy, Write},
    path::PathBuf,
};

use eyre::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{get_config_id, AxiomConfig, AxiomSdk, API_KEY_HEADER};

pub trait ConfigSdk {
    fn get_vm_config_metadata(&self, config_id: Option<&str>) -> Result<VmConfigMetadata>;
    fn get_proving_keys(&self, config_id: Option<&str>, key_type: &str) -> Result<PkDownloader>;
    fn get_evm_verifier(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()>;
    fn get_vm_commitment(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()>;
    fn download_config(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmConfigMetadata {
    pub id: String,
    pub created_at: String,
    pub openvm_version: String,
    pub stark_backend_version: String,
    pub status: String,
    pub active: bool,
    #[serde(rename = "app_vm_config")]
    pub app_vm_commit: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PkDownloader {
    pub download_url: String,
}

impl PkDownloader {
    pub fn download_pk(&self, output_path: &str) -> Result<()> {
        std::fs::create_dir_all(output_path)?;

        let client = Client::new();

        let response = client
            .get(&self.download_url)
            .send()
            .context("Failed to download proving keys")?;

        if response.status().is_success() {
            println!("Proving keys downloaded successfully");
            let mut file = File::create(output_path)?;
            file.write_all(&response.bytes()?)?;
            Ok(())
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Config status request failed with status: {}",
                response.status()
            ))
        }
    }
}

impl ConfigSdk for AxiomSdk {
    fn get_vm_config_metadata(&self, config_id: Option<&str>) -> Result<VmConfigMetadata> {
        let config_id = get_config_id(config_id, &self.config)?;
        let url = format!("{}/configs/{}", self.config.api_url, config_id);



        // Make the GET request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send status request")?;

        if response.status().is_success() {
            let body: Value = response.json()?;
            let metadata = serde_json::from_value(body)?;
            Ok(metadata)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Config status request failed with status: {}",
                response.status()
            ))
        }
    }

    fn get_proving_keys(&self, config_id: Option<&str>, key_type: &str) -> Result<PkDownloader> {
        // Load configuration
        let config_id = get_config_id(config_id, &self.config)?;
        let url = format!(
            "{}/configs/{}/pk/{}",
            self.config.api_url, config_id, key_type
        );

        println!("Getting {key_type} proving key for config ID: {config_id}");

        // Make the GET request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send download request")?;

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the response to get the download URL
            let response_json: Value = response.json()?;
            let downloader = serde_json::from_value(response_json)?;
            Ok(downloader)
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            Err(eyre::eyre!(
                "Download request failed with status: {}",
                response.status()
            ))
        }
    }

    fn get_evm_verifier(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()> {
        download_artifact(&self.config, config_id, "evm_verifier", output)
    }

    fn get_vm_commitment(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()> {
        download_artifact(&self.config, config_id, "app_vm_commit", output)
    }

    fn download_config(&self, config_id: Option<&str>, output: Option<PathBuf>) -> Result<()> {
        download_artifact(&self.config, config_id, "config", output)
    }
}

fn download_artifact(
    config: &AxiomConfig,
    config_id: Option<&str>,
    artifact_type: &str,
    output: Option<PathBuf>,
) -> Result<()> {
    // Load configuration
    let config_id = get_config_id(config_id, config)?;
    let url = format!("{}/configs/{}/{}", config.api_url, config_id, artifact_type);

    println!("Downloading {artifact_type} for config ID: {config_id}");

    // Determine output path
    let output_path = match output {
        Some(path) => path,
        None => {
            // Create organized directory structure
            let config_dir = format!("axiom-artifacts/configs/{}", config_id);
            std::fs::create_dir_all(&config_dir)
                .context(format!("Failed to create config directory: {}", config_dir))?;
            
            if artifact_type == "evm_verifier" {
                PathBuf::from(format!("{}/evm_verifier.json", config_dir))
            } else if artifact_type == "config" {
                PathBuf::from(format!("{}/config.toml", config_dir))
            } else {
                PathBuf::from(format!("{}/{}", config_dir, artifact_type))
            }
        }
    };

    // Make the GET request
    let client = Client::new();
    let api_key = config
        .api_key
        .as_ref()
        .ok_or(eyre::eyre!("API key not set"))?;

    let response = client
        .get(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send download request")?;

    // Check if the request was successful
    if response.status().is_success() {
        // Create the output file
        let mut file = File::create(&output_path)
            .context(format!("Failed to create output file: {output_path:?}"))?;

        // Stream the response body to the file
        copy(&mut response.bytes()?.as_ref(), &mut file)
            .context("Failed to write response to file")?;

        println!("Successfully downloaded to {output_path:?}");
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Download request failed with status: {}",
            response.status()
        ))
    }
}
