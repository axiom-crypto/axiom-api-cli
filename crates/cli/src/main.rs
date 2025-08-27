use std::process;

use axiom_sdk::set_cli_version;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Generator, Shell, generate};
use dotenvy::dotenv;
use eyre::Result;
use std::fs;

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
    /// Manage VM configuration artifacts
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
    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn generate_completions<G: Generator>(
    generator: G,
    cmd: &mut clap::Command,
    shell: Shell,
) -> Result<()> {
    let bin_name = cmd.get_name().to_string();
    let filename = match shell {
        Shell::Bash => "cargo-axiom.bash",
        Shell::Zsh => "_cargo-axiom",
        Shell::Fish => "cargo-axiom.fish",
        Shell::PowerShell => "cargo-axiom.ps1",
        Shell::Elvish => "cargo-axiom.elv",
        _ => "cargo-axiom.completion",
    };

    let mut file = fs::File::create(filename)?;
    generate(generator, cmd, bin_name, &mut file);

    println!("✅ Generated completion file: {filename}");
    println!();

    match shell {
        Shell::Bash => {
            println!("To install bash completions:");
            println!("  # Option 1: Copy to system completion directory");
            println!("  sudo cp {filename} /etc/bash_completion.d/");
            println!("  # Option 2: Copy to user completion directory");
            println!("  mkdir -p ~/.bash_completion.d");
            println!("  cp {filename} ~/.bash_completion.d/");
            println!("  # Option 3: Source directly in ~/.bashrc");
            println!("  echo 'source $(pwd)/{filename}' >> ~/.bashrc");
        }
        Shell::Zsh => {
            println!("To install zsh completions:");
            println!("  # Option 1: Copy to system completion directory");
            println!("  sudo cp {filename} /usr/local/share/zsh/site-functions/");
            println!("  # Option 2: Copy to user completion directory");
            println!("  mkdir -p ~/.zfunc");
            println!("  cp {filename} ~/.zfunc/");
            println!("  echo 'fpath=(~/.zfunc $fpath)' >> ~/.zshrc");
            println!("  echo 'autoload -U compinit && compinit' >> ~/.zshrc");
        }
        Shell::Fish => {
            println!("To install fish completions:");
            println!("  mkdir -p ~/.config/fish/completions");
            println!("  cp {filename} ~/.config/fish/completions/");
        }
        Shell::PowerShell => {
            println!("To install PowerShell completions:");
            println!("  # Add this line to your PowerShell profile:");
            println!("  . $(pwd)/{filename}");
        }
        Shell::Elvish => {
            println!("To install elvish completions:");
            println!("  # Add this line to your ~/.config/elvish/rc.elv:");
            println!("  eval (slurp < $(pwd)/{filename})");
        }
        _ => {
            println!("To install completions:");
            println!("  # Please refer to your shell's documentation for completion installation");
            println!("  # The completion file has been saved as: {filename}");
        }
    }

    println!();
    println!("💡 After installation:");
    println!(
        "  • Restart your shell OR run 'source ~/.{}rc' to activate",
        shell.to_string().to_lowercase()
    );
    println!("  • Try typing 'cargo axiom ' and press TAB for autocompletion");

    Ok(())
}

fn main() {
    dotenv().ok();

    // Make CLI version available to the SDK for request headers
    set_cli_version(env!("CARGO_PKG_VERSION"));

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
        AxiomCommands::Completions { shell } => {
            let mut cmd = Cargo::command();
            generate_completions(shell, &mut cmd, shell)
        }
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
