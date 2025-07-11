use std::path::PathBuf;

use axiom_sdk::{config::ConfigSdk, AxiomSdk};
use clap::Args;
use eyre::Result;

#[derive(Args, Debug)]
pub struct DownloadKeysCmd {
    /// The config ID to download public key for
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,

    /// The type of key to download
    #[clap(long, value_parser = [
        "app_vm",
        "leaf_vm",
        "internal_vm",
        "root_verifier",
        "halo2_outer",
        "halo2_wrapper",
    ])]
    r#type: String,

    /// Optional output file path (defaults to key_type name in current directory)
    #[clap(long, value_name = "FILE")]
    output: Option<PathBuf>,
}

impl DownloadKeysCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        let pk_downloader = sdk.get_proving_keys(self.config_id.as_deref(), &self.r#type)?;
        println!("Download URL: {}", pk_downloader.download_url);
        Ok(())
    }
}
