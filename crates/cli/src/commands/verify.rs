use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, verify::VerifySdk};
use clap::{Args, Subcommand};
use eyre::Result;

use crate::{formatting::Formatter, progress::CliProgressCallback};

#[derive(Args, Debug)]
pub struct VerifyCmd {
    #[command(subcommand)]
    command: VerifySubcommand,
}

#[derive(Debug, Subcommand)]
enum VerifySubcommand {
    /// Verify an EVM proof
    Evm {
        /// The config ID to use for verification
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// Path to the proof file
        #[clap(long, value_name = "FILE")]
        proof: PathBuf,

        /// Run in detached mode (don't wait for completion)
        #[clap(long)]
        detach: bool,
    },
    /// Verify a STARK proof
    Stark {
        /// The program ID to use for verification
        #[clap(long, value_name = "ID")]
        program_id: String,

        /// Path to the proof file
        #[clap(long, value_name = "FILE")]
        proof: PathBuf,

        /// Run in detached mode (don't wait for completion)
        #[clap(long)]
        detach: bool,
    },
    /// Check the status of a verification
    Status {
        /// The verification ID to check status for
        #[clap(long, value_name = "ID")]
        verify_id: String,

        /// Wait for the verification to complete
        #[clap(long)]
        wait: bool,
    },
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config).with_callback(callback);

        match self.command {
            VerifySubcommand::Evm {
                config_id,
                proof,
                detach,
            } => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);
                let verify_id = sdk.verify_evm(config_id.as_deref(), proof)?;

                if !detach {
                    sdk.wait_for_evm_verify_completion(&verify_id)
                } else {
                    println!(
                        "To check the verification status, run: cargo axiom verify status --verify-id {verify_id}"
                    );
                    Ok(())
                }
            }
            VerifySubcommand::Stark {
                program_id,
                proof,
                detach,
            } => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);
                let verify_id = sdk.verify_stark(&program_id, proof)?;

                if !detach {
                    sdk.wait_for_stark_verify_completion(&verify_id)
                } else {
                    println!(
                        "To check the verification status, run: cargo axiom verify status --verify-id {verify_id}"
                    );
                    Ok(())
                }
            }
            VerifySubcommand::Status { verify_id, wait } => {
                if wait {
                    sdk.wait_for_verify_completion(&verify_id)
                } else {
                    let verify_status = sdk.get_verification_result(&verify_id)?;
                    Self::print_verify_status(&verify_status);
                    Ok(())
                }
            }
        }
    }

    fn print_verify_status(status: &axiom_sdk::verify::VerifyStatus) {
        // Just show the status information, no completion messages
        Formatter::print_section("Verification Summary");
        match status.result.as_str() {
            "verified" => Formatter::print_field("Verification Result", "✓ VERIFIED"),
            "failed" => Formatter::print_field("Verification Result", "✗ FAILED"),
            _ => Formatter::print_field("Verification Result", &status.result.to_uppercase()),
        }
        Formatter::print_field("Verification ID", &status.id);
        Formatter::print_field("Proof Type", &status.proof_type.to_uppercase());
        Formatter::print_field("Created At", &status.created_at);
    }
}
