use axiom_sdk::{AxiomConfig, DEFAULT_CONFIG_ID, STAGING_DEFAULT_CONFIG_ID};
use clap::Parser;
use eyre::{OptionExt, Result};

const STAGING_API_URL: &str = "https://api.staging.app.axiom.xyz/v1";
const PROD_API_URL: &str = "https://api.axiom.xyz/v1";

#[derive(Debug, Parser)]
#[command(name = "register", about = "Register Axiom API credentials")]
pub struct RegisterCmd {
    #[clap(flatten)]
    register_args: RegisterArgs,
}

impl RegisterCmd {
    pub fn run(self, _output_mode: crate::output::OutputMode) -> Result<()> {
        // Register command doesn't support JSON output - it's for setup
        execute(self.register_args)
    }
}

#[derive(Debug, Parser)]
pub struct RegisterArgs {
    /// The API URL to use (defaults to https://api.axiom.xyz/v1)
    #[clap(long, value_name = "URL")]
    api_url: Option<String>,

    /// Axiom API key
    #[clap(long, value_name = "KEY")]
    api_key: Option<String>,

    /// Whether to use staging API
    #[clap(long)]
    staging: bool,
}

pub fn execute(args: RegisterArgs) -> Result<()> {
    println!("Registering Axiom API credentials...");

    // Use provided API URL or default
    let api_url = args.api_url.clone().unwrap_or_else(|| {
        if args.staging {
            STAGING_API_URL.to_string()
        } else {
            PROD_API_URL.to_string()
        }
    });

    // Get API key from args or env var AXIOM_API_KEY
    let api_key = args.api_key
        .or_else(|| std::env::var("AXIOM_API_KEY").ok())
        .ok_or_eyre("API key must be provided either with --api-key flag or AXIOM_API_KEY environment variable")?;

    // Validate the API key with the backend
    println!("Validating API key...");
    axiom_sdk::validate_api_key(&api_url, &api_key)
        .map_err(|e| eyre::eyre!("Invalid API key - {}", e))?;

    println!("API key is valid!");

    // Create and save the configuration
    let config_id = if args.staging {
        Some(STAGING_DEFAULT_CONFIG_ID.to_string())
    } else {
        Some(DEFAULT_CONFIG_ID.to_string())
    };

    let mut config = AxiomConfig::new(api_url, Some(api_key), config_id);
    config.console_base_url = if args.staging {
        Some("https://axiom-proving-service-staging.vercel.app".to_string())
    } else if args.api_url.is_none() {
        // default to prod
        Some("https://prove.axiom.xyz".to_string())
    } else {
        // custom API URL provided, don't set console base url
        None
    };

    axiom_sdk::save_config(&config)?;

    println!("Axiom API credentials registered successfully!");

    Ok(())
}
