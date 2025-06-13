use std::path::PathBuf;

use dirs::home_dir;
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};

pub const API_KEY_HEADER: &str = "Axiom-API-Key";

pub const DEFAULT_CONFIG_ID: &str = "91b7737e-2f72-479d-b8db-43ca2c6d3328";
pub const STAGING_DEFAULT_CONFIG_ID: &str = "3796a702-0800-428d-9c14-ddc74df753b5";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_url: String,
    pub api_key: Option<String>,
    pub config_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "https://api.axiom.xyz/v1".to_string(),
            api_key: None,
            config_id: Some(DEFAULT_CONFIG_ID.to_string()),
        }
    }
}

pub fn get_axiom_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or_else(|| eyre::eyre!("Could not find home directory"))?;
    Ok(home.join(".axiom"))
}

pub fn get_config_path() -> PathBuf {
    get_axiom_dir().unwrap().join("config.json")
}

pub fn load_config_without_validation() -> Result<Config> {
    let config_path = get_config_path();

    if !config_path.exists() {
        // Try to load from old config format
        return Ok(Config::default());
    }

    let config_str = std::fs::read_to_string(config_path).context("Failed to read config file")?;

    serde_json::from_str(&config_str).context("Failed to parse config file")
}

pub fn load_config() -> Result<Config> {
    let config = load_config_without_validation()?;
    if config.api_key.is_none() {
        return Err(eyre::eyre!(
            "CLI not initialized. Run 'cargo axiom init' first."
        ));
    }
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
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
        .ok_or_else(|| eyre::eyre!("API key not found. Run 'cargo axiom init' first."))
}

pub fn set_config_id(id: String) -> Result<()> {
    let mut config = load_config()?;
    config.config_id = Some(id);
    save_config(&config)
}

pub fn get_config_id(args_config_id: Option<String>, config: &Config) -> Result<String> {
    if let Some(id) = args_config_id {
        set_config_id(id.clone())?;
        Ok(id)
    } else if let Some(id) = &config.config_id {
        println!("using cached config ID: {}", id);
        Ok(id.clone())
    } else {
        Err(eyre::eyre!("No config ID provided"))
    }
}
