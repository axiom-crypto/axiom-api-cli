use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, config::ConfigSdk};
use clap::Args;
use eyre::Result;

#[derive(Args, Debug)]
pub struct DownloadKeysCmd {
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
}

impl DownloadKeysCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        let pk_downloader = sdk.get_proving_keys(self.config_id.as_deref(), &self.key_type)?;
        println!("Download URL: {}", pk_downloader.download_url);
        Ok(())
    }
}
