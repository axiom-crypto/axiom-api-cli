use std::{fs, path::Path, process::Command};

use clap::Parser;
use eyre::{OptionExt, Result, WrapErr, bail};
use toml_edit::{DocumentMut, Item, Table, Value};

const MAIN_RS_PREPEND: &str = r#"openvm::init!();

"#;

const OPENVM_STANDARD_TOML_URL: &str = "https://raw.githubusercontent.com/openvm-org/openvm/main/crates/sdk/src/config/v1.5/openvm_standard.toml";

fn fetch_openvm_toml_template() -> Result<String> {
    let output = Command::new("curl")
        .args(["-fsSL", OPENVM_STANDARD_TOML_URL])
        .output()
        .wrap_err("failed to run curl to fetch openvm_standard.toml")?;

    if !output.status.success() {
        bail!(
            "failed to fetch openvm_standard.toml from {}",
            OPENVM_STANDARD_TOML_URL
        );
    }

    String::from_utf8(output.stdout).wrap_err("openvm_standard.toml response was not valid UTF-8")
}

#[derive(Debug, Parser)]
#[command(name = "init", about = "Initialize a new OpenVM project")]
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
    /// Path to create the package in
    #[clap(value_name = "PATH")]
    path: Option<String>,

    /// Set the package name, default is the directory name
    #[clap(long, value_name = "NAME")]
    name: Option<String>,
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing OpenVM project...");

    // Check if cargo openvm is installed
    let check_status = Command::new("cargo")
        .arg("openvm")
        .arg("--help")
        .output()
        .map_err(|_| eyre::eyre!("cargo openvm is not installed. Please install it first."))?;

    if !check_status.status.success() {
        bail!("cargo openvm is not installed or not working properly.");
    }

    // Build the cargo openvm init command
    let mut cmd = Command::new("cargo");
    cmd.arg("openvm").arg("init");

    // Add path if provided
    if let Some(path) = &args.path {
        cmd.arg(path);
    }

    // Add name if provided
    if let Some(name) = &args.name {
        cmd.arg("--name").arg(name);
    }

    // Execute cargo openvm init
    let status = cmd.status()?;
    if !status.success() {
        bail!("Failed to initialize OpenVM project");
    }

    // Determine the project directory
    let project_dir = if let Some(path) = &args.path {
        Path::new(path).to_path_buf()
    } else {
        std::env::current_dir()?
    };

    // Modify src/main.rs to prepend the required imports
    let main_rs_path = project_dir.join("src").join("main.rs");
    if main_rs_path.exists() {
        let existing_content = fs::read_to_string(&main_rs_path)?;
        let new_content = format!("{}{}", MAIN_RS_PREPEND, existing_content);
        fs::write(&main_rs_path, new_content)?;
    }

    // Modify Cargo.toml to add additional dependencies
    let cargo_toml_path = project_dir.join("Cargo.toml");
    if cargo_toml_path.exists() {
        let cargo_content = fs::read_to_string(&cargo_toml_path)?;
        let mut doc = cargo_content.parse::<DocumentMut>()?;

        // Extract tag from existing openvm dependency
        let extracted_tag = doc
            .get("dependencies")
            .and_then(|deps| deps.get("openvm"))
            .and_then(|openvm| openvm.get("tag"))
            .and_then(|tag| tag.as_str())
            .map(|s| s.to_string());

        // Get or create dependencies table
        let deps = doc["dependencies"]
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| eyre::eyre!("Failed to access dependencies table"))?;

        // Add each dependency with proper TOML structure
        let git_url = "https://github.com/openvm-org/openvm.git";

        // Helper to create a dependency entry
        let create_dep = |tag: Option<&str>| -> Item {
            let mut table = toml_edit::InlineTable::new();
            table.insert("git", git_url.into());
            if let Some(t) = tag {
                table.insert("tag", t.into());
            }
            table.insert("default-features", false.into());
            Item::Value(Value::InlineTable(table))
        };

        let tag_as_str = extracted_tag.as_deref();

        deps["openvm-algebra-guest"] = create_dep(tag_as_str);
        deps["openvm-ecc-guest"] = create_dep(tag_as_str);

        // For openvm-pairing with features
        let mut pairing_table = toml_edit::InlineTable::new();
        pairing_table.insert("git", git_url.into());
        if let Some(t) = &extracted_tag {
            pairing_table.insert("tag", t.into());
        }
        let features = toml_edit::Array::from_iter(["bn254", "bls12_381"]);
        pairing_table.insert("features", Value::Array(features));
        deps["openvm-pairing"] = Item::Value(Value::InlineTable(pairing_table));

        // For packages with different names
        let create_dep_with_package = |package: &str, tag: Option<&str>| -> Item {
            let mut table = toml_edit::InlineTable::new();
            table.insert("git", git_url.into());
            if let Some(t) = tag {
                table.insert("tag", t.into());
            }
            table.insert("package", package.into());
            Item::Value(Value::InlineTable(table))
        };

        deps["openvm-k256"] = create_dep_with_package("k256", tag_as_str);
        deps["openvm-p256"] = create_dep_with_package("p256", tag_as_str);

        // Write back preserving formatting
        fs::write(&cargo_toml_path, doc.to_string())?;
    }

    // Create .env.example if it doesn't exist
    let env_example_path = project_dir.join(".env.example");
    if !env_example_path.exists() {
        fs::write(&env_example_path, "AXIOM_API_KEY=\n")?;
    }

    // Create .env if it doesn't exist
    let env_path = project_dir.join(".env");
    if !env_path.exists() {
        fs::write(&env_path, "AXIOM_API_KEY=\n")?;
    }

    // Update .gitignore to include .env and ./openvm
    let gitignore_path = project_dir.join(".gitignore");
    if gitignore_path.exists() {
        let gitignore_content = fs::read_to_string(&gitignore_path)?;
        let additions = vec![".env", "./openvm", "/.axiom", "proof.json"];
        let mut updated_content = gitignore_content;

        for addition in &additions {
            if !updated_content.contains(addition) {
                updated_content.push_str(&format!("{}\n", addition));
            }
        }

        fs::write(&gitignore_path, updated_content)?;
    }

    // Create or replace openvm.toml file
    let openvm_toml_template = fetch_openvm_toml_template()?;
    let openvm_toml_path = project_dir.join("openvm.toml");
    fs::write(&openvm_toml_path, openvm_toml_template)?;

    // Run `cargo fetch` so that `Cargo.lock` will be created
    let toolchain_file_content = include_str!("../../../../rust-toolchain.toml");
    let doc = toolchain_file_content
        .parse::<toml_edit::Document<_>>()
        .context("Failed to parse rust-toolchain.toml")?;
    let required_version_str = doc["toolchain"]["channel"]
        .as_str()
        .ok_or_eyre("Could not find 'toolchain.channel' in rust-toolchain.toml")?;
    let _ = Command::new("cargo")
        .current_dir(&project_dir)
        .arg(format!("+{}", required_version_str))
        .arg("generate-lockfile")
        .status();

    // Attempt to stage and commit initialized files. Ignore failures (e.g., not a git repo or nothing to commit).
    let _ = Command::new("git")
        .current_dir(&project_dir)
        .args(["add", "."])
        .status();
    let _ = Command::new("git")
        .current_dir(&project_dir)
        .args(["commit", "-q", "-m", "initial commit"])
        .status();

    Ok(())
}
