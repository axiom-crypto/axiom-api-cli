use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, config::ConfigSdk};
use clap::{Args, Subcommand};
use eyre::Result;

use crate::progress::CliProgressCallback;

#[derive(Args, Debug)]
pub struct ConfigCmd {
    #[command(subcommand)]
    command: Option<ConfigSubcommand>,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    /// Download config artifacts
    Download {
        /// The config ID to download for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// Download EVM verifier instead of config
        #[clap(long)]
        evm_verifier: bool,

        /// Optional output file path (defaults to artifact name in current directory)
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Download proving keys
    #[command(name = "download-keys")]
    DownloadKeys {
        /// The config ID to download public key for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// The type of key to download
        #[clap(long = "type", value_parser = [
            "app_pk",
            "agg_pk",
            "halo2_pk",
            "app_vk",
            "agg_vk",
        ])]
        key_type: String,

        /// Optional output file path (defaults to key_type name in current directory)
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Get config information
    Status {
        /// The config ID to check status for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,
    },
}

impl ConfigCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config.clone());

        match self.command {
            Some(ConfigSubcommand::Status { config_id }) => {
                let vm_config_metadata = sdk.get_vm_config_metadata(config_id.as_deref())?;
                Self::print_config_status(&vm_config_metadata);
                Ok(())
            }
            Some(ConfigSubcommand::Download {
                config_id,
                evm_verifier,
                output,
            }) => {
                let output_path = output.or_else(|| {
                    let config_id_str = config_id
                        .as_deref()
                        .or(config.config_id.as_deref())
                        .unwrap_or("default");
                    let config_dir = std::path::PathBuf::from("axiom-artifacts")
                        .join("configs")
                        .join(config_id_str);
                    if evm_verifier {
                        Some(config_dir.join("evm_verifier.json"))
                    } else {
                        Some(config_dir.join("config.toml"))
                    }
                });

                if evm_verifier {
                    sdk.get_evm_verifier(config_id.as_deref(), output_path)?;
                } else {
                    sdk.download_config(config_id.as_deref(), output_path)?;
                }
                Ok(())
            }
            Some(ConfigSubcommand::DownloadKeys {
                config_id,
                key_type,
                output,
            }) => {
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);

                let pk_downloader = sdk.get_proving_keys(config_id.as_deref(), &key_type)?;

                let output_path = match output {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => format!("{}.bin", key_type),
                };

                pk_downloader
                    .download_pk_with_callback(&output_path, &CliProgressCallback::new())?;
                println!("✓ Downloaded to: {}", output_path);
                Ok(())
            }
            None => Err(eyre::eyre!("A subcommand is required for config")),
        }
    }

    fn print_config_status(metadata: &axiom_sdk::config::VmConfigMetadata) {
        use crate::formatting::Formatter;

        Formatter::print_section("Config Status");
        Formatter::print_field("ID", &metadata.id);
        Formatter::print_field("Status", &metadata.status);
        Formatter::print_field("OpenVM Version", &metadata.openvm_version);
        Formatter::print_field("STARK Backend Version", &metadata.stark_backend_version);
        Formatter::print_field("Active", &metadata.active.to_string());
        Formatter::print_field("Created At", &metadata.created_at);
        Formatter::print_field("App VM Commit", &metadata.app_vm_commit);
    }
}
