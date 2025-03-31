use std::{fs::File, path::Path};

use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use flate2::{write::GzEncoder, Compression};
use reqwest::blocking::Client;
use tar::Builder;
use walkdir;

use crate::config::{get_api_key, load_config, API_KEY_HEADER};

#[derive(Debug, Parser)]
#[command(name = "build", about = "Build the project on Axiom Proving Service")]
pub struct BuildCmd {
    #[command(subcommand)]
    command: Option<BuildSubcommand>,

    #[clap(flatten)]
    build_args: BuildArgs,
}

#[derive(Debug, Subcommand)]
enum BuildSubcommand {
    /// Check the status of a build
    Status {
        /// The program ID to check status for
        #[clap(long, value_name = "ID")]
        program_id: String,
    },

    List,

    /// Download build artifacts
    Download {
        /// The program ID to download artifacts for
        #[clap(long, value_name = "ID")]
        program_id: String,

        /// The type of artifact to download (exe or elf)
        #[clap(long, value_name = "TYPE", value_parser = ["exe", "elf"])]
        program_type: String,
    },
}

impl BuildCmd {
    pub fn run(self) -> Result<()> {
        match self.command {
            Some(BuildSubcommand::Status { program_id }) => check_build_status(program_id),
            Some(BuildSubcommand::List) => list_builds(),
            Some(BuildSubcommand::Download {
                program_id,
                program_type,
            }) => download_program(program_id, program_type),
            None => execute(self.build_args),
        }
    }
}

fn list_builds() -> Result<()> {
    let config = load_config()?;
    let api_key = get_api_key()?;
    let url = format!("{}/programs", config.api_url);
    let response = Client::new()
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()?;
    let body = response.json::<serde_json::Value>()?;
    println!("{}", body);
    Ok(())
}

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// The configuration ID to use for the build
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,

    /// Keep the tar archive after uploading
    #[clap(long)]
    keep_tarball: Option<bool>,

    /// Comma-separated list of file patterns to exclude (e.g. "*.log,temp/*")
    #[clap(long, value_name = "PATTERNS")]
    exclude_files: Option<String>,
}

fn is_rust_project() -> bool {
    Path::new("Cargo.toml").exists()
}

fn find_git_root() -> Result<std::path::PathBuf> {
    // Start from the current directory
    let mut current_dir = std::env::current_dir()?;

    loop {
        // Check if .git directory exists in the current directory
        let git_dir = current_dir.join(".git");
        if git_dir.exists() && git_dir.is_dir() {
            return Ok(current_dir);
        }

        // Move up to parent directory
        if !current_dir.pop() {
            // We've reached the root of the filesystem without finding a .git directory
            return Err(eyre::eyre!("Not in a git repository"));
        }
    }
}

fn create_tar_archive(exclude_patterns: &[String]) -> Result<String> {
    let tar_path = "program.tar.gz";
    let tar_file = File::create(tar_path)?;
    let enc = GzEncoder::new(tar_file, Compression::default());
    let mut builder = Builder::new(enc);

    // Find the git root directory
    let git_root = find_git_root().context("Failed to find git root directory")?;

    // Get the git root directory name
    let dir_name = git_root
        .file_name()
        .ok_or_else(|| eyre::eyre!("Failed to get git root directory name"))?
        .to_string_lossy()
        .to_string();

    // Change to git root directory
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&git_root)?;

    // Walk through the directory and add files to the archive
    let walker = walkdir::WalkDir::new(".")
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            let path_str = path.to_string_lossy();

            // Skip dotfiles, target directories (anywhere in path), and the tar file itself
            let default_exclusion = file_name.starts_with(".")
                || path_str.contains("/target/")
                || path.starts_with("target/")
                || path_str.contains("/openvm/")
                || path.starts_with("openvm/")
                || path_str.contains("/program.tar.gz")
                || path.starts_with("program.tar.gz");

            // Check against user-provided exclusion patterns
            let matches_exclusion = exclude_patterns.iter().any(|s| path_str.contains(s));

            !(default_exclusion || matches_exclusion)
        });

    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        // TODO: print if verbose
        // println!("adding to tarball: {}", path.display());
        if path.is_file() {
            // Create path with the parent directory name
            let relative_path = path.strip_prefix(".").unwrap();
            let archive_path = format!("{}/{}", dir_name, relative_path.display());

            let mut file = File::open(path)?;
            builder.append_file(archive_path, &mut file)?;
        } else if path.is_dir() {
            // Skip directories that start with dot or are "target"
            let dir_name_str = path.file_name().unwrap_or_default().to_string_lossy();
            if dir_name_str.starts_with(".") || dir_name_str == "target" {
                continue;
            }

            // Create directory in the archive
            let relative_path = path.strip_prefix(".").unwrap();
            let archive_path = format!("{}/{}", dir_name, relative_path.display());
            builder.append_dir(archive_path, path)?;
        }
    }

    builder.finish()?;

    // Change back to the original directory
    std::env::set_current_dir(original_dir)?;

    Ok(tar_path.to_string())
}

