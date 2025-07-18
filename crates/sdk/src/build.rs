use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use eyre::{Context, OptionExt, Result};
use flate2::{write::GzEncoder, Compression};
use openvm_build::cargo_command;
use reqwest::blocking::Client;
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::Builder;

use crate::{AxiomSdk, API_KEY_HEADER};

pub const MAX_PROGRAM_SIZE_MB: u64 = 1024;

pub const AXIOM_CARGO_HOME: &str = "axiom_cargo_home";

pub trait BuildSdk {
    fn list_programs(&self) -> Result<Vec<BuildStatus>>;
    fn get_build_status(&self, program_id: &str) -> Result<BuildStatus>;
    fn download_program(&self, program_id: &str, program_type: &str) -> Result<()>;
    fn download_build_logs(&self, program_id: &str) -> Result<()>;
    fn register_new_program(
        &self,
        program_dir: impl AsRef<Path>,
        args: BuildArgs,
    ) -> Result<String>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildStatus {
    pub id: String,
    pub created_at: String,
    pub status: String,
    pub config_uuid: String,
    pub error_message: Option<String>,
    pub name: String,
    pub created_by: String,
    pub last_active_at: String,
    pub launched_at: Option<String>,
    pub terminated_at: Option<String>,
    pub program_hash: String,
    pub openvm_config: String,
    pub cells_used: u64,
    pub proofs_run: u64,
    pub is_favorite: bool,
}

#[derive(Debug)]
pub struct BuildArgs {
    /// The configuration ID to use for the build
    pub config_source: Option<ConfigSource>,
    /// The binary to build, if there are multiple binaries in the project
    pub bin: Option<String>,
    /// Keep the tar archive after uploading
    pub keep_tarball: Option<bool>,
    /// Comma-separated list of file patterns to exclude (e.g. "*.log,temp/*")
    pub exclude_files: Option<String>,
    /// Comma-separated list of directories to include even if not tracked by git
    pub include_dirs: Option<String>,
    /// Internal Only: Force creating the config and triggering keygen.
    pub force_keygen: bool,
}

#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// The configuration ID to use for the build
    ConfigId(String),
    /// Path to a local configuration file
    ConfigPath(String),
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

struct TarFile {
    path: String,
    keep: bool,
}

impl Drop for TarFile {
    fn drop(&mut self) {
        if !self.keep {
            std::fs::remove_file(&self.path).unwrap();
        }
    }
}

impl BuildSdk for AxiomSdk {
    fn list_programs(&self) -> Result<Vec<BuildStatus>> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;
        let url = format!("{}/programs", self.config.api_url);

        let response = Client::new()
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()?;

        let body: Value = response.json()?;

        // Extract the items array from the response
        if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
            if items.is_empty() {
                println!("No programs found");
                return Ok(vec![]);
            }

            let mut programs = vec![];

            for item in items {
                let build_status = serde_json::from_value(item.clone())?;
                programs.push(build_status);
            }

