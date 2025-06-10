use clap::Parser;
use eyre::{Context, Result};

use crate::{
    commands::build::find_git_root,
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

    #[clap(long)]
    bin: bool,

    #[clap(long)]
    lib: bool,

    #[clap(long)]
    name: Option<String>,

    #[clap(long, default_value = "2021")]
    edition: String,
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing Axiom configuration...");

    let openvm_available = std::process::Command::new("cargo")
        .args(["openvm", "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !openvm_available {
        eprintln!("Warning: cargo openvm not found. Please install openvm-cli to set up project structure.");
        eprintln!("You can install it with: cargo install --git https://github.com/openvm-org/openvm openvm-cli");
    } else {
        println!("Setting up OpenVM project structure...");
        let mut cmd = std::process::Command::new("cargo");
        cmd.args(["openvm", "init"]);

        if args.bin {
            cmd.arg("--bin");
        }
        if args.lib {
            cmd.arg("--lib");
        }
        if let Some(name) = &args.name {
            cmd.args(["--name", name]);
        }
        cmd.args(["--edition", &args.edition]);

        let status = cmd.status().context("Failed to run 'cargo openvm init'")?;
        if !status.success() {
            return Err(eyre::eyre!(
                "cargo openvm init failed with status: {}",
                status
            ));
        }
    }

    let git_root_result = crate::commands::build::find_git_root();
    if git_root_result.is_err() {
        println!("Initializing Git repository...");
        let status = std::process::Command::new("git")
            .args(["init"])
            .status()
            .context("Failed to run 'git init'")?;
        if !status.success() {
            eprintln!("Warning: Failed to initialize Git repository");
        }
    }

    let env_path = std::path::Path::new(".env");
    if !env_path.exists() {
        println!("Creating .env file...");
        std::fs::write(env_path, "AXIOM_API_KEY=\n").context("Failed to create .env file")?;
    } else {
        println!(".env file already exists, skipping creation");
    }

    let gitignore_path = std::path::Path::new(".gitignore");
    let mut gitignore_content = if gitignore_path.exists() {
        std::fs::read_to_string(gitignore_path).context("Failed to read .gitignore file")?
    } else {
        String::new()
    };

    let entries_to_add = vec!["./openvm", ".env"];
    let mut modified = false;

    for entry in entries_to_add {
        if !gitignore_content.lines().any(|line| line.trim() == entry) {
            if !gitignore_content.is_empty() && !gitignore_content.ends_with('\n') {
                gitignore_content.push('\n');
            }
            gitignore_content.push_str(entry);
            gitignore_content.push('\n');
            modified = true;
        }
    }

    if modified {
        println!("Updating .gitignore...");
        std::fs::write(gitignore_path, gitignore_content)
            .context("Failed to update .gitignore file")?;
    }

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