pub fn execute(args: BuildArgs) -> Result<()> {
    // Check if we're in a Rust project
    if !is_rust_project() {
        return Err(eyre::eyre!(
            "Not in a Rust project. Make sure Cargo.toml exists."
        ));
    }

    // Get the config_id from args, return error if not provided
    let config_id = args
        .config_id
        .ok_or_else(|| eyre::eyre!("Config ID is required. Use --config-id to specify."))?;

    // Get the git root directory
    let git_root = find_git_root().context("Failed to find git root directory")?;

    // Get the current directory
    let current_dir = std::env::current_dir()?;

    // Calculate the relative path from git root to current directory
    let program_path = current_dir
        .strip_prefix(&git_root)
        .context("Failed to determine relative path from git root")?
        .to_string_lossy()
        .to_string();

    // Parse exclude patterns
    let exclude_patterns = args
        .exclude_files
        .map(|patterns| {
            patterns
                .split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    // Create tar archive of the current directory
    println!("Creating archive of the project...");
    let tar_path =
        create_tar_archive(&exclude_patterns).context("Failed to create project archive")?;

    // Use the staging API URL
    let config = load_config()?;

    // Add program_path as a query parameter if it's not empty
    let url = if program_path.is_empty() {
        format!("{}/programs?config_id={}", config.api_url, config_id)
    } else {
        format!(
            "{}/programs?config_id={}&program_path={}",
            config.api_url, config_id, program_path
        )
    };

    println!("Sending build request for config ID: {}", config_id);
    if !program_path.is_empty() {
        println!("Using program path: {}", program_path);
    }

    // Make the POST request with multipart form data
    let client = Client::new();
    let api_key = get_api_key()?;

    let form = reqwest::blocking::multipart::Form::new()
        .file("program", &tar_path)
        .context("Failed to attach program archive")?;

    let response = client
        .post(url)
        .header(API_KEY_HEADER, api_key)
        .multipart(form)
        .send()
        .context("Failed to send build request")?;

    // Clean up the tar file
    if !args.keep_tarball.unwrap_or(false) {
        std::fs::remove_file(tar_path).ok();
    }

    // Check if the request was successful
    if response.status().is_success() {
        let body = response.json::<serde_json::Value>().unwrap();
        let program_id = body["id"].as_str().unwrap();
        println!("Build request sent successfully: {}", program_id);
        println!(
            "To check the build status, run: cargo axiom build status --program-id {}",
            program_id
        );
        Ok(())
    } else {
        Err(eyre::eyre!(
            "Build request failed with status: {}",
            response.status()
        ))
    }
}

fn check_build_status(program_id: String) -> Result<()> {
    // Load configuration
    let config = load_config()?;
    let url = format!("{}/programs/{}", config.api_url, program_id);

    println!("Checking build status for program ID: {}", program_id);

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send status request")?;

    // Check if the request was successful
    if response.status().is_success() {
        println!("Build status: {}", response.text().unwrap());
        Ok(())
    } else {
        Err(eyre::eyre!(
            "Status request failed with status: {}",
            response.status()
        ))
    }
}

fn download_program(program_id: String, program_type: String) -> Result<()> {
    // Load configuration
    let config = load_config()?;
    let url = format!(
        "{}/programs/{}/{}",
        config.api_url, program_id, program_type
    );

    println!(
        "Downloading {} for program ID: {}",
        program_type, program_id
    );

    // Make the GET request
    let client = Client::new();
    let api_key = get_api_key()?;

    let response = client
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to download artifact")?;

    // Check if the request was successful
    if response.status().is_success() {
        // Create output filename based on program ID and artifact type
        let filename = format!("program_{}.{}", program_id, program_type);

        // Write the response body to a file
        let mut file = File::create(&filename)
            .context(format!("Failed to create output file: {}", filename))?;

        let content = response.bytes().context("Failed to read response body")?;

        std::io::copy(&mut content.as_ref(), &mut file)
            .context("Failed to write artifact to file")?;

        // Make the file executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&filename)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&filename, perms)?;
        }

        println!("Artifact downloaded successfully to: {}", filename);
        Ok(())
    } else {
        Err(eyre::eyre!(
            "Download request failed with status: {}",
            response.status()
        ))
    }
}
