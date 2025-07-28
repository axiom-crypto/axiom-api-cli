use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, verify::VerifySdk};
use clap::{Args, Subcommand};
use eyre::{OptionExt, Result};

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

    /// Wait for the verification to complete
    #[clap(long)]
    wait: bool,
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
                Self::print_verify_status(&verify_status);
                Ok(())
            }
            None => {
                let proof = self
                    .proof
                    .ok_or_eyre("Proof file is required. Use --proof to specify.")?;

                let verify_id = sdk.verify_proof(self.config_id.as_deref(), proof)?;

                if self.wait {
                    sdk.wait_for_verify_completion(&verify_id)
                } else {
                    println!(
                        "To check the verification status, run: cargo axiom verify status --verify-id {verify_id}"
                    );
                    Ok(())
                }
            }
        }
    }

    fn print_verify_status(status: &axiom_sdk::verify::VerifyStatus) {
        use axiom_sdk::formatting::Formatter;

        Formatter::print_section("Verification Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Result", &status.result);
        Formatter::print_field("Created At", &status.created_at);
    }
}
