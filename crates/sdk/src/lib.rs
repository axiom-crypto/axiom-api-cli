use std::path::PathBuf;

use dirs::home_dir;
use eyre::{Context, OptionExt, Result};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub mod build;
pub mod config;
pub mod projects;
pub mod prove;
pub mod run;
pub mod verify;

pub const API_KEY_HEADER: &str = "Axiom-API-Key";

pub const DEFAULT_CONFIG_ID: &str = "8700ea25-f3b2-4ac2-a745-3e26d754d7a5";
pub const STAGING_DEFAULT_CONFIG_ID: &str = "649822e0-dc4c-4eaf-bbe4-580feb20f9f1";

#[derive(Default)]
pub struct AxiomSdk {
    pub config: AxiomConfig,
}

impl AxiomSdk {
    pub fn new(config: AxiomConfig) -> Self {
        Self { config }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxiomConfig {
    pub api_url: String,
    pub api_key: Option<String>,
    pub config_id: Option<String>,
    pub last_project_id: Option<u32>,
}

impl AxiomConfig {
    pub fn new(api_url: String, api_key: Option<String>, config_id: Option<String>) -> Self {
        Self {
            api_url,
            api_key,
            config_id,
            last_project_id: None,
        }
    }
}

impl Default for AxiomConfig {
    fn default() -> Self {
        Self {
            api_url: "https://api.axiom.xyz/v1".to_string(),
            api_key: None,
            config_id: Some(DEFAULT_CONFIG_ID.to_string()),
            last_project_id: None,
        }
    }
}

pub fn get_axiom_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or_eyre("Could not find home directory")?;
    Ok(home.join(".axiom"))
}

pub fn get_config_path() -> PathBuf {
    get_axiom_dir().unwrap().join("config.json")
}

pub fn load_config_without_validation() -> Result<AxiomConfig> {
    let config_path = get_config_path();

    if !config_path.exists() {
        // Try to load from old config format
        return Ok(AxiomConfig::default());
    }

    let config_str = std::fs::read_to_string(config_path).context("Failed to read config file")?;

    serde_json::from_str(&config_str).context("Failed to parse config file")
}

pub fn load_config() -> Result<AxiomConfig> {
    let config = load_config_without_validation()?;
    if config.api_key.is_none() {
        eyre::bail!("CLI not initialized. Run 'cargo axiom init' first.");
    }
    Ok(config)
}

pub fn save_config(config: &AxiomConfig) -> Result<()> {
    let config_path = get_config_path();

    // Ensure the directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }

    let config_str = serde_json::to_string_pretty(config).context("Failed to serialize config")?;

    std::fs::write(config_path, config_str).context("Failed to write config file")?;

    Ok(())
}

pub fn get_api_key() -> Result<String> {
    let config = load_config()?;
    config
        .api_key
        .ok_or_eyre("API key not found. Run 'cargo axiom init' first.")
}

pub fn set_config_id(id: &str) -> Result<()> {
    let mut config = load_config()?;
    config.config_id = Some(id.to_string());
    save_config(&config)
}

pub fn get_config_id(args_config_id: Option<&str>, config: &AxiomConfig) -> Result<String> {
    if let Some(id) = args_config_id {
        set_config_id(id)?;
        Ok(id.to_string())
    } else if let Some(id) = &config.config_id {
        Ok(id.clone())
    } else {
        Err(eyre::eyre!("No config ID provided"))
    }
}

pub fn set_project_id(id: u32) -> Result<()> {
    let mut config = load_config()?;
    config.last_project_id = Some(id);
    save_config(&config)
}

pub fn get_project_id(args_project_id: Option<u32>, config: &AxiomConfig) -> Option<u32> {
    if let Some(id) = args_project_id {
        if set_project_id(id).is_ok() {
            Some(id)
        } else {
            args_project_id
        }
    } else {
        config.last_project_id
    }
}

pub fn validate_api_key(api_url: &str, api_key: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/validate_api_key", api_url);

    let response = client.get(url).header(API_KEY_HEADER, api_key).send()?;

    if response.status().is_success() {
        // API key is valid - backend returns {"message": "OK"}
        Ok(())
    } else if response.status().is_client_error() {
        // API key is invalid - backend returns 401/403
        Err(eyre::eyre!("API key is not valid or inactive"))
    } else {
        // Server error or other issues
        Err(eyre::eyre!(
            "Failed to validate API key: HTTP {}",
            response.status()
        ))
    }
}

pub fn authenticated_get(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(client.get(url).header(API_KEY_HEADER, api_key))
}

pub fn authenticated_post(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(client.post(url).header(API_KEY_HEADER, api_key))
}

pub fn authenticated_put(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(client.put(url).header(API_KEY_HEADER, api_key))
}

pub fn send_request_json<T: DeserializeOwned>(
    request_builder: RequestBuilder,
    error_context: &str,
) -> Result<T> {
    let response = request_builder
        .send()
        .with_context(|| error_context.to_string())?;

    handle_json_response(response)
}

pub fn send_request(request_builder: RequestBuilder, error_context: &str) -> Result<()> {
    let response = request_builder
        .send()
        .with_context(|| error_context.to_string())?;

    handle_response(response)
}

fn handle_json_response<T: DeserializeOwned>(response: Response) -> Result<T> {
    if response.status().is_success() {
        let result: T = response.json()?;
        Ok(result)
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Request failed with status: {}",
            response.status()
        ))
    }
}

