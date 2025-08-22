use std::process;

use clap::{Args, Parser, Subcommand};
use dotenvy::dotenv;

mod commands;

use commands::{
    BuildCmd, ConfigCmd, DownloadKeysCmd, InitCmd, ProjectsCmd, ProveCmd, RegisterCmd, RunCmd,
    VerifyCmd, VersionCmd,
};

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
    /// Initialize a new OpenVM project
    Init(InitCmd),
    /// Register Axiom API credentials
    Register(RegisterCmd),
    /// Build the project on Axiom Proving Service
    Build(BuildCmd),
    /// Generate a proof using the Axiom Proving Service
    Prove(ProveCmd),
    /// Execute a program using the Axiom Execution Service
    Run(RunCmd),
    /// Generate key artifacts
    Config(ConfigCmd),
    /// Download proving keys
    #[command(name = "download-keys")]
    DownloadKeys(DownloadKeysCmd),
    /// Verify a proof using the Axiom Verifying Service
    Verify(VerifyCmd),
    /// Manage projects
    Projects(ProjectsCmd),
    /// Display version information
    Version(VersionCmd),
}

fn main() {
    dotenv().ok();

    let Cargo::Axiom(args) = Cargo::parse();

    let result = match args.command {
        AxiomCommands::Init(cmd) => cmd.run(),
        AxiomCommands::Register(cmd) => cmd.run(),
        AxiomCommands::Build(cmd) => cmd.run(),
        AxiomCommands::Prove(cmd) => cmd.run(),
        AxiomCommands::Run(cmd) => cmd.run(),
        AxiomCommands::Config(cmd) => cmd.run(),
        AxiomCommands::DownloadKeys(cmd) => cmd.run(),
        AxiomCommands::Verify(cmd) => cmd.run(),
        AxiomCommands::Projects(cmd) => cmd.run(),
        AxiomCommands::Version(cmd) => cmd.run(),
    };

    if let Err(err) = result {
        if args.debug {
            // In debug mode, print the full error with backtrace
            eprintln!("Error: {err:?}");
        } else {
            // In normal mode, just print the error message
            eprintln!("Error: {err}");
        }
        process::exit(1);
    }
}
