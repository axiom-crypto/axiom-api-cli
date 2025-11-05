use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, config::ConfigSdk};
use clap::{Args, Subcommand};
use eyre::Result;

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
        let sdk = AxiomSdk::new(config);

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
                if evm_verifier {
                    sdk.get_evm_verifier(config_id.as_deref(), output.into())?;
                } else {
                    sdk.download_config(config_id.as_deref(), output.into())?;
                }
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
