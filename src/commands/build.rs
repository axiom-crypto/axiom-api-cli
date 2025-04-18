use std::{fs::File, path::Path};

use clap::{Parser, Subcommand};
use comfy_table;
use eyre::{Context, Result};
use flate2::{write::GzEncoder, Compression};
use reqwest::blocking::Client;
use tar::Builder;
use walkdir;

use crate::config::{get_api_key, get_config_id, load_config, API_KEY_HEADER};

const MAX_PROGRAM_SIZE_MB: u64 = 10;

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
        #[clap(long, value_name = "TYPE", value_parser = ["exe", "elf", "source", "app_exe_commit"])]
        program_type: String,
    },

    Logs {
        /// The program ID to download logs for
        #[clap(long, value_name = "ID")]
        program_id: String,
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
            Some(BuildSubcommand::Logs { program_id }) => download_logs(program_id),
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

    // Extract the items array from the response
    if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            println!("No builds found");
            return Ok(());
        }

        // Create a new table
        let mut table = comfy_table::Table::new();
        table.set_header(["ID", "Status", "Created At"]);

        // Add rows to the table
        for item in items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("-");
            let created_at = item
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("-");

            table.add_row([id, status, created_at]);
        }

        // Print the table
        println!("{}", table);
    } else {
        println!("Unexpected response format: {}", body);
    }

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

    /// Watch the build progress and wait until it's done
    #[clap(short, long)]
    watch: bool,
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

    // Get list of files tracked by git
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .output()
        .context("Failed to run 'git ls-files'")?;

    if !output.status.success() {
        return Err(eyre::eyre!("Failed to get git tracked files"));
    }

    let tracked_files: std::collections::HashSet<String> = String::from_utf8(output.stdout)?
        .lines()
        .map(|s| s.to_string())
        .collect();

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

            // Check if file is tracked by git (directories are allowed to continue traversal)
            let is_tracked =
                path.is_dir() || tracked_files.contains(path_str.trim_start_matches("./"));

            !(default_exclusion || matches_exclusion || !is_tracked)
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
    let config = load_config()?;

    // Check if we're in a Rust project
    if !is_rust_project() {
        return Err(eyre::eyre!(
            "Not in a Rust project. Make sure Cargo.toml exists."
        ));
    }

    // Get the config_id from args, return error if not provided
    let config_id = get_config_id(args.config_id, &config)?;

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

    // Check if the tar file size exceeds 10MB
    let metadata = std::fs::metadata(&tar_path).context("Failed to get tar file metadata")?;
    if metadata.len() > MAX_PROGRAM_SIZE_MB * 1024 * 1024 {
        std::fs::remove_file(tar_path).ok();
        return Err(eyre::eyre!(
            "Project archive size ({}) exceeds maximum allowed size of {}MB",
            metadata.len(),
            MAX_PROGRAM_SIZE_MB
        ));
    }

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

        if args.watch {
            // Poll the build status until it's done
            println!("Watching build status...");
            watch_build_status(program_id.to_string())?;
        } else {
            println!(
                "To check the build status, run: cargo axiom build status --program-id {}",
                program_id
            );
        }
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Build request failed with status: {}",
            response.status()
        ))
    }
}

// Helper function to make API requests
fn make_api_request(endpoint: &str) -> Result<reqwest::blocking::Response> {
    let config = load_config()?;
    let api_key = get_api_key()?;
    let url = format!("{}{}", config.api_url, endpoint);

    let response = Client::new()
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .context("Failed to send API request")?;

    if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        return Err(eyre::eyre!("Client error ({}): {}", status, error_text));
    } else if !response.status().is_success() {
        return Err(eyre::eyre!(
            "API request failed with status: {}",
            response.status()
        ));
    }

    Ok(response)
}

// Helper function to get program status
fn get_program_status(program_id: &str) -> Result<serde_json::Value> {
    let response = make_api_request(&format!("/programs/{}", program_id))?;
    let status_json = response
        .json::<serde_json::Value>()
        .context("Failed to parse status response")?;
    Ok(status_json)
}

// Helper function to fetch logs
fn fetch_logs(program_id: &str) -> Result<String> {
    let response = make_api_request(&format!("/programs/{}/logs", program_id))?;
    let logs = response.text().context("Failed to read logs content")?;
    Ok(logs)
}

fn check_build_status(program_id: String) -> Result<()> {
    println!("Checking build status for program ID: {}", program_id);

    let status = get_program_status(&program_id)?;
    println!("Build status: {}", status);
    Ok(())
}

fn watch_build_status(program_id: String) -> Result<()> {
    // Poll interval in seconds
    let poll_interval = std::time::Duration::from_secs(5);

    loop {
        // Get status using helper function
        let body = get_program_status(&program_id)?;
        let status = body
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");

        // Print the current status with timestamp
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        println!("[{}] Build status: {}", now, status);

        // If the build is done (ready, failed, error) or unknown, break the loop
        if status == "ready" || status == "failed" || status == "error" || status == "unknown" {
            if status == "ready" {
                println!("Build completed successfully!");
            } else if status == "unknown" {
                println!("Build status is unknown. Please check manually.");
            } else {
                println!("Build failed with status: {}", status);

                // Automatically fetch and display logs on failure
                println!("\nFetching build logs...");
                match fetch_and_print_logs(&program_id) {
                    Ok(_) => {}
                    Err(e) => println!("Failed to fetch logs: {}", e),
                }
            }
            break;
        }

        // Wait before polling again
        std::thread::sleep(poll_interval);
    }

    Ok(())
}

fn download_program(program_id: String, program_type: String) -> Result<()> {
    // Load configuration
    let config = load_config()?;
    let url = format!(
        "{}/programs/{}/download/{}",
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
        let ext = if program_type == "source" {
            "tar.gz".to_string()
        } else {
            program_type
        };
        let filename = format!("program_{}.{}", program_id, ext);

        // Write the response body to a file
        let mut file = File::create(&filename)
            .context(format!("Failed to create output file: {}", filename))?;

        let content = response.bytes().context("Failed to read response body")?;

        std::io::copy(&mut content.as_ref(), &mut file)
            .context("Failed to write artifact to file")?;

        println!("Artifact downloaded successfully to: {}", filename);
        Ok(())
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
    } else {
        Err(eyre::eyre!(
            "Download request failed with status: {}",
            response.status()
        ))
    }
}

fn download_logs(program_id: String) -> Result<()> {
    let logs = fetch_logs(&program_id)?;

    // Create output filename based on program ID
    let filename = format!("program_{}_logs.txt", program_id);

    // Write the logs to a file
    let mut file =
        File::create(&filename).context(format!("Failed to create log file: {}", filename))?;

    std::io::copy(&mut logs.as_bytes(), &mut file).context("Failed to write logs to file")?;

    println!("Logs downloaded successfully to: {}", filename);
    Ok(())
}

// Fetch and print logs directly to the console
fn fetch_and_print_logs(program_id: &str) -> Result<()> {
    let logs = fetch_logs(program_id)?;

    // Print logs to console with some formatting
    println!("\n==== BUILD LOGS ====");
    println!("{}", logs);
    println!("==== END OF LOGS ====");

    Ok(())
}
