use std::process;

use clap::{Args, Parser, Subcommand};
use dotenv::dotenv;

mod commands;
mod config;

use commands::{BuildCmd, ConfigCmd, InitCmd, ProveCmd, VerifyCmd};

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    #[command(name = "axiom")]
    Axiom(AxiomArgs),
}

#[derive(Args)]
#[command(author, about, long_about = None)] // TODO: Add version
struct AxiomArgs {
    /// Enable debug mode to show full error traces
    #[arg(long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: AxiomCommands,
}

#[derive(Subcommand)]
enum AxiomCommands {
    /// Initialize Axiom configuration
    Init(InitCmd),
    /// Build the project on Axiom Proving Service
    Build(BuildCmd),
    /// Generate a proof using the Axiom Proving Service
    Prove(ProveCmd),
    /// Generate key artifacts
    Config(ConfigCmd),
    /// Verify a proof using the Axiom Verifying Service
    Verify(VerifyCmd),
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let Cargo::Axiom(args) = Cargo::parse();

    let result = match args.command {
        AxiomCommands::Build(cmd) => cmd.run(),
        AxiomCommands::Init(cmd) => cmd.run(),
        AxiomCommands::Prove(cmd) => cmd.run(),
        AxiomCommands::Config(cmd) => cmd.run(),
        AxiomCommands::Verify(cmd) => cmd.run(),
    };

    if let Err(err) = result {
        if args.debug {
            // In debug mode, print the full error with backtrace
            eprintln!("Error: {:?}", err);
        } else {
            // In normal mode, just print the error message
            eprintln!("Error: {}", err);
        }
        process::exit(1);
    }
}
