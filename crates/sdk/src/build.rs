use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::{Arc, Mutex},
};

use eyre::{Context, OptionExt, Result};
use flate2::{Compression, write::GzEncoder};
use openvm_build::cargo_command;
use reqwest::blocking::Client;
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::Builder;

use crate::{API_KEY_HEADER, AxiomSdk, authenticated_get, download_file, send_request_json};

pub const MAX_PROGRAM_SIZE_MB: u64 = 1024;
const BUILD_POLLING_INTERVAL_SECS: u64 = 10;

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
    fn wait_for_build_completion(&self, program_id: &str) -> Result<()>;
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
    /// The configuration source to use for the build
    pub config_source: Option<ConfigSource>,
    /// The binary to build, if there are multiple binaries in the project
    pub bin: Option<String>,
    /// Keep the tar archive after uploading
    pub keep_tarball: Option<bool>,
    /// Comma-separated list of file patterns to exclude (e.g. "*.log,temp/*")
    pub exclude_files: Option<String>,
    /// Comma-separated list of directories to include even if not tracked by git
    pub include_dirs: Option<String>,
    /// The project ID to associate with the build
    pub project_id: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// The configuration ID to use for the build
    ConfigId(String),
    /// Path to an OpenVM TOML configuration file
    ConfigPath(String),
}
struct ProgressReader<R> {
    inner: R,
    progress: Arc<Mutex<indicatif::ProgressBar>>,
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            let pb = self.progress.lock().unwrap();
            pb.inc(n as u64);
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
        let url = format!("{}/programs", self.config.api_url);

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to list programs")?;

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

