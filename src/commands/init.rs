use clap::Parser;
use dialoguer::Password;
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
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing Axiom configuration...");

    // Ask for API key
    let api_key = Password::new()
        .with_prompt("Enter your Axiom API key")
        .interact()?;

    // Use provided API URL or default
    // TODO: default should be prod
    let api_url = args
        .api_url
        .unwrap_or_else(|| "https://api.staging.app.axiom.xyz".to_string());

    // Create and save the configuration
    let config = config::Config {
        api_key: Some(api_key),
        api_url,
    };

    config::save_config(&config)?;

    println!("Axiom configuration initialized successfully!");

    Ok(())
}