            Ok(programs)
        } else {
            Err(eyre::eyre!("Unexpected response format: {}", body))
        }
    }

    fn get_build_status(&self, program_id: &str) -> Result<BuildStatus> {
        let url = format!("{}/programs/{}", self.config.api_url, program_id);

        println!("Checking build status for program ID: {program_id}");

        // Make the GET request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = client
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to send status request")?;

        // Check if the request was successful
        if response.status().is_success() {
            let body: Value = response.json()?;
            let build_status = serde_json::from_value(body)?;
            Ok(build_status)
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

    fn download_program(&self, program_id: &str, program_type: &str) -> Result<()> {
        let url = format!(
            "{}/programs/{}/download/{}",
            self.config.api_url, program_id, program_type
        );

        println!("Downloading {program_type} for program ID: {program_id}");

        // Make the GET request
        let client = Client::new();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

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
                program_type.to_string()
            };
            let filename = format!("program_{program_id}.{ext}");

            // Write the response body to a file
            let mut file = File::create(&filename)
                .context(format!("Failed to create output file: {filename}"))?;

            let content = response.bytes().context("Failed to read response body")?;

            std::io::copy(&mut content.as_ref(), &mut file)
                .context("Failed to write artifact to file")?;

            println!("Artifact downloaded successfully to: {filename}");
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

    fn download_build_logs(&self, program_id: &str) -> Result<()> {
        let url = format!("{}/programs/{}/logs", self.config.api_url, program_id);

        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

        let response = Client::new()
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()?;

        // Check if the request was successful
        if response.status().is_success() {
            // Create output filename based on program ID
            let filename = format!("program_{program_id}_logs.txt");

            // Write the response body to a file
            let mut file = File::create(&filename)
                .context(format!("Failed to create log file: {filename}"))?;

            let content = response.bytes().context("Failed to read response body")?;

            std::io::copy(&mut content.as_ref(), &mut file)
                .context("Failed to write logs to file")?;

            println!("Logs downloaded successfully to: {filename}");
        } else if response.status().is_client_error() {
            let status = response.status();
            let error_text = response.text()?;
            eyre::bail!("Client error ({}): {}", status, error_text);
        } else {
            eyre::bail!(
                "Logs download request failed with status: {}",
                response.status()
            );
        }

        Ok(())
    }

    fn register_new_program(
        &self,
        program_dir: impl AsRef<Path>,
        args: BuildArgs,
    ) -> Result<String> {
        // Check if we're in a Rust project
        if !is_rust_project(program_dir.as_ref()) {
            eyre::bail!("Not in a Rust project. Make sure Cargo.toml exists.");
        }

        // Check toolchain version using rustc_version crate
        let toolchain_version = rustc_version::version_meta()
            .context("Failed to get toolchain version")?
            .semver;

        if toolchain_version.major != 1 || toolchain_version.minor != 85 {
            eyre::bail!(
                "Unsupported toolchain version, expected 1.85, found: {}, Use `rustup default 1.85` to install as your default.",
                toolchain_version.to_string()
            );
        }

        // Use config id if it was provided
        let config_id = match &args.config_source {
            // If config id was provided, use it
            Some(ConfigSource::ConfigId(id)) => Some(id.to_string()),
            // If config path was provided, do nothing
            Some(ConfigSource::ConfigPath(_)) => None,
            // If no config source was provided, use the config id from the
            // config file (which could be None)
            None => self.config.config_id.clone(),
        };

        // Get the git root directory
        let git_root =
            find_git_root(program_dir.as_ref()).context("Failed to find git root directory")?;

        // Get the current directory, which should be the guest program directory
        let current_dir = program_dir.as_ref().to_path_buf();

        // Calculate the relative path from git root to current directory
        let program_path = current_dir
            .strip_prefix(&git_root)
            .context("Failed to determine relative path from git root")?
            .to_string_lossy()
            .to_string();

        if !program_path.is_empty() {
            println!("Using program path: {program_path}");
        }

        let cargo_workspace_root = find_cargo_workspace_root(program_dir.as_ref())
            .context("Failed to find cargo workspace root")?;
        // Calculate the relative path from git root to cargo workspace root
        let cargo_root_path = cargo_workspace_root
            .strip_prefix(&git_root)
            .context("Failed to determine relative path from git root to cargo workspace root")?
            .to_string_lossy()
            .to_string();

        if !cargo_root_path.is_empty() {
            println!("Using cargo workspace root: {cargo_root_path}");
        }

        // Check for bin flag
        let current_dir = program_dir.as_ref().to_path_buf();
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(current_dir.clone())
            .exec()?;
        let mut pkgs_in_current_dir: Vec<_> = metadata
            .workspace_packages()
            .into_iter()
            .filter(|p| current_dir.starts_with(p.manifest_path.parent().unwrap()))
            .collect();

        let packages_to_consider = if pkgs_in_current_dir.is_empty() {
            if current_dir.as_path() == metadata.workspace_root.as_std_path() {
                metadata.workspace_packages()
            } else {
                eyre::bail!("Could not determine which Cargo package to build. Please run this command from a package directory or the workspace root.");
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
                    eyre::bail!(
                        "Binary '{}' not found. Available binaries: {}",
                        bin_name,
                        binaries
                            .iter()
                            .map(|b| b.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                Some(bin_name.clone())
            } else {
                eyre::bail!(
                    "Multiple binaries found. Please specify which one to build with the --bin flag. Available binaries: {}",
                    binaries
                        .iter()
                        .map(|b| b.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
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
        println!("bin_to_build: {bin_to_build:?}");

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
        let tar_file = create_tar_archive(
            program_dir.as_ref(),
            args.keep_tarball.unwrap_or(false),
            &exclude_patterns,
            &include_dirs,
        )?;
        let tar_path = &tar_file.path;

        // Check if the tar file size exceeds 10MB
        let metadata = std::fs::metadata(tar_path).context("Failed to get tar file metadata")?;
        if metadata.len() > MAX_PROGRAM_SIZE_MB * 1024 * 1024 {
            std::fs::remove_file(tar_path).ok();
            eyre::bail!(
                "Project archive size ({}) exceeds maximum allowed size of {}MB",
                metadata.len(),
                MAX_PROGRAM_SIZE_MB
            );
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
            "{}/programs?program_path={}&cargo_root_path={}",
            self.config.api_url, program_path_query, cargo_root_query
        );
        if let Some(id) = &config_id {
            url.push_str(&format!("&config_id={id}"));
        }
        if let Some(bin) = bin_to_build {
            url.push_str(&format!("&bin_name={bin}"));
        }
        if args.force_keygen {
            url.push_str("&force_keygen=true");
        }
        if let Ok(sha) = get_git_commit_sha(&git_root) {
            url.push_str(&format!("&commit_sha={sha}"));
        }

        if let Some(id) = &config_id {
            println!("Sending build request for config ID: {id}");
        } else if let Some(ConfigSource::ConfigPath(path)) = args.config_source.clone() {
            println!("Sending build request with config file: {path}");
        } else {
            println!("Sending build request with default config");
        }

        // Make the POST request with multipart form data
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300)) // 5 minute timeout
            .build()?;
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or(eyre::eyre!("API key not set"))?;

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

                    print!("\rUploading: {percent}% ({speed:.2} KB/s)");
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
        let file = File::open(tar_path).context("Failed to open tar file")?;
        let progress_reader = ProgressReader {
            inner: file,
            progress: Arc::clone(&progress),
        };

        // Create the form with the progress-tracking reader
        let part = reqwest::blocking::multipart::Part::reader(progress_reader)
            .file_name("program.tar.gz")
            .mime_str("application/gzip")?;

        let mut form = reqwest::blocking::multipart::Form::new().part("program", part);

        if let Some(ConfigSource::ConfigPath(config_path_str)) = args.config_source {
            let config_path = Path::new(&config_path_str);
            let config_file_content = std::fs::read(config_path).with_context(|| {
                format!("Failed to read config file at: {}", config_path.display())
            })?;
            let file_name = config_path
                .file_name()
                .ok_or_eyre("Invalid config file path")?
                .to_string_lossy()
                .to_string();
            let config_part = reqwest::blocking::multipart::Part::bytes(config_file_content)
                .file_name(file_name)
                .mime_str("application/octet-stream")?;
            form = form.part("config", config_part);
        }

        let response = client
            .post(url)
            .header(API_KEY_HEADER, api_key)
            .multipart(form)
            .send()?;

        // Wait for the progress thread to finish
        progress_handle.join().unwrap();

        // Check if the request was successful
        if response.status().is_success() {
            let body = response.json::<serde_json::Value>().unwrap();
            let program_id = body["id"].as_str().unwrap();
            println!("Build request sent successfully: {program_id}");
            Ok(program_id.to_string())
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
}

fn is_rust_project(program_dir: impl AsRef<Path>) -> bool {
    program_dir.as_ref().join("Cargo.toml").exists()
}

fn find_git_root(program_dir: impl AsRef<Path>) -> Result<std::path::PathBuf> {
    // Start from the current directory
    let mut current_dir = program_dir.as_ref().to_path_buf();

    loop {
        // Check if .git directory exists in the current directory
        let git_dir = current_dir.join(".git");
        if git_dir.exists() && git_dir.is_dir() {
            return Ok(current_dir);
        }

        // Move up to parent directory
        if !current_dir.pop() {
            // We've reached the root of the filesystem without finding a .git directory
            eyre::bail!("Not in a git repository");
        }
    }
}

fn find_cargo_workspace_root(program_dir: impl AsRef<Path>) -> Result<std::path::PathBuf> {
    // Start from the current directory
    let mut current_dir = program_dir.as_ref().to_path_buf();
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

fn get_git_commit_sha(git_root: impl AsRef<Path>) -> Result<String> {
    let git_dir = git_root.as_ref().join(".git");

    // Read .git/HEAD to get the current reference
    let head_file = git_dir.join("HEAD");
    let head_content = std::fs::read_to_string(&head_file).context("Failed to read .git/HEAD")?;

    let head_content = head_content.trim();

    // Check if HEAD contains a direct SHA or a reference
    if head_content.starts_with("ref: ") {
        // It's a reference, read the referenced file
        let ref_path = head_content.strip_prefix("ref: ").unwrap();
        let ref_file = git_dir.join(ref_path);

        let commit_sha = std::fs::read_to_string(&ref_file)
            .context(format!("Failed to read git reference file: {ref_path}"))?
            .trim()
            .to_string();

        if commit_sha.is_empty() {
            eyre::bail!("Got empty commit SHA from git reference");
        }

        Ok(commit_sha)
    } else if head_content.len() == 40 && head_content.chars().all(|c| c.is_ascii_hexdigit()) {
        // It's a direct SHA (40 hex characters)
        Ok(head_content.to_string())
    } else {
        Err(eyre::eyre!(
            "Unexpected format in .git/HEAD: {}",
            head_content
        ))
    }
}

// The tarball contains everything in the git root of the guest program that's tracked by git.
// Additionally, it does `cargo fetch` to pre-fetch dependencies so private dependencies are included.
fn create_tar_archive(
    program_dir: impl AsRef<Path>,
    keep_tarball: bool,
    exclude_patterns: &[String],
    include_dirs: &[String],
) -> Result<TarFile> {
    let tar_path = program_dir.as_ref().join("program.tar.gz");
    let tar_file = File::create(&tar_path)?;
    let tar = TarFile {
        path: tar_path.to_string_lossy().to_string(),
        keep: keep_tarball,
    };
    let enc = GzEncoder::new(tar_file, Compression::default());
    let mut builder = Builder::new(enc);

    // Find the git root directory
    let git_root =
        find_git_root(program_dir.as_ref()).context("Failed to find git root directory")?;
    // Get the git root directory name
    let dir_name = git_root
        .file_name()
        .ok_or_eyre("Failed to get git root directory name")?
        .to_string_lossy()
        .to_string();

    let original_dir = std::env::current_dir()?;

    // Pre-fetch dependencies to pull the private dependencies in the axiom_cargo_home (set it as CARGO_HOME) directory
    let cargo_workspace_root = find_cargo_workspace_root(program_dir.as_ref())
        .context("Failed to find cargo workspace root")?;
    println!(
        "found cargo workspace root: {}",
        cargo_workspace_root.display()
    );
    std::env::set_current_dir(&cargo_workspace_root)?;
    let axiom_cargo_home = cargo_workspace_root.join(AXIOM_CARGO_HOME);
    std::fs::create_dir_all(&axiom_cargo_home)?;

    // Clean up the axiom_cargo_home directory when the function exits
    defer! {
        std::fs::remove_dir_all(&axiom_cargo_home).ok();
    }

    // Run cargo fetch with CARGO_HOME set to axiom_cargo_home
    // Fetch 1: target = x86 linux which is the cloud machine
    println!("Fetching dependencies to {AXIOM_CARGO_HOME}...");
    let status = std::process::Command::new("cargo")
        .env("CARGO_HOME", &axiom_cargo_home)
        .arg("fetch")
        .arg("--target")
        .arg("x86_64-unknown-linux-gnu")
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        eyre::bail!("Failed to fetch cargo dependencies");
    }

    // Fetch 2: Use local target as Cargo might have some dependencies for the local machine that's different from the cloud machine
    // if local is not linux x86. And even though they are not needed in compilation, cargo tries to download them first.
    println!("Fetching dependencies to {AXIOM_CARGO_HOME}...");
    let status = std::process::Command::new("cargo")
        .env("CARGO_HOME", &axiom_cargo_home)
        .arg("fetch")
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        eyre::bail!("Failed to fetch cargo dependencies");
    }

    // Fetch 3: Run cargo fetch for some host dependencies (std stuffs)
    let status = cargo_command("fetch", &[])
        .env("CARGO_HOME", &axiom_cargo_home)
        .status()
        .context("Failed to run 'cargo fetch'")?;
    if !status.success() {
        eyre::bail!("Failed to fetch cargo dependencies");
    }

    std::env::set_current_dir(&git_root)?;
    // Get list of files tracked by git
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .output()
        .context("Failed to run 'git ls-files'")?;

    if !output.status.success() {
        eyre::bail!("Failed to get git tracked files");
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
        eyre::bail!("Cargo.toml and Cargo.lock are required and should be tracked by git");
    }

    // Walk through the directory and add files to the archive
    let walker = walkdir::WalkDir::new(".")
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            let path_str = path.to_string_lossy();

            // Exclude the tar file itself to avoid adding it to the tarball
            if path_str.ends_with("program.tar.gz") {
                return false;
            }

            // Check against user-provided exclusion patterns
            let matches_exclusion = exclude_patterns.iter().any(|s| path_str.contains(s));
            // Check if path is in user-provided include directories
            let in_include_dir = include_dirs
                .iter()
                .any(|dir| path_str.starts_with(&format!("./{dir}")) || path_str.starts_with(dir));
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
    // Change back to the original directory
    std::env::set_current_dir(original_dir)?;

    Ok(tar)
}
