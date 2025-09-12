use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use eyre::{Context, OptionExt, Result, eyre};
use flate2::{Compression, write::GzEncoder};
use openvm_build::cargo_command;
use reqwest::blocking::Client;
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::Builder;

use crate::{
    API_KEY_HEADER, AxiomSdk, ProgressCallback, add_cli_version_header, authenticated_get,
    download_file, send_request_json,
};

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
    pub project_id: String,
    pub project_name: String,
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
    pub project_id: Option<String>,
    /// The project name if it's creating a new project
    pub project_name: Option<String>,
    /// Allow building with uncommitted changes
    pub allow_dirty: bool,
}

#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// The configuration ID to use for the build
    ConfigId(String),
    /// Path to an OpenVM TOML configuration file
    ConfigPath(String),
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

struct CountingReader<R: Read> {
    inner: R,
    progress: Arc<AtomicU64>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes_read = self.inner.read(buf)?;
        if bytes_read > 0 {
            self.progress
                .fetch_add(bytes_read as u64, Ordering::Relaxed);
        }
        Ok(bytes_read)
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
                self.callback.on_info("No programs found");
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

        // Make the GET request with longer timeout for large files
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(600)) // 10 minute timeout for large downloads
            .build()?;
        let api_key = self.config.api_key.as_ref().ok_or_eyre("API key not set")?;

        let response = add_cli_version_header(client.get(url).header(API_KEY_HEADER, api_key))
            .send()
            .context("Failed to download artifact")?;

        let status = response.status();

        if status.is_success() {
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
            let filename = format!("{}/program.{}", build_dir, ext);

            // Write the response body to a file using streaming
            let mut file = File::create(&filename)
                .context(format!("Failed to create output file: {filename}"))?;

            let content_length = response.content_length();
            let mut response = response;

            if let Some(total) = content_length {
                self.callback
                    .on_progress_start(&format!("Downloading {}", program_type), Some(total));
            } else {
                self.callback
                    .on_progress_start(&format!("Downloading {}", program_type), None);
            }

            if content_length.is_some() {
                let mut buffer = vec![0u8; 1024 * 1024];
                let mut downloaded = 0u64;

                loop {
                    let bytes_read = response.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    file.write_all(&buffer[..bytes_read])?;
                    downloaded += bytes_read as u64;
                    self.callback.on_progress_update(downloaded);
                }
            } else {
                std::io::copy(&mut response, &mut file)?;
            }

            self.callback.on_progress_finish("✓ Download complete");
            self.callback.on_success(&filename.to_string());
            Ok(())
        } else if status.is_client_error() {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            self.callback.on_progress_finish("");
            self.callback
                .on_error(&format!("Client error response: {}", error_text));
            Err(eyre::eyre!("Client error ({}): {}", status, error_text))
        } else {
            self.callback.on_progress_finish("");
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            self.callback
                .on_error(&format!("Server error response: {}", error_text));
            Err(eyre::eyre!(
                "Download request failed with status: {} - {}",
                status,
                error_text
            ))
        }
    }

    fn download_build_logs(&self, program_id: &str) -> Result<()> {
        let url = format!("{}/programs/{}/logs", self.config.api_url, program_id);
        let build_dir = format!("axiom-artifacts/program-{}/artifacts", program_id);
        std::fs::create_dir_all(&build_dir)
            .context(format!("Failed to create build directory: {}", build_dir))?;

        let filename = std::path::PathBuf::from(format!("{}/logs.txt", build_dir));
        let response = authenticated_get(&self.config, &url)?;
        download_file(response, &filename, "Failed to download build logs")?;
        self.callback
            .on_success(&format!("✓ {}", filename.display()));
        Ok(())
    }

    fn register_new_program(
        &self,
        program_dir: impl AsRef<Path>,
        args: BuildArgs,
    ) -> Result<String> {
        self.register_new_program_base(program_dir, args, &*self.callback)
    }

    fn wait_for_build_completion(&self, program_id: &str) -> Result<()> {
        self.wait_for_build_completion_base(program_id, &*self.callback)
    }
}

