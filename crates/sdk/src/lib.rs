use std::{path::PathBuf, sync::OnceLock};

use cargo_openvm::input::decode_hex_string;
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
pub const CLI_VERSION_HEADER: &str = "Axiom-CLI-Version";
static CLI_VERSION: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProofType {
    Evm,
    Stark,
}

impl std::fmt::Display for ProofType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProofType::Evm => write!(f, "evm"),
            ProofType::Stark => write!(f, "stark"),
        }
    }
}

impl std::str::FromStr for ProofType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "evm" => Ok(ProofType::Evm),
            "stark" => Ok(ProofType::Stark),
            _ => eyre::bail!("Invalid proof type: {s}. Must be 'evm' or 'stark'"),
        }
    }
}

pub const DEFAULT_CONFIG_ID: &str = "3c866d43-f693-4eba-9e0f-473f60858b73";
pub const STAGING_DEFAULT_CONFIG_ID: &str = "0d20f5cc-f3f1-4e20-b90b-2f1c5b5bf75d";

/// Trait for handling progress reporting and user feedback during SDK operations.
///
/// Implementations can provide custom behavior for different types of progress
/// events, status updates, and user interface elements. This allows the SDK to
/// remain UI-agnostic while still providing rich feedback to users.
///
/// # Examples
///
/// ```
/// use axiom_sdk::{ProgressCallback, NoopCallback};
///
/// // Use the no-op callback for silent operation
/// let callback = NoopCallback;
///
/// // Or implement your own callback for custom behavior
/// struct MyCallback;
/// impl ProgressCallback for MyCallback {
///     fn on_success(&self, text: &str) {
///         println!("✓ {}", text);
///     }
///     fn on_error(&self, text: &str) {
///         eprintln!("✗ {}", text);
///     }
///     // ... implement other methods as needed
/// }
/// ```
pub trait ProgressCallback {
    /// Called to display a header/title for a new operation section
    fn on_header(&self, text: &str);
    /// Called when an operation completes successfully
    fn on_success(&self, text: &str);
    /// Called to display informational messages
    fn on_info(&self, text: &str);
    /// Called to display warning messages
    fn on_warning(&self, text: &str);
    /// Called when an error occurs
    fn on_error(&self, text: &str);
    /// Called to display a section divider/header
    fn on_section(&self, title: &str);
    /// Called to display a field name-value pair
    fn on_field(&self, key: &str, value: &str);
    /// Called to display ongoing status updates (e.g., "Processing...")
    fn on_status(&self, text: &str);
    /// Called when starting a progress operation that may show a progress bar
    ///
    /// # Parameters
    /// * `message` - Description of the operation being performed
    /// * `total` - Total number of units if known (for progress bars), None for spinners
    fn on_progress_start(&self, message: &str, total: Option<u64>);
    /// Called to update progress with the current completion count
    fn on_progress_update(&self, current: u64);
    /// Called to update the progress message without restarting
    fn on_progress_update_message(&self, message: &str);
    /// Called when finishing a progress operation
    fn on_progress_finish(&self, message: &str);
    /// Called to clear the current line (for status updates)
    fn on_clear_line(&self);
    /// Called to clear the current line and reset cursor position
    fn on_clear_line_and_reset(&self);
}

/// A no-op implementation of [`ProgressCallback`] that ignores all events.
///
/// This is useful for headless or automated environments where you want to
/// suppress all progress reporting and user interface updates.
///
/// # Examples
///
/// ```
/// use axiom_sdk::{AxiomSdk, NoopCallback};
///
/// let config = axiom_sdk::load_config().unwrap();
/// let sdk = AxiomSdk::new(config);
/// let callback = NoopCallback;
///
/// // All progress events will be silently ignored
/// // let result = sdk.some_operation(&callback);
/// ```
pub struct NoopCallback;

impl ProgressCallback for NoopCallback {
    fn on_header(&self, _text: &str) {}
    fn on_success(&self, _text: &str) {}
    fn on_info(&self, _text: &str) {}
    fn on_warning(&self, _text: &str) {}
    fn on_error(&self, _text: &str) {}
    fn on_section(&self, _title: &str) {}
    fn on_field(&self, _key: &str, _value: &str) {}
    fn on_status(&self, _text: &str) {}
    fn on_progress_start(&self, _message: &str, _total: Option<u64>) {}
    fn on_progress_update(&self, _current: u64) {}
    fn on_progress_update_message(&self, _message: &str) {}
    fn on_progress_finish(&self, _message: &str) {}
    fn on_clear_line(&self) {}
    fn on_clear_line_and_reset(&self) {}
}

pub struct AxiomSdk {
    pub config: AxiomConfig,
    callback: Box<dyn ProgressCallback>,
}

impl AxiomSdk {
    pub fn new(config: AxiomConfig) -> Self {
        Self {
            config,
            callback: Box::new(NoopCallback),
        }
    }

    pub fn with_callback<T: ProgressCallback + 'static>(mut self, callback: T) -> Self {
        self.callback = Box::new(callback);
        self
    }
}

