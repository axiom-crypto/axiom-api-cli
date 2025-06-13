use clap::Parser;
use eyre::Result;

use crate::{
    config,
    config::{load_config_without_validation, DEFAULT_CONFIG_ID, STAGING_DEFAULT_CONFIG_ID},
};

const STAGING_API_URL: &str = "https://api.staging.app.axiom.xyz/v1";
const PROD_API_URL: &str = "https://api.axiom.xyz/v1";

#[derive(Debug, Parser)]
#[command(name = "init", about = "Initialize Axiom configuration")]
pub struct InitCmd {
    #[clap(flatten)]
    init_args: InitArgs,
}

impl InitCmd {
    pub fn run(self) -> Result<()> {
        execute(self.init_args)
    }
}

#[derive(Debug, Parser)]
pub struct InitArgs {
    /// The API URL to use (defaults to https://api.staging.app.axiom.xyz)
    #[clap(long, value_name = "URL")]
    api_url: Option<String>,

    /// Axiom API key
    #[clap(long, value_name = "KEY")]
    api_key: Option<String>,

    /// Whether to use staging API
    #[clap(long)]
    staging: bool,
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing Axiom configuration...");

    // Use provided API URL or default
    let api_url = args.api_url.unwrap_or_else(|| {
        if args.staging {
            STAGING_API_URL.to_string()
        } else {
            PROD_API_URL.to_string()
        }
    });

    // Get API key from args or env var AXIOM_API_KEY
    let api_key = args.api_key.or_else(|| std::env::var("AXIOM_API_KEY").ok());

    if api_key.is_none() {
        eprintln!("Error: API key must be provided either with --api-key flag or AXIOM_API_KEY environment variable");
        std::process::exit(1);
    }

    let mut config = load_config_without_validation().unwrap_or_else(|_| config::Config {
        api_url: api_url.clone(),
        api_key: None,
        config_id: None,
    });

    config.api_key = Some(api_key.unwrap());
    config.api_url = api_url;
    config.config_id = if args.staging {
        Some(STAGING_DEFAULT_CONFIG_ID.to_string())
    } else {
        Some(DEFAULT_CONFIG_ID.to_string())
    };

    config::save_config(&config)?;

    println!("Axiom configuration initialized successfully!");

    Ok(())
}
