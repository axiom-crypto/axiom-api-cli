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
    /// Get config information
    Get {
        /// The config ID to get information for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,
    },

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
            Some(ConfigSubcommand::Get { config_id }) => {
                let vm_config_metadata = sdk.get_vm_config_metadata(config_id.as_deref())?;
                println!("{}", serde_json::to_string_pretty(&vm_config_metadata)?);
                Ok(())
            }
            Some(ConfigSubcommand::Status { config_id }) => {
                let vm_config_metadata = sdk.get_vm_config_metadata(config_id.as_deref())?;
                println!("Config status: {vm_config_metadata:?}");
                Ok(())
            }
            Some(ConfigSubcommand::Download {
                config_id,
                evm_verifier,
                output,
            }) => {
                if evm_verifier {
                    sdk.get_evm_verifier(config_id.as_deref(), output)
                } else {
                    sdk.download_config(config_id.as_deref(), output)
                }
            }
            None => Err(eyre::eyre!("A subcommand is required for config")),
        }
    }
}
