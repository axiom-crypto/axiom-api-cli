//! Whole file is copied from cargo-openvm
use std::{path::PathBuf, str::FromStr};

/// Input can be either:
/// (1) one single hex string
/// (2) A JSON file containing an array of hex strings.
/// Each hex string (either in the file or the direct input) is either:
/// - Hex strings of bytes, which is prefixed with 0x01
/// - Hex strings of native field elements (represented as u32, little endian), prefixed with 0x02
#[derive(Debug, Clone)]
pub enum Input {
    FilePath(PathBuf),
    HexBytes(Vec<u8>),
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