impl Default for AxiomSdk {
    fn default() -> Self {
        Self {
            config: AxiomConfig::default(),
            callback: Box::new(NoopCallback),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxiomConfig {
    pub api_url: String,
    pub api_key: Option<String>,
    pub config_id: Option<String>,
    pub console_base_url: Option<String>,
}

fn default_console_base_url() -> String {
    "https://prove.axiom.xyz".to_string()
}

impl AxiomConfig {
    pub fn new(api_url: String, api_key: Option<String>, config_id: Option<String>) -> Self {
        Self {
            api_url,
            api_key,
            config_id,
            console_base_url: Some(default_console_base_url()),
        }
    }
}

impl Default for AxiomConfig {
    fn default() -> Self {
        Self {
            api_url: "https://api.axiom.xyz/v1".to_string(),
            api_key: None,
            config_id: Some(DEFAULT_CONFIG_ID.to_string()),
            console_base_url: Some(default_console_base_url()),
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
        eyre::bail!("CLI not initialized. Run 'cargo axiom register' first.");
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

/// Validates input JSON format for OpenVM programs.
///
/// Ensures that the input follows the expected format with hex strings
/// that start with the proper prefixes for bytes (01) or field elements (02).
pub fn validate_input_json(json: &serde_json::Value) -> Result<(), eyre::Error> {
    json["input"]
        .as_array()
        .ok_or_eyre("Input must be an array under 'input' key")?
        .iter()
        .try_for_each(|inner| {
            inner
                .as_str()
                .ok_or_eyre("Each value must be a hex string")
                .and_then(|s| match decode_hex_string(s) {
                    Err(msg) => Err(eyre::eyre!("Invalid hex string: {msg}")),
                    Ok(_) => Ok(()),
                })
        })?;
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

pub fn validate_api_key(api_url: &str, api_key: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/validate_api_key", api_url);

    let response = add_cli_version_header(client.get(url))
        .header(API_KEY_HEADER, api_key)
        .send()?;

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

pub fn add_cli_version_header(builder: RequestBuilder) -> RequestBuilder {
    if let Some(version) = CLI_VERSION.get() {
        return builder.header(CLI_VERSION_HEADER, version);
    }
    builder
}

pub fn set_cli_version(version: &str) {
    let _ = CLI_VERSION.set(version.to_string());
}

pub fn authenticated_get(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(add_cli_version_header(client.get(url)).header(API_KEY_HEADER, api_key))
}

pub fn authenticated_post(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(add_cli_version_header(client.post(url)).header(API_KEY_HEADER, api_key))
}

pub fn authenticated_put(config: &AxiomConfig, url: &str) -> Result<RequestBuilder> {
    let client = Client::new();
    let api_key = config.api_key.as_ref().ok_or_eyre("API key not set")?;

    Ok(add_cli_version_header(client.put(url)).header(API_KEY_HEADER, api_key))
}

/// Calculate a human-readable duration between two RFC3339 timestamps.
///
/// Returns a formatted string like "5s", "2m 30s", or "1h 15m 30s".
///
/// # Arguments
/// * `start` - RFC3339 timestamp string (e.g., "2023-01-01T10:00:00Z")
/// * `end` - RFC3339 timestamp string (e.g., "2023-01-01T10:05:30Z")
///
/// # Returns
/// * `Ok(String)` - Human-readable duration (e.g., "5m 30s")
/// * `Err(String)` - Error message if timestamps are invalid
///
/// # Examples
/// ```
/// use axiom_sdk::calculate_duration;
///
/// let start = "2023-01-01T10:00:00Z";
/// let end = "2023-01-01T10:05:30Z";
/// let duration = calculate_duration(start, end).unwrap();
/// assert_eq!(duration, "5m 30s");
/// ```
pub fn calculate_duration(start: &str, end: &str) -> Result<String, String> {
    use chrono::DateTime;

    let start_time = DateTime::parse_from_rfc3339(start).map_err(|_| "Invalid start timestamp")?;
    let end_time = DateTime::parse_from_rfc3339(end).map_err(|_| "Invalid end timestamp")?;

    let duration = end_time.signed_duration_since(start_time);
    let total_seconds = duration.num_seconds();

    if total_seconds < 60 {
        Ok(format!("{}s", total_seconds))
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        Ok(format!("{}m {}s", minutes, seconds))
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        Ok(format!("{}h {}m {}s", hours, minutes, seconds))
    }
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
        assert_eq!(config.console_base_url, Some(default_console_base_url()));
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
        assert_eq!(config.console_base_url, Some(default_console_base_url()));
    }

    #[test]
    fn test_axiom_config_serialization() {
        let config = AxiomConfig {
            api_url: "https://test.api.com/v1".to_string(),
            api_key: Some("test-key".to_string()),
            config_id: Some("test-config-id".to_string()),
            console_base_url: Some(default_console_base_url()),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AxiomConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.api_url, deserialized.api_url);
        assert_eq!(config.api_key, deserialized.api_key);
        assert_eq!(config.config_id, deserialized.config_id);
        assert_eq!(config.console_base_url, deserialized.console_base_url);
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
    }

    #[test]
    fn test_duration_calculation() {
        let start = "2023-01-01T12:00:00Z";
        let end = "2023-01-01T12:01:30Z";

        let result = calculate_duration(start, end).unwrap();
        assert_eq!(result, "1m 30s");
    }

    #[test]
    fn test_duration_calculation_seconds_only() {
        let start = "2023-01-01T12:00:00Z";
        let end = "2023-01-01T12:00:45Z";

        let result = calculate_duration(start, end).unwrap();
        assert_eq!(result, "45s");
    }

    #[test]
    fn test_duration_calculation_hours() {
        let start = "2023-01-01T12:00:00Z";
        let end = "2023-01-01T14:15:30Z";

        let result = calculate_duration(start, end).unwrap();
        assert_eq!(result, "2h 15m 30s");
    }
}