        let request = authenticated_get(&self.config, &url)?;
        let body: Value = send_request_json(request, "Failed to get build status")?;
        let build_status = serde_json::from_value(body)?;
        Ok(build_status)
    }

    fn download_program(&self, program_id: &str, program_type: &str) -> Result<()> {
        let url = format!(
            "{}/programs/{}/download/{}",
            self.config.api_url, program_id, program_type
        );

        // Make the GET request
        let client = Client::new();
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = client
            .get(url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .context("Failed to download artifact")?;

        // Check if the request was successful
        if response.status().is_success() {
            // Create organized directory structure
            let build_dir = format!("axiom-artifacts/program-{}/artifacts", program_id);
            std::fs::create_dir_all(&build_dir)
                .context(format!("Failed to create build directory: {}", build_dir))?;

            // Create output filename based on artifact type
            let ext = if program_type == "source" {
                "tar.gz".to_string()
            } else {
                program_type.to_string()
            };
            let filename = format!("{}/{}.{}", build_dir, program_type, ext);

            // Write the response body to a file
            let mut file = File::create(&filename)
                .context(format!("Failed to create output file: {filename}"))?;

            let content = response.bytes().context("Failed to read response body")?;

            std::io::copy(&mut content.as_ref(), &mut file)
                .context("Failed to write artifact to file")?;

            println!("  ✓ {}", filename);
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

        // Create organized directory structure
        let build_dir = format!("axiom-artifacts/program-{}/artifacts", program_id);
        std::fs::create_dir_all(&build_dir)
            .context(format!("Failed to create build directory: {}", build_dir))?;

        // Create output filename in the build directory
        let filename = std::path::PathBuf::from(format!("{}/logs.txt", build_dir));
        let request = authenticated_get(&self.config, &url)?;
        download_file(request, &filename, "Failed to download build logs")?;
        println!("  ✓ {}", filename.display());
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
            // If config path was provided, do nothing (we'll upload the file separately)
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

        let cargo_workspace_root = find_cargo_workspace_root(program_dir.as_ref())
            .context("Failed to find cargo workspace root")?;
        // Calculate the relative path from git root to cargo workspace root
        let cargo_root_path = cargo_workspace_root
            .strip_prefix(&git_root)
            .context("Failed to determine relative path from git root to cargo workspace root")?
            .to_string_lossy()
            .to_string();

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
                eyre::bail!(
                    "Could not determine which Cargo package to build. Please run this command from a package directory or the workspace root."
                );
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
        println!();
        Formatter::print_info("Creating project archive...");
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
        if let Some(project_id) = args.project_id {
            url.push_str(&format!("&project_id={project_id}"));
        }
        if let Some(bin) = bin_to_build {
            url.push_str(&format!("&bin_name={bin}"));
        }
        if let Ok(sha) = get_git_commit_sha(&git_root) {
            url.push_str(&format!("&commit_sha={sha}"));
        }

        use crate::formatting::Formatter;
        Formatter::print_header("Building Program");

        if let Some(id) = &config_id {
            Formatter::print_field("Config ID", id);
        } else if let Some(ConfigSource::ConfigPath(path)) = args.config_source.clone() {
            Formatter::print_field("Config File", &path);
        } else {
            Formatter::print_field("Config", "Default");
        }

        // Make the POST request with multipart form data
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300)) // 5 minute timeout
            .build()?;
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        // Create progress bar for upload
        let pb = Formatter::create_upload_progress(metadata.len());
        let progress = Arc::new(Mutex::new(pb));

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

        // Add config file if provided
        if let Some(ConfigSource::ConfigPath(config_path_str)) = args.config_source {
            let config_path = Path::new(&config_path_str);
            let config_file_content = std::fs::read(config_path).with_context(|| {
                format!(
                    "Failed to read OpenVM config file at: {}",
                    config_path.display()
                )
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

        // Finish the progress bar
        progress
            .lock()
            .unwrap()
            .finish_with_message("✓ Upload complete!");

        // Check if the request was successful
        if response.status().is_success() {
            let body = response.json::<serde_json::Value>().unwrap();
            let program_id = body["id"].as_str().unwrap();
            Formatter::print_success(&format!("Build initiated ({})", program_id));
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

    fn wait_for_build_completion(&self, program_id: &str) -> Result<()> {
        use crate::config::ConfigSdk;
        use crate::formatting::{Formatter, calculate_duration};
        use std::time::Duration;

        println!();
        let spinner = Formatter::create_spinner("Checking build status...");

        loop {
            // Get status without printing repetitive messages
            let url = format!("{}/programs/{}", self.config.api_url, program_id);
            let api_key = self
                .config
                .api_key
                .as_ref()
                .ok_or(eyre::eyre!("API key not set"))?;

            let response = Client::new()
                .get(url)
                .header(API_KEY_HEADER, api_key)
                .send()
                .context("Failed to send status request")?;

            let build_status: BuildStatus = if response.status().is_success() {
                let body: Value = response.json()?;
                serde_json::from_value(body)?
            } else {
                return Err(eyre::eyre!(
                    "Failed to get build status: {}",
                    response.status()
                ));
            };

            match build_status.status.as_str() {
                "ready" => {
                    spinner.finish_with_message("✓ Build completed successfully!");

                    // Get OpenVM version from config
                    let config_metadata =
                        self.get_vm_config_metadata(Some(&build_status.config_uuid))?;

                    // Print completion information
                    Formatter::print_section("Build Summary");
                    Formatter::print_field("Program ID", &build_status.id);
                    Formatter::print_field("Program Hash", &build_status.program_hash);
                    Formatter::print_field("Config ID", &build_status.config_uuid);
                    Formatter::print_field("OpenVM Version", &config_metadata.openvm_version);

                    if let Some(launched_at) = &build_status.launched_at {
                        if let Some(terminated_at) = &build_status.terminated_at {
                            Formatter::print_section("Build Stats");
                            Formatter::print_field("Created", &build_status.created_at);
                            Formatter::print_field("Initiated", launched_at);
                            Formatter::print_field("Finished", terminated_at);

                            if let Ok(duration) = calculate_duration(launched_at, terminated_at) {
                                Formatter::print_field("Duration", &duration);
                            }
                        }
                    }

                    // Download artifacts automatically
                    Formatter::print_section("Downloading Artifacts");

                    // Download ELF
                    Formatter::print_info("Downloading ELF...");
                    if let Err(e) = self.download_program(&build_status.id, "elf") {
                        println!("Warning: Failed to download ELF: {}", e);
                    }

                    // Download EXE
                    Formatter::print_info("Downloading EXE...");
                    if let Err(e) = self.download_program(&build_status.id, "exe") {
                        println!("Warning: Failed to download EXE: {}", e);
                    }

                    // Download logs
                    Formatter::print_info("Downloading logs...");
                    if let Err(e) = self.download_build_logs(&build_status.id) {
                        println!("Warning: Failed to download logs: {}", e);
                    }

                    return Ok(());
                }
                "error" | "failed" => {
                    let error_msg = build_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    spinner.finish_with_message(format!("✗ Build failed: {}", error_msg));
                    eyre::bail!("Build failed: {}", error_msg);
                }
                "processing" => {
                    spinner.set_message("Build in progress...");
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
                "not_ready" => {
                    spinner.set_message("Build queued...");
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
                _ => {
                    spinner.set_message(format!("Build status: {}...", build_status.status));
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
            }
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

    std::env::set_current_dir(&cargo_workspace_root)?;
    let axiom_cargo_home = cargo_workspace_root.join(AXIOM_CARGO_HOME);
    std::fs::create_dir_all(&axiom_cargo_home)?;

    // Clean up the axiom_cargo_home directory when the function exits
    defer! {
        std::fs::remove_dir_all(&axiom_cargo_home).ok();
    }

    // Run cargo fetch with CARGO_HOME set to axiom_cargo_home
    // Fetch 1: target = x86 linux which is the cloud machine
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