fn handle_response(response: Response) -> Result<()> {
    if response.status().is_success() {
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Request failed with status: {}",
            response.status()
        ))
    }
}

pub fn download_file(
    request_builder: RequestBuilder,
    output_path: &std::path::Path,
    error_context: &str,
) -> Result<()> {
    let response = request_builder
        .send()
        .with_context(|| error_context.to_string())?;

    if response.status().is_success() {
        let mut file = std::fs::File::create(output_path).context(format!(
            "Failed to create output file: {}",
            output_path.display()
        ))?;

        let content = response.bytes().context("Failed to read response body")?;

        std::io::copy(&mut content.as_ref(), &mut file)
            .context("Failed to write response to file")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = AxiomConfig::default();
        assert_eq!(config.api_url, "https://api.axiom.xyz/v1");
        assert!(config.api_key.is_none());
        assert_eq!(config.config_id, Some(DEFAULT_CONFIG_ID.to_string()));
        assert!(config.last_project_id.is_none());
    }

    #[test]
    fn test_config_new() {
        let config = AxiomConfig::new(
            "https://test.api.com".to_string(),
            Some("test-key".to_string()),
            Some("test-config".to_string()),
        );
        assert_eq!(config.api_url, "https://test.api.com");
        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert_eq!(config.config_id, Some("test-config".to_string()));
        assert!(config.last_project_id.is_none());
    }

    #[test]
    fn test_get_project_id_with_args() {
        let config = AxiomConfig::default();

        // Mock save_config to avoid file system operations
        let project_id = 123;
        let result = get_project_id(Some(project_id), &config);

        // Should return the provided project_id
        assert_eq!(result, Some(project_id));
    }

    #[test]
    fn test_get_project_id_from_config() {
        let config = AxiomConfig {
            last_project_id: Some(456),
            ..Default::default()
        };

        let result = get_project_id(None, &config);

        // Should return the config's project_id
        assert_eq!(result, Some(456));
    }

    #[test]
    fn test_get_project_id_none() {
        let config = AxiomConfig::default();

        let result = get_project_id(None, &config);

        // Should return None when no project_id available
        assert_eq!(result, None);
    }

    #[test]
    fn test_axiom_config_serialization() {
        let config = AxiomConfig {
            api_url: "https://test.api.com/v1".to_string(),
            api_key: Some("test-key".to_string()),
            config_id: Some("test-config-id".to_string()),
            last_project_id: Some(123),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AxiomConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.api_url, deserialized.api_url);
        assert_eq!(config.api_key, deserialized.api_key);
        assert_eq!(config.config_id, deserialized.config_id);
        assert_eq!(config.last_project_id, deserialized.last_project_id);
    }

    #[test]
    fn test_axiom_config_serialization_backwards_compatibility() {
        // Test that old configs without last_project_id still deserialize correctly
        let json = r#"{
            "api_url": "https://api.axiom.xyz/v1",
            "api_key": "test-key",
            "config_id": "test-config"
        }"#;

        let config: AxiomConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.api_url, "https://api.axiom.xyz/v1");
        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert_eq!(config.config_id, Some("test-config".to_string()));
        assert!(config.last_project_id.is_none());
    }
}
