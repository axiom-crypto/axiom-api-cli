use axiom_sdk::{
    AxiomSdk,
    build::{BuildSdk, UploadElfArgs},
};
use clap::Parser;
use eyre::Result;

use crate::progress::CliProgressCallback;

#[derive(Debug, Parser)]
#[command(
    name = "upload-elf",
    about = "Upload pre-built ELF and VMEXE to Axiom Proving Service"
)]
pub struct UploadElfCmd {
    /// The configuration ID to use
    #[clap(long, value_name = "ID")]
    config_id: String,

    /// The project ID to associate with the program
    #[arg(long, value_name = "ID")]
    project_id: Option<String>,

    /// The project name if creating a new project
    #[arg(long, value_name = "NAME")]
    project_name: Option<String>,

    /// The binary name
    #[clap(long, value_name = "BIN")]
    bin_name: Option<String>,

    /// Custom program name
    #[clap(long, value_name = "NAME")]
    program_name: Option<String>,

    /// Default number of GPUs for this program
    #[clap(long)]
    default_num_gpus: Option<usize>,
}

impl UploadElfCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config.clone()).with_callback(callback);

        let program_dir = std::env::current_dir()?;

        let args = UploadElfArgs {
            config_id: self.config_id,
            project_id: self.project_id,
            project_name: self.project_name,
            bin_name: self.bin_name,
            program_name: self.program_name,
            default_num_gpus: self.default_num_gpus,
        };

        let program_id = sdk.upload_elf(&program_dir, args)?;

        // Print console URL if available
        let status = sdk.get_build_status(&program_id)?;

        if let Some(base) = sdk.config.console_base_url.clone() {
            let console_url = format!(
                "{}/projects/{}",
                base.trim_end_matches('/'),
                status.project_id,
            );
            println!("Console: {}", console_url);
        }

        Ok(())
    }
}
