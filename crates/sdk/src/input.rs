//! Whole file is copied from cargo-openvm
use std::{fs, path::PathBuf, str::FromStr};

use eyre::Context;
use serde_json::json;

use crate::validate_input_json;

/// Input can be either:
/// (1) one single hex string
/// (2) A JSON file containing an array of hex strings.
/// (3) A JSON value directly provided.
/// Each hex string (either in the file or the direct input) is either:
/// - Hex strings of bytes, which is prefixed with 0x01
/// - Hex strings of native field elements (represented as u32, little endian), prefixed with 0x02
#[derive(Debug, Clone)]
pub enum Input {
    /// Path to a JSON file containing input data
    FilePath(PathBuf),
    /// JSON value provided directly (for programmatic use)
    Value(serde_json::Value),
    /// Raw hex-encoded bytes
    HexBytes(Vec<u8>),
}

impl Input {
    /// Convert input to validated JSON format for API submission
    pub fn to_input_json(&self) -> eyre::Result<serde_json::Value> {
        let value = match self {
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
            Input::Value(input_json) => {
                validate_input_json(input_json)?;
                input_json.clone()
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
        };
        Ok(value)
    }
}

impl FromStr for Input {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(bytes) = decode_hex_string(s) {
            Ok(Input::HexBytes(bytes))
        } else if PathBuf::from(s).exists() {
            Ok(Input::FilePath(PathBuf::from(s)))
        } else {
            Err("Input must be a valid file path or a hex string of even length.".to_string())
        }
    }
}

pub fn decode_hex_string(s: &str) -> eyre::Result<Vec<u8>> {
    // Remove 0x prefix if present (exactly once)
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() % 2 != 0 {
        eyre::bail!("The hex string must be of even length");
    }
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        eyre::bail!("The hex string must consist of hex digits");
    }
    if s.starts_with("02") {
        if s.len() % 8 != 2 {
            eyre::bail!(
                "If the hex value starts with 02, a whole number of 32-bit elements must follow"
            );
        }
    } else if !s.starts_with("01") {
        eyre::bail!("The hex value must start with 01 or 02");
    }
    hex::decode(s).map_err(|e| eyre::eyre!("Invalid hex: {}", e))
}