impl AxiomSdk {
    pub fn wait_for_build_completion_base(
        &self,
        program_id: &str,
        callback: &dyn ProgressCallback,
    ) -> Result<()> {
        use std::time::Duration;

        callback.on_progress_start("Checking build status...", None);

        loop {
            let response = authenticated_get(
                &self.config,
                &format!("{}/programs/{}", self.config.api_url, program_id),
            )?;
            let build_status: BuildStatus =
                send_request_json(response, "Failed to get build status")?;

            match build_status.status.as_str() {
                "ready" => {
                    callback.on_progress_finish("✓ Build completed successfully!");

                    // Add spacing before sections
                    println!();

                    // Match the detailed status format
                    callback.on_section("Build Status");
                    callback.on_field("ID", &build_status.id);
                    callback.on_field("Name", &build_status.name);
                    callback.on_field("Project ID", &build_status.project_id);
                    callback.on_field("Project Name", &build_status.project_name);
                    callback.on_field("Status", &build_status.status);
                    callback.on_field("Program Hash", &build_status.program_hash);
                    callback.on_field("Config ID", &build_status.config_uuid);
                    callback.on_field("Created By", &build_status.created_by);
                    callback.on_field("Created At", &build_status.created_at);
                    callback.on_field("Last Active", &build_status.last_active_at);

                    if let Some(launched_at) = &build_status.launched_at {
                        callback.on_field("Launched At", launched_at);
                    }

                    if let Some(terminated_at) = &build_status.terminated_at {
                        callback.on_field("Terminated At", terminated_at);
                    }

                    if let Some(error_message) = &build_status.error_message {
                        callback.on_field("Error", error_message);
                    }

                    callback.on_section("Statistics");
                    callback.on_field("Cells Used", &build_status.cells_used.to_string());
                    callback.on_field("Proofs Run", &build_status.proofs_run.to_string());

                    // Download artifacts automatically
                    callback.on_section("Downloading Artifacts");

                    // Download ELF
                    callback.on_info("Downloading ELF...");
                    if let Err(e) = self.download_program(&build_status.id, "elf") {
                        callback.on_error(&format!("Warning: Failed to download ELF: {}", e));
                    }

                    // Download EXE
                    callback.on_info("Downloading EXE...");
                    if let Err(e) = self.download_program(&build_status.id, "exe") {
                        callback.on_error(&format!("Warning: Failed to download EXE: {}", e));
                    }

                    // Download logs
                    callback.on_info("Downloading logs...");
                    if let Err(e) = self.download_build_logs(&build_status.id) {
                        callback.on_error(&format!("Warning: Failed to download logs: {}", e));
                    }

                    return Ok(());
                }
                "error" | "failed" => {
                    callback.on_progress_finish("");
                    let error_msg = build_status
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string());
                    eyre::bail!("Build failed: {}", error_msg);
                }
                "processing" => {
                    callback.on_progress_update_message("Building program");
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
                "not_ready" => {
                    callback.on_progress_update_message("Build queued");
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
                _ => {
                    callback.on_progress_update_message(&format!(
                        "Build status: {}",
                        build_status.status
                    ));
                    std::thread::sleep(Duration::from_secs(BUILD_POLLING_INTERVAL_SECS));
                }
            }
        }
    }

    pub fn register_new_program_base(
        &self,
        program_dir: impl AsRef<Path>,
        args: BuildArgs,
        callback: &dyn ProgressCallback,
    ) -> Result<String> {
        // Check if we're in a Rust project
        if !is_rust_project(program_dir.as_ref()) {
            eyre::bail!("Not in a Rust project. Make sure Cargo.toml exists.");
        }

        let git_root = find_git_root(program_dir.as_ref()).context(
            "Not in a git repository. Please run this command from within a git repository.",
        )?;

        // Check if git repository is clean unless allow-dirty is specified
        if !args.allow_dirty {
            let is_clean = check_git_clean(&git_root)?;
            if !is_clean {
                eyre::bail!(
                    "Git repository has uncommitted changes. Please commit your changes or use --allow-dirty to build anyway.\n\
                    Run 'git status' to see uncommitted changes."
                );
            }
        }

        let config_id = match &args.config_source {
            Some(ConfigSource::ConfigId(id)) => Some(id.clone()),
            Some(ConfigSource::ConfigPath(_)) => None, // Will be handled in form data
            None => self.config.config_id.clone(),
        };

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
        callback.on_info("Creating project archive...");
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
        if let Some(project_name) = args.project_name {
            let encoded: String =
                url::form_urlencoded::byte_serialize(project_name.as_bytes()).collect();
            url.push_str(&format!("&project_name={}", encoded));
        }
        if let Some(bin) = bin_to_build {
            url.push_str(&format!("&bin_name={bin}"));
        }
        if let Ok(sha) = get_git_commit_sha(&git_root) {
            url.push_str(&format!("&commit_sha={sha}"));
        }

        callback.on_header("Building Program");

        if let Some(id) = &config_id {
            callback.on_field("Config ID", id);
        } else if let Some(ConfigSource::ConfigPath(path)) = args.config_source.clone() {
            callback.on_field("Config File", &path);
        } else {
            callback.on_field("Config", "Default");
        }

        // Start progress tracking for upload
        callback.on_progress_start("Uploading", Some(metadata.len()));

        // Use a counting reader and perform the request in a background thread while
        // polling progress from the main thread to update the callback.
        let uploaded = Arc::new(AtomicU64::new(0));
        let uploaded_for_thread = Arc::clone(&uploaded);
        let tar_path_string = tar_path.clone();
        let url_clone = url.clone();
        let api_key_owned = self
            .config
            .api_key
            .as_ref()
            .ok_or_eyre("API key not set")?
            .to_string();
        let config_source_for_form = args.config_source.clone();

        let handle = std::thread::spawn(move || -> Result<reqwest::blocking::Response> {
            let client = Client::builder()
                .timeout(std::time::Duration::from_secs(300)) // 5 minute timeout
                .build()?;

            // Open the file and wrap with counting reader
            let file = File::open(&tar_path_string).context("Failed to open tar file")?;
            let counting_reader = CountingReader {
                inner: file,
                progress: uploaded_for_thread,
            };

            // Create multipart form
            let part = reqwest::blocking::multipart::Part::reader(counting_reader)
                .file_name("program.tar.gz")
                .mime_str("application/gzip")?;

            let mut form = reqwest::blocking::multipart::Form::new().part("program", part);

            // Add config file if provided
            if let Some(ConfigSource::ConfigPath(config_path_str)) = config_source_for_form {
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

            let request = add_cli_version_header(
                client
                    .post(url_clone)
                    .header(API_KEY_HEADER, api_key_owned)
                    .multipart(form),
            );

            let response = request.send()?;
            Ok(response)
        });

        // Poll progress until the upload thread finishes
        loop {
            if handle.is_finished() {
                break;
            }
            let current = uploaded.load(Ordering::Relaxed);
            callback.on_progress_update(current);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let response = handle
            .join()
            .map_err(|e| eyre!("upload thread panicked: {e:?}"))??;

        // Finish the progress tracking
        callback.on_progress_finish("✓ Upload complete!");

        // Check if the request was successful
        if response.status().is_success() {
            let body = response
                .json::<serde_json::Value>()
                .context("Failed to parse build response as JSON")?;
            let program_id = body["id"]
                .as_str()
                .ok_or_eyre("Missing 'id' field in build response")?;
            callback.on_success(&format!("Build initiated ({})", program_id));
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

fn check_git_clean(git_root: impl AsRef<Path>) -> Result<bool> {
    // Check if the git repository is clean (no uncommitted changes)
    let output = std::process::Command::new("git")
        .current_dir(git_root.as_ref())
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to run 'git status --porcelain'")?;

    if !output.status.success() {
        eyre::bail!("Failed to check git status");
    }

    // If output is empty, the repository is clean
    Ok(output.stdout.is_empty())
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

    // Get the required rust version from rust-toolchain.toml
    let toolchain_file_content = include_str!("../../../rust-toolchain.toml");
    let doc = toolchain_file_content
        .parse::<toml_edit::Document<_>>()
        .context("Failed to parse rust-toolchain.toml")?;
    let required_version_str = doc["toolchain"]["channel"]
        .as_str()
        .ok_or_eyre("Could not find 'toolchain.channel' in rust-toolchain.toml")?;

    // Run cargo fetch with CARGO_HOME set to axiom_cargo_home
    // Fetch 1: target = x86 linux which is the cloud machine
    let status = std::process::Command::new("cargo")
        .env("CARGO_HOME", &axiom_cargo_home)
        .arg(format!("+{}", required_version_str))
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
        .arg(format!("+{}", required_version_str))
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

fn is_rust_project(dir: &Path) -> bool {
    dir.join("Cargo.toml").exists()
}
