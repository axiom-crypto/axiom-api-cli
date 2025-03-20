use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod commands;
mod config;

use commands::{BuildCmd, InitCmd};

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    #[command(name = "axiom")]
    Axiom(AxiomArgs),
}

#[derive(Args)]
#[command(author, about, long_about = None)] // TODO: Add version
struct AxiomArgs {
    #[command(subcommand)]
    command: AxiomCommands,
}

#[derive(Subcommand)]
enum AxiomCommands {
    /// Initialize Axiom configuration
    Init(InitCmd),
    /// Build the project on Axiom Proving Service
    Build(BuildCmd),
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cargo::Axiom(args) = Cargo::parse();

    match args.command {
        AxiomCommands::Build(cmd) => cmd.run(),
        AxiomCommands::Init(cmd) => cmd.run(),
    }
}
