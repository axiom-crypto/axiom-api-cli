use std::{
    fs::File,
    io::{Read, Write, copy},
    path::PathBuf,
};

use bytes::Bytes;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    API_KEY_HEADER, AxiomConfig, AxiomSdk, SaveOption, add_cli_version_header, get_config_id,
};

pub trait ConfigSdk {
    fn get_vm_config_metadata(&self, config_id: Option<&str>) -> Result<VmConfigMetadata>;
    fn get_proving_keys(&self, config_id: Option<&str>, key_type: &str) -> Result<PkDownloader>;
    fn get_evm_verifier(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes>;
    fn get_vm_commitment(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes>;
    fn download_config(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VmConfigMetadata {
    pub id: String,
    pub created_at: String,
    pub openvm_version: String,
    pub stark_backend_version: String,
    pub status: String,
    pub active: bool,
    pub app_vm_commit: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PkDownloader {
    pub download_url: String,
}

impl PkDownloader {
    pub fn download_pk(&self, output_path: &str) -> Result<()> {
        self.download_pk_with_callback(output_path, &crate::NoopCallback)
    }

    pub fn download_pk_with_callback(
        &self,
        output_path: &str,
        callback: &dyn crate::ProgressCallback,
    ) -> Result<()> {
        let path = std::path::Path::new(output_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let client = Client::new();

        let mut response = client
            .get(&self.download_url)
            .send()
            .context("Failed to download proving keys")?;

        if response.status().is_success() {
            let content_length = response.content_length();

            if let Some(total) = content_length {
                callback.on_progress_start("Downloading proving key", Some(total));
            } else {
                callback.on_progress_start("Downloading proving key", None);
            }

            let mut file = File::create(output_path)?;
            if content_length.is_some() {
                let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer
                let mut downloaded = 0u64;

                loop {
                    let bytes_read = response.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    file.write_all(&buffer[..bytes_read])?;
                    downloaded += bytes_read as u64;
                    callback.on_progress_update(downloaded);
                }
            } else {
                copy(&mut response, &mut file)?;
            }
            callback.on_progress_finish("✓ Key downloaded successfully");
            Ok(())
        } else if response.status().is_client_error() {
            callback.on_progress_finish("");
            let status = response.status();
            let error_text = response.text()?;
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            callback.on_progress_finish("");
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
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(&url).header(API_KEY_HEADER, api_key))
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

        self.callback.on_info(&format!(
            "Getting {key_type} proving key for config ID: {config_id}"
        ));
        let (key_type_part, p_or_v) = key_type.split_once('_').unwrap();
        let url = if p_or_v == "pk" {
            format!(
                "{}/configs/{}/pk/{}",
                self.config.api_url, config_id, key_type_part
            )
        } else if p_or_v == "vk" {
            format!(
                "{}/configs/{}/vk/{}",
                self.config.api_url, config_id, key_type_part,
            )
        } else {
            return Err(eyre::eyre!("Invalid key type: {}", key_type));
        };

        // Make the GET request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(&url).header(API_KEY_HEADER, api_key))
            .send()
            .context("Failed to send download request")?;

        // Check if the request was successful
        if response.status().is_success() {
            // Parse the response to get the download URL
            let response_json: Value = response
                .json()
                .context("Failed to parse proving key response as JSON")?;
            let downloader: PkDownloader =
                serde_json::from_value(response_json.clone()).context(format!(
                    "Failed to deserialize proving key response. Got: {}",
                    response_json
                ))?;
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

    fn get_evm_verifier(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes> {
        let config_id_str = get_config_id(config_id, &self.config)?;
        self.callback.on_info(&format!(
            "Downloading evm_verifier for config ID: {config_id_str}"
        ));
        let result = download_artifact(&self.config, config_id, "evm_verifier", output.clone());
        if output.saves() && result.is_ok() {
            let output_path = output.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "axiom-artifacts/configs/{}/evm_verifier.json",
                    config_id_str
                ))
            });
            self.callback
                .on_success(&format!("Successfully downloaded to {output_path:?}"));
        }
        result
    }

    fn get_vm_commitment(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes> {
        let config_id_str = get_config_id(config_id, &self.config)?;
        self.callback.on_info(&format!(
            "Downloading app_vm_commit for config ID: {config_id_str}"
        ));
        let result = download_artifact(&self.config, config_id, "app_vm_commit", output.clone());
        if output.saves() && result.is_ok() {
            let output_path = output.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "axiom-artifacts/configs/{}/app_vm_commit",
                    config_id_str
                ))
            });
            self.callback
                .on_success(&format!("Successfully downloaded to {output_path:?}"));
        }
        result
    }

    fn download_config(&self, config_id: Option<&str>, output: SaveOption) -> Result<Bytes> {
        let config_id_str = get_config_id(config_id, &self.config)?;
        self.callback.on_info(&format!(
            "Downloading config for config ID: {config_id_str}"
        ));
        let result = download_artifact(&self.config, config_id, "config", output.clone());
        if output.saves() && result.is_ok() {
            let output_path = output.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "axiom-artifacts/configs/{}/config.toml",
                    config_id_str
                ))
            });
            self.callback
                .on_success(&format!("Successfully downloaded to {output_path:?}"));
        }
        result
    }
}

fn download_artifact(
    config: &AxiomConfig,
    config_id: Option<&str>,
    artifact_type: &str,
    output: SaveOption,
) -> Result<Bytes> {
    // Load configuration
    let config_id = get_config_id(config_id, config)?;
    let url = format!("{}/configs/{}/{}", config.api_url, config_id, artifact_type);

    // Make the GET request
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    let response = add_cli_version_header(client.get(&url).header(API_KEY_HEADER, api_key))
        .send()
        .context("Failed to send download request")?;

    // Check if the request was successful
    if response.status().is_success() {
        let bytes = response.bytes()?;

        if output.saves() {
            // Determine output path
            let output_path = match output {
                SaveOption::Path(path) => path,
                SaveOption::DefaultPath => {
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
                SaveOption::DoNotSave => unreachable!(),
            };
            let mut file = File::create(&output_path)
                .context(format!("Failed to create output file: {output_path:?}"))?;
            copy(&mut bytes.as_ref(), &mut file).context("Failed to write response to file")?;
        }

        Ok(bytes)
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        eyre::bail!("Client error ({}): {}", status, error_text)
    } else {
        eyre::bail!("Download request failed with status: {}", response.status())
    }
}
