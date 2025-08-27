use std::process;

use axiom_sdk::set_cli_version;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use dotenvy::dotenv;
use eyre::Result;
use std::{fs, path::PathBuf};

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

fn generate_completions(shell: Shell, cmd: &mut clap::Command) -> Result<PathBuf> {
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
    generate(shell, cmd, bin_name, &mut file);

    println!("✅ Generated completion file: {filename}");
    println!();

    match shell {
        Shell::Bash => {
            println!("To install bash completions:");
            println!("  # Linux system-wide:");
            println!("  sudo cp {filename} /etc/bash_completion.d/");
            println!();
            println!("  # macOS with Homebrew:");
            println!("  cp {filename} $(brew --prefix)/etc/bash_completion.d/");
            println!();
            println!("  # Or source directly in ~/.bashrc:");
            println!("  echo 'source $(pwd)/{filename}' >> ~/.bashrc");
            println!();
            println!("💡 Activate: Restart your shell OR run 'source ~/.bashrc'");
        }
        Shell::Zsh => {
            println!("To install zsh completions:");
            println!("  # Linux system-wide:");
            println!("  sudo cp {filename} /usr/local/share/zsh/site-functions/");
            println!();
            println!("  # macOS with Homebrew:");
            println!("  cp {filename} $(brew --prefix)/share/zsh/site-functions/");
            println!();
            println!("  # Or user-local:");
            println!("  mkdir -p ~/.zfunc");
            println!("  cp {filename} ~/.zfunc/");
            println!("  echo 'fpath=(~/.zfunc $fpath)' >> ~/.zshrc");
            println!("  echo 'autoload -U compinit && compinit' >> ~/.zshrc");
            println!();
            println!("💡 Activate: Restart your shell OR run 'source ~/.zshrc'");
        }
        Shell::Fish => {
            println!("To install fish completions:");
            println!("  mkdir -p ~/.config/fish/completions");
            println!("  cp {filename} ~/.config/fish/completions/");
            println!();
            println!("💡 Fish loads completions automatically. To reload: 'exec fish'");
        }
        Shell::PowerShell => {
            println!("To install PowerShell completions:");
            println!("  # Add to your profile (permanent):");
            println!(r#"  Add-Content -Path $PROFILE -Value ". '$(pwd)/{filename}'""#);
            println!();
            println!("  # Or load for current session:");
            println!(r#"  . "$(pwd)/{filename}""#);
        }
        Shell::Elvish => {
            println!("To install elvish completions:");
            println!("  # Add this line to ~/.config/elvish/rc.elv:");
            println!(r#"  eval (slurp < "$(pwd)/{filename}")"#);
        }
        _ => {
            println!("Completion file saved as: {filename}");
            println!("Please refer to your shell's documentation for installation.");
        }
    }

    println!();
    println!("  • Try typing 'cargo axiom ' and press TAB to test autocompletion");

    Ok(PathBuf::from(filename))
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
            generate_completions(shell, &mut cmd).map(|_| ())
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
