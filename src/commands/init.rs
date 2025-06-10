use clap::Parser;
use eyre::{Context, Result};


#[derive(Debug, Parser)]
#[command(name = "init", about = "Initialize an Axiom project with OpenVM integration")]
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
    #[clap(value_name = "PATH")]
    path: Option<std::path::PathBuf>,

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
    println!("Initializing Axiom project...");

    let openvm_available = std::process::Command::new("cargo")
        .args(["openvm", "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !openvm_available {
        eprintln!("Error: cargo openvm not found. Please install openvm-cli first.");
        eprintln!("You can install it with: cargo install --git https://github.com/openvm-org/openvm openvm-cli");
        std::process::exit(1);
    }

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
    
    if let Some(path) = &args.path {
        cmd.arg(path);
    }

    let status = cmd.status().context("Failed to run 'cargo openvm init'")?;
    if !status.success() {
        return Err(eyre::eyre!(
            "cargo openvm init failed with status: {}",
            status
        ));
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

    println!("Axiom project initialized successfully!");
    println!("Next steps:");
    println!("1. Run 'cargo axiom register' to configure your API credentials");
    println!("2. Run 'cargo axiom build' to build your project");

    Ok(())
}
