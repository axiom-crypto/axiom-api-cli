use axiom_sdk::{
    build::{BuildSdk, ConfigSource},
    AxiomSdk,
};
use clap::{Parser, Subcommand};
use comfy_table;
use eyre::Result;

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

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// The configuration ID to use for the build
    #[clap(long, value_name = "ID", conflicts_with = "config")]
    config_id: Option<String>,

    /// Path to a local configuration file
    #[clap(long, value_name = "PATH")]
    config: Option<String>,

    /// Internal Only: Force creating the config and triggering keygen.
    #[clap(long)]
    force_keygen: bool,

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

impl BuildCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            Some(BuildSubcommand::Status { program_id }) => {
                let build_status = sdk.get_build_status(&program_id)?;
                println!(
                    "Build status: {}",
                    serde_json::to_string(&build_status).unwrap()
                );
                Ok(())
            }
            Some(BuildSubcommand::List) => {
                let build_status_list = sdk.list_programs()?;

                // Create a new table
                let mut table = comfy_table::Table::new();
                table.set_header(["ID", "Status", "Created At"]);

                // Add rows to the table
                for build_status in build_status_list {
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
                let args = axiom_sdk::build::BuildArgs {
                    config_source,
                    bin: self.build_args.bin,
                    keep_tarball: self.build_args.keep_tarball,
                    exclude_files: self.build_args.exclude_files,
                    include_dirs: self.build_args.include_dirs,
                    force_keygen: self.build_args.force_keygen,
                };
                let program_id = sdk.register_new_program(&program_dir, args)?;
                println!(
                    "To check the build status, run: cargo axiom build status --program-id {program_id}"
                );
                Ok(())
            }
        }
    }
}
