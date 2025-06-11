use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand};
use comfy_table;
use eyre::{Context, Result};
use flate2::{write::GzEncoder, Compression};
use openvm_build::cargo_command;
use reqwest::blocking::Client;
use tar::Builder;
use walkdir;

use crate::config::{get_api_key, get_config_id, load_config, API_KEY_HEADER};

const MAX_PROGRAM_SIZE_MB: u64 = 1024;

const AXIOM_CARGO_HOME: &str = "axiom_cargo_home";

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

    /// The binary to build, if there are multiple binaries in the project
    #[clap(long, value_name = "BIN")]
    bin: Option<String>,

    /// Keep the tar archive after uploading
    #[clap(long)]
    keep_tarball: Option<bool>,

    /// Comma-separated list of file patterns to exclude (e.g. "*.log,temp/*")
    #[clap(long, value_name = "PATTERNS")]
    exclude_files: Option<String>,

    /// Comma-separated list of directories to include even if not tracked by git
    #[clap(long, value_name = "DIRS")]
    include_dirs: Option<String>,
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

fn find_cargo_workspace_root() -> Result<std::path::PathBuf> {
    // Start from the current directory
    let mut current_dir = std::env::current_dir()?;
    // Keep track of the last directory with a Cargo.toml
    let mut last_cargo_dir = None;

    loop {
        // Check if Cargo.toml exists in the current directory
        let cargo_toml = current_dir.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check if this is a workspace root by reading the Cargo.toml file
            let mut content = String::new();
            File::open(&cargo_toml)?.read_to_string(&mut content)?;
            // If the file contains [workspace], it's a workspace root
            if content.contains("[workspace]") {
                return Ok(current_dir);
            }
            // Remember this directory as it has a Cargo.toml
            last_cargo_dir = Some(current_dir.clone());
        }
        // Move up to parent directory
        if !current_dir.pop() {
            // We've reached the root of the filesystem
            break;
        }
    }

    // If we found at least one Cargo.toml, return the topmost directory with one
    if let Some(dir) = last_cargo_dir {
        return Ok(dir);
    }

    // We didn't find any Cargo.toml
    Err(eyre::eyre!("Not in a Cargo project"))
}

// The tarball contains everything in the git root of the guest program that's tracked by git.
// Additionally, it does `cargo fetch` to pre-fetch dependencies so private dependencies are included.
fn create_tar_archive(exclude_patterns: &[String], include_dirs: &[String]) -> Result<String> {
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

    let original_dir = std::env::current_dir()?;

    // Pre-fetch dependencies to pull the private dependencies in the axiom_cargo_home (set it as CARGO_HOME) directory
    let cargo_workspace_root =
        find_cargo_workspace_root().context("Failed to find cargo workspace root")?;
    println!(
        "found cargo workspace root: {}",
        cargo_workspace_root.display()
    );
    std::env::set_current_dir(&cargo_workspace_root)?;
    let axiom_cargo_home = cargo_workspace_root.join(AXIOM_CARGO_HOME);
    std::fs::create_dir_all(&axiom_cargo_home)?;

    // Run cargo fetch with CARGO_HOME set to axiom_cargo_home
    // Fetch 1: target = x86 linux which is the cloud machine
    println!("Fetching dependencies to {}...", AXIOM_CARGO_HOME);
    let status = std::process::Command::new("cargo")
        .env("CARGO_HOME", &axiom_cargo_home)
        .arg("fetch")
        .arg("--target")
        .arg("x86_64-unknown-linux-gnu")
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        return Err(eyre::eyre!("Failed to fetch cargo dependencies"));
    }

    // Fetch 2: Use local target as Cargo might have some dependencies for the local machine that's different from the cloud machine
    // if local is not linux x86. And even though they are not needed in compilation, cargo tries to download them first.
    println!("Fetching dependencies to {}...", AXIOM_CARGO_HOME);
    let status = std::process::Command::new("cargo")
        .env("CARGO_HOME", &axiom_cargo_home)
        .arg("fetch")
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        return Err(eyre::eyre!("Failed to fetch cargo dependencies"));
    }

    // Fetch 3: Run cargo fetch for some host dependencies (std stuffs)
    let status = cargo_command("fetch", &[])
        .env("CARGO_HOME", &axiom_cargo_home)
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        return Err(eyre::eyre!("Failed to fetch cargo dependencies"));
    }

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

    let has_cargo_toml = tracked_files
        .iter()
        .any(|path| path.ends_with("Cargo.toml"));
    let has_cargo_lock = tracked_files
        .iter()
        .any(|path| path.ends_with("Cargo.lock"));

    if !has_cargo_toml || !has_cargo_lock {
        return Err(eyre::eyre!(
            "Cargo.toml and Cargo.lock are required and should be tracked by git"
        ));
    }

    // Walk through the directory and add files to the archive
    let walker = walkdir::WalkDir::new(".")
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            let path_str = path.to_string_lossy();
            // Check against user-provided exclusion patterns
            let matches_exclusion = exclude_patterns.iter().any(|s| path_str.contains(s));
            // Check if path is in user-provided include directories
            let in_include_dir = include_dirs.iter().any(|dir| {
                path_str.starts_with(&format!("./{}", dir)) || path_str.starts_with(dir)
            });
            // Check if file is tracked by git (directories are allowed to continue traversal)
            // Allow axiom_cargo_home directory even though it's not tracked by git
            let is_tracked = path.is_dir()
                || tracked_files.contains(path_str.trim_start_matches("./"))
                || path_str.contains(AXIOM_CARGO_HOME)
                || in_include_dir;

            is_tracked && !matches_exclusion
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
        }
    }

    builder.finish()?;
    // Clean up the axiom_cargo_home directory
    std::fs::remove_dir_all(axiom_cargo_home).ok();
    // Change back to the original directory
    std::env::set_current_dir(original_dir)?;

    Ok(tar_path.to_string())
}

