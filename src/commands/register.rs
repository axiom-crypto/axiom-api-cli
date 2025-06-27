use clap::Parser;
use eyre::Result;

use crate::{
    config,
    config::{DEFAULT_CONFIG_ID, STAGING_DEFAULT_CONFIG_ID},
};

const STAGING_API_URL: &str = "https://api.staging.app.axiom.xyz/v1";
const PROD_API_URL: &str = "https://api.axiom.xyz/v1";

#[derive(Debug, Parser)]
#[command(name = "register", about = "Register and configure Axiom API credentials")]
pub struct RegisterCmd {
    #[clap(flatten)]
    register_args: RegisterArgs,
}

impl RegisterCmd {
    pub fn run(self) -> Result<()> {
        execute(self.register_args)
    }
}

#[derive(Debug, Parser)]
pub struct RegisterArgs {
    #[clap(long, value_name = "URL")]
    api_url: Option<String>,

    #[clap(long, value_name = "KEY")]
    api_key: Option<String>,

    #[clap(long)]
    staging: bool,
}

pub fn execute(args: RegisterArgs) -> Result<()> {
    println!("Registering Axiom API configuration...");

    let api_url = args.api_url.unwrap_or_else(|| {
        if args.staging {
            STAGING_API_URL.to_string()
        } else {
            PROD_API_URL.to_string()
        }
    });

    let api_key = args.api_key.or_else(|| std::env::var("AXIOM_API_KEY").ok());

    if api_key.is_none() {
        eprintln!("Error: API key must be provided either with --api-key flag or AXIOM_API_KEY environment variable");
        std::process::exit(1);
    }

    let config = config::Config {
        api_key: Some(api_key.unwrap()),
        api_url,
        config_id: if args.staging {
            Some(STAGING_DEFAULT_CONFIG_ID.to_string())
        } else {
            Some(DEFAULT_CONFIG_ID.to_string())
        },
    };

    config::save_config(&config)?;

    println!("Axiom API configuration registered successfully!");

    Ok(())
}
