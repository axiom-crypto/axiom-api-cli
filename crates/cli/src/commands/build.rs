use std::io::{self, Write};

use axiom_sdk::{
    AxiomSdk,
    build::{BuildSdk, ConfigSource},
};
use clap::{Parser, Subcommand};
use comfy_table;
use eyre::Result;

use crate::{formatting::Formatter, progress::CliProgressCallback};

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

        /// Wait for the build to complete
        #[clap(long)]
        wait: bool,
    },

    /// List all build programs
    List {
        /// Page number (default: 1)
        #[arg(long, default_value = "1")]
        page: u32,
        /// Page size (default: 20)
        #[arg(long, default_value = "20")]
        page_size: u32,
    },

    /// Download build artifacts
    Download {
        /// The program ID to download artifacts for
        #[clap(long, value_name = "ID")]
        program_id: String,

        /// The type of artifact to download (exe or elf)
        #[clap(long, value_name = "TYPE", value_parser = ["exe", "elf", "source", "app_exe_commit"])]
        program_type: String,
    },

    /// Download build logs for a program
    Logs {
        /// The program ID to download logs for
        #[clap(long, value_name = "ID")]
        program_id: String,
    },
}

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// The configuration ID to use for the build
    #[clap(long, value_name = "ID", conflicts_with = "config")]
    config_id: Option<String>,

    /// Path to an OpenVM TOML configuration file
    #[clap(long, value_name = "PATH")]
    config: Option<String>,

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

    /// The project ID to associate with the build
    #[arg(long, value_name = "ID")]
    project_id: Option<String>,

    /// Run in detached mode (don't wait for completion)
    #[clap(long)]
    detach: bool,

    /// Allow building with uncommitted changes
    #[clap(long)]
    allow_dirty: bool,

    /// Specify default_num_gpus for this program
    #[clap(long)]
    default_num_gpus: Option<usize>,

    /// OpenVM Rust toolchain version (e.g., nightly-2025-02-14)
    #[clap(long, value_name = "VERSION")]
    openvm_rust_toolchain: Option<String>,
}

impl BuildCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config.clone()).with_callback(callback);

        match self.command {
            Some(BuildSubcommand::Status { program_id, wait }) => {
                if wait {
                    sdk.wait_for_build_completion(&program_id)
                } else {
                    let build_status = sdk.get_build_status(&program_id)?;
                    Self::print_build_status(&build_status);
                    Ok(())
                }
            }
            Some(BuildSubcommand::List { page, page_size }) => {
                let response = sdk.list_programs(Some(page), Some(page_size))?;

                if response.items.is_empty() {
                    println!("No programs found");
                    return Ok(());
                }

                // Create a new table
                let mut table = comfy_table::Table::new();
                table.set_header(["ID", "Status", "Created At"]);

                // Add rows to the table
                for build_status in response.items {
                    let get_value = |s: &str| {
                        if s.is_empty() {
                            "-".to_string()
                        } else {
                            s.to_string()
                        }
                    };
                    let id = get_value(&build_status.id);
                    let status = get_value(&build_status.status);
                    let created_at = get_value(&build_status.created_at);

                    table.add_row([id, status, created_at]);
                }

                // Print the table
                println!("{table}");

                let pagination = &response.pagination;
                println!(
                    "Showing page {} of {} (total: {} programs)",
                    pagination.page, pagination.pages, pagination.total
                );

                Ok(())
            }
            Some(BuildSubcommand::Download {
                program_id,
                program_type,
            }) => sdk.download_program(&program_id, &program_type),
            Some(BuildSubcommand::Logs { program_id }) => sdk.download_build_logs(&program_id),
            None => {
                let program_dir = std::env::current_dir()?;
                let config_source = match (self.build_args.config_id, self.build_args.config) {
                    (Some(config_id), _) => Some(ConfigSource::ConfigId(config_id)),
                    (_, Some(config)) => Some(ConfigSource::ConfigPath(config)),
                    (None, None) => None,
                };

                let project_id = {
                    let cache_path = program_dir.join(".axiom").join("project-id");
                    match std::fs::read_to_string(&cache_path) {
                        Ok(contents) => {
                            let trimmed = contents.trim();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            }
                        }
                        Err(_) => None,
                    }
                };
                let had_cached_pid = project_id.is_some();
                let project_name_for_creation = if had_cached_pid {
                    None
                } else {
                    // No project ID found, prompt for a new project name (optional)
                    print!("Enter a project name (leave blank to skip): ");
                    let _ = io::stdout().flush();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    let name = input.trim().to_string();
                    if name.is_empty() { None } else { Some(name) }
                };

                let args = axiom_sdk::build::BuildArgs {
                    config_source,
                    bin: self.build_args.bin,
                    keep_tarball: self.build_args.keep_tarball,
                    exclude_files: self.build_args.exclude_files,
                    include_dirs: self.build_args.include_dirs,
                    project_id,
                    project_name: project_name_for_creation.clone(),
                    allow_dirty: self.build_args.allow_dirty,
                    default_num_gpus: self.build_args.default_num_gpus,
                    openvm_rust_toolchain: self.build_args.openvm_rust_toolchain,
                };
                let program_id = sdk.register_new_program(&program_dir, args)?;

                // Always fetch the latest build status to get project ID and print console URL
                let status = sdk.get_build_status(&program_id)?;

                if let Some(base) = sdk.config.console_base_url.clone() {
                    let console_url = format!(
                        "{}/projects/{}",
                        base.trim_end_matches('/'),
                        status.project_id,
                    );
                    println!("Console: {}", console_url);
                }

                // If we didn't have a cached project ID, try to fetch and cache it now
                if !had_cached_pid {
                    let cache_dir = program_dir.join(".axiom");
                    let cache_path = cache_dir.join("project-id");
                    if !cache_path.exists() {
                        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                            eprintln!("Warning: failed to create .axiom directory: {e}");
                        } else if let Err(e) =
                            std::fs::write(&cache_path, status.project_id.as_bytes())
                        {
                            eprintln!("Warning: failed to write project ID cache: {e}");
                        } else {
                            println!("✓ Saved project ID {} for future builds", status.project_id);
                        }
                    }
                }

                if !self.build_args.detach {
                    sdk.wait_for_build_completion(&program_id)
                } else {
                    println!(
                        "To check the build status, run: cargo axiom build status --program-id {program_id}"
                    );
                    Ok(())
                }
            }
        }
    }

    fn print_build_status(status: &axiom_sdk::build::BuildStatus) {
        Formatter::print_section("Build Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Name", &status.name);
        Formatter::print_field("Project ID", &status.project_id);
        Formatter::print_field("Project Name", &status.project_name);
        Formatter::print_field("Status", &status.status);
        Formatter::print_field("Program Hash", &status.program_hash);
        Formatter::print_field("Config ID", &status.config_uuid);
        Formatter::print_field("Created By", &status.created_by);
        Formatter::print_field("Created At", &status.created_at);
        Formatter::print_field("Last Active", &status.last_active_at);

        if let Some(launched_at) = &status.launched_at {
            Formatter::print_field("Launched At", launched_at);
        }

        if let Some(terminated_at) = &status.terminated_at {
            Formatter::print_field("Terminated At", terminated_at);
        }

        if let Some(error_message) = &status.error_message {
            Formatter::print_field("Error", error_message);
        }
        Formatter::print_field("Default Num GPUs", &status.default_num_gpus.to_string());
    }
}
