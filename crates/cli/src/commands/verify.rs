use std::path::PathBuf;

use axiom_sdk::{verify::VerifySdk, AxiomSdk};
use clap::{Args, Subcommand};
use eyre::Result;

#[derive(Args, Debug)]
pub struct VerifyCmd {
    #[command(subcommand)]
    command: Option<VerifySubcommand>,

    /// The config ID to use for verification
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,

    /// Path to the proof file
    #[clap(long, value_name = "FILE")]
    proof: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum VerifySubcommand {
    /// Check the status of a verification
    Status {
        /// The verification ID to check status for
        #[clap(long, value_name = "ID")]
        verify_id: String,
    },
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            Some(VerifySubcommand::Status { verify_id }) => {
                let verify_status = sdk.get_verification_result(&verify_id)?;
                println!(
                    "Verification status: {}",
                    serde_json::to_string(&verify_status).unwrap()
                );
                Ok(())
            }
            None => {
                let proof = self.proof.ok_or_else(|| {
                    eyre::eyre!("Proof file is required. Use --proof to specify.")
                })?;

                let verify_id = sdk.verify_proof(self.config_id.as_deref(), proof)?;
                println!(
                    "To check the verification status, run: cargo axiom verify status --verify-id {}",
                    verify_id
                );
                Ok(())
            }
        }
    }
}