struct ProgressReader<R> {
    inner: R,
    progress: Arc<Mutex<(u64, u64)>>, // (bytes_read, total_bytes)
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            let mut progress = self.progress.lock().unwrap();
            progress.0 += n as u64;
        }
        Ok(n)
    }
}

pub fn execute(args: BuildArgs) -> Result<()> {
    let config = load_config()?;

    // Check if we're in a Rust project
    if !is_rust_project() {
        return Err(eyre::eyre!(
            "Not in a Rust project. Make sure Cargo.toml exists."
        ));
    }

    // Check toolchain version using rustc_version crate
    let toolchain_version = rustc_version::version_meta()
        .context("Failed to get toolchain version")?
        .semver;

    if toolchain_version.major != 1 || toolchain_version.minor != 85 {
        return Err(eyre::eyre!(
            "Unsupported toolchain version, expected 1.85, found: {}, Use `rustup default 1.85` to install as your default.",
            toolchain_version.to_string()
        ));
    }

    // Get the config_id from args, return error if not provided
    let config_id = get_config_id(args.config_id, &config)?;

    // Get the git root directory
    let git_root = find_git_root().context("Failed to find git root directory")?;

    // Get the current directory, which should be the guest program directory
    let current_dir = std::env::current_dir()?;

    // Calculate the relative path from git root to current directory
    let program_path = current_dir
        .strip_prefix(&git_root)
        .context("Failed to determine relative path from git root")?
        .to_string_lossy()
        .to_string();

    if !program_path.is_empty() {
        println!("Using program path: {}", program_path);
    }

    let cargo_workspace_root =
        find_cargo_workspace_root().context("Failed to find cargo workspace root")?;
    // Calculate the relative path from git root to cargo workspace root
    let cargo_root_path = cargo_workspace_root
        .strip_prefix(&git_root)
        .context("Failed to determine relative path from git root to cargo workspace root")?
        .to_string_lossy()
        .to_string();

    if !cargo_root_path.is_empty() {
        println!("Using cargo workspace root: {}", cargo_root_path);
    }

    // Check for bin flag
    let metadata = cargo_metadata::MetadataCommand::new().exec()?;
    let current_dir = std::env::current_dir()?;
    let mut pkgs_in_current_dir: Vec<_> = metadata
        .workspace_packages()
        .into_iter()
        .filter(|p| current_dir.starts_with(p.manifest_path.parent().unwrap()))
        .collect();

    let packages_to_consider = if pkgs_in_current_dir.is_empty() {
        if current_dir.as_path() == metadata.workspace_root.as_std_path() {
            metadata.workspace_packages()
        } else {
            return Err(eyre::eyre!("Could not determine which Cargo package to build. Please run this command from a package directory or the workspace root."));
        }
    } else {
        pkgs_in_current_dir.sort_by_key(|p| p.manifest_path.as_str().len());
        vec![pkgs_in_current_dir.pop().unwrap()]
    };

    let binaries: Vec<_> = packages_to_consider
        .iter()
        .flat_map(|p| p.targets.iter().filter(|t| t.is_bin()))
        .collect();

    let bin_to_build = if binaries.len() > 1 {
        if let Some(bin_name) = &args.bin {
            if !binaries.iter().any(|b| &b.name == bin_name) {
                return Err(eyre::eyre!(
                    "Binary '{}' not found. Available binaries: {}",
                    bin_name,
                    binaries
                        .iter()
                        .map(|b| b.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Some(bin_name.clone())
        } else {
            return Err(eyre::eyre!(
                "Multiple binaries found. Please specify which one to build with the --bin flag. Available binaries: {}",
                binaries
                    .iter()
                    .map(|b| b.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    } else if let Some(bin) = binaries.first() {
        args.bin
            .as_ref()
            .map_or(Ok(Some(bin.name.clone())), |user_bin| {
                if &bin.name != user_bin {
                    Err(eyre::eyre!(
                        "Binary '{}' not found. Available binary: {}",
                        user_bin,
                        bin.name
                    ))
                } else {
                    Ok(Some(user_bin.clone()))
                }
            })?
    } else {
        None
    };
    println!("bin_to_build: {:?}", bin_to_build);

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

    // Parse include directories
    let include_dirs = args
        .include_dirs
        .map(|dirs| {
            dirs.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    // Create tar archive of the current directory
    println!("Creating archive of the project...");
    let tar_path = create_tar_archive(&exclude_patterns, &include_dirs)?;

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
    let program_path_query = if program_path.is_empty() {
        ".".to_string()
    } else {
        program_path
    };
    let cargo_root_query = if cargo_root_path.is_empty() {
        ".".to_string()
    } else {
        cargo_root_path
    };
    let mut url = format!(
        "{}/programs?config_id={}&program_path={}&cargo_root_path={}",
        config.api_url, config_id, program_path_query, cargo_root_query
    );
    if let Some(bin) = bin_to_build {
        url.push_str(&format!("&bin_name={}", bin));
    }

    println!("Sending build request for config ID: {}", config_id);

    // Make the POST request with multipart form data
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(300)) // 5 minute timeout
        .build()?;
    let api_key = get_api_key()?;

    // Create a progress tracker
    let progress = Arc::new(Mutex::new((0, metadata.len())));
    let progress_clone = Arc::clone(&progress);

    // Spawn a thread to display progress
    let progress_handle = std::thread::spawn(move || {
        let start_time = Instant::now();
        let mut last_percent = 0;

        loop {
            std::thread::sleep(Duration::from_millis(100));
            let (current, total) = *progress_clone.lock().unwrap();

            if total == 0 {
                break;
            }

            let percent = ((current as f64 / total as f64) * 100.0) as u8;

            // Only update when the percentage changes
            if percent != last_percent {
                // Calculate speed
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    current as f64 / elapsed / 1024.0
                } else {
                    0.0
                };

                print!("\rUploading: {}% ({:.2} KB/s)", percent, speed);
                io::stdout().flush().unwrap();
                last_percent = percent;
            }

            if current >= total {
                println!("\rUpload complete!                ");
                break;
            }
        }
    });

    // Open the file with progress tracking
    let file = File::open(&tar_path).context("Failed to open tar file")?;
    let progress_reader = ProgressReader {
        inner: file,
        progress: Arc::clone(&progress),
    };

    // Create the form with the progress-tracking reader
    let part = reqwest::blocking::multipart::Part::reader(progress_reader)
        .file_name("program.tar.gz")
        .mime_str("application/gzip")?;

    let form = reqwest::blocking::multipart::Form::new().part("program", part);

    let response = client
        .post(url)
        .header(API_KEY_HEADER, api_key)
        .multipart(form)
        .send()?;

    // Wait for the progress thread to finish
    progress_handle.join().unwrap();

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
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        Err(eyre::eyre!("Client error ({}): {}", status, error_text))
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
    let config = load_config()?;
    let api_key = get_api_key()?;
    let url = format!("{}/programs/{}/logs", config.api_url, program_id);
    let response = Client::new()
        .get(url)
        .header(API_KEY_HEADER, api_key)
        .send()?;
    // Check if the request was successful
    if response.status().is_success() {
        // Create output filename based on program ID
        let filename = format!("program_{}_logs.txt", program_id);

        // Write the response body to a file
        let mut file =
            File::create(&filename).context(format!("Failed to create log file: {}", filename))?;

        let content = response.bytes().context("Failed to read response body")?;

        std::io::copy(&mut content.as_ref(), &mut file).context("Failed to write logs to file")?;

        println!("Logs downloaded successfully to: {}", filename);
    } else if response.status().is_client_error() {
        let status = response.status();
        let error_text = response.text()?;
        return Err(eyre::eyre!("Client error ({}): {}", status, error_text));
    } else {
        return Err(eyre::eyre!(
            "Logs download request failed with status: {}",
            response.status()
        ));
    }
    Ok(())
}
