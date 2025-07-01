use std::path::PathBuf;

use axiom_sdk::{config::ConfigSdk, AxiomSdk};
use clap::{Args, Subcommand};
use eyre::Result;

#[derive(Args, Debug)]
pub struct ConfigCmd {
    #[command(subcommand)]
    command: Option<ConfigSubcommand>,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    /// Download config artifacts: proving keys, evm verifier, leaf committed exe etc.
    Download {
        /// The config ID to download public key for
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// The type of key to download
        #[clap(long, value_parser = [
            // These will give a download URL because the files are huge
            "app_vm",
            "leaf_vm",
            "internal_vm",
            "root_verifier",
            "halo2_outer",
            "halo2_wrapper",
            // These will download (stream) the file because they are small
            "config",
            "evm_verifier",
            "app_vm_commit",
        ])]
        key_type: String,

        /// Optional output file path (defaults to key_type name in current directory)
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
            Some(ConfigSubcommand::Status { config_id }) => {
                let vm_config_metadata = sdk.get_vm_config_metadata(config_id.as_deref())?;
                println!("Config status: {:?}", vm_config_metadata);
                Ok(())
            }
            Some(ConfigSubcommand::Download {
                config_id,
                key_type,
                output,
            }) => match key_type.as_str() {
                "evm_verifier" => sdk.get_evm_verifier(config_id.as_deref(), output),
                "app_vm_commit" => sdk.get_vm_commitment(config_id.as_deref(), output),
                "config" => sdk.download_config(config_id.as_deref(), output),
                _ => {
                    let pk_downloader = sdk.get_proving_keys(config_id.as_deref(), &key_type)?;
                    println!("Download URL: {}", pk_downloader.download_url);
                    Ok(())
                }
            },
            None => Err(eyre::eyre!("A subcommand is required for config")),
        }
    }
}
