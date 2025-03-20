use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
enum Cargo {
    #[command(name = "axiom")]
    Axiom(AxiomArgs),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct AxiomArgs {
    #[command(subcommand)]
    command: AxiomCommands,
}

#[derive(Subcommand)]
enum AxiomCommands {
    /// Build the project with Axiom
    Build {
        /// Optional build arguments
        #[arg(last = true)]
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    let Cargo::Axiom(args) = Cargo::parse();
    
    match args.command {
        AxiomCommands::Build { args } => commands::build::execute(args),
    }
}
