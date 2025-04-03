use clap::Parser;
use eyre::Result;

use crate::config;

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
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing Axiom configuration...");

    // Use provided API URL or default
    // TODO: default should be prod
    let api_url = args
        .api_url
        .unwrap_or_else(|| "https://api.staging.app.axiom.xyz".to_string());

    // Get API key from args or env var AXIOM_API_KEY
    let api_key = args.api_key.or_else(|| std::env::var("AXIOM_API_KEY").ok());

    if api_key.is_none() {
        eprintln!("Error: API key must be provided either with --api-key flag or AXIOM_API_KEY environment variable");
        std::process::exit(1);
    }

    // Create and save the configuration
    let config = config::Config {
        api_key: Some(api_key.unwrap()),
        api_url,
        config_id: Some("c77596d5-511f-4ab3-87fe-6bb0702cfab2".to_string()),
    };

    config::save_config(&config)?;

    println!("Axiom configuration initialized successfully!");

    Ok(())
}
