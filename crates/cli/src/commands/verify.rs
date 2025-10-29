use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, ProofType, verify::VerifySdk};
use clap::{Args, Subcommand};
use eyre::Result;

use crate::{formatting::Formatter, progress::CliProgressCallback};

#[derive(Args, Debug)]
pub struct VerifyCmd {
    #[command(subcommand)]
    command: Option<VerifySubcommand>,

    #[clap(flatten)]
    verify_args: VerifyArgs,
}

#[derive(Debug, Subcommand)]
enum VerifySubcommand {
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

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// The type of proof to verify (stark or evm)
    #[clap(long = "type")]
    proof_type: Option<ProofType>,

    /// The program ID to use for verification (required for STARK proofs)
    #[clap(long, value_name = "ID")]
    program_id: Option<String>,

    /// The config ID to use for verification (optional for EVM proofs)
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,

    /// Path to the proof file
    #[clap(long, value_name = "FILE")]
    proof: Option<PathBuf>,

    /// Run in detached mode (don't wait for completion)
    #[clap(long)]
    detach: bool,
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config).with_callback(callback);

        match self.command {
            Some(VerifySubcommand::Status { verify_id, wait }) => {
                if wait {
                    sdk.wait_for_verify_completion(&verify_id)
                } else {
                    let verify_status = sdk.get_verification_result(&verify_id)?;
                    Self::print_verify_status(&verify_status);
                    Ok(())
                }
            }
            None => {
                // Main verify command with --type flag
                let proof_type = self
                    .verify_args
                    .proof_type
                    .ok_or_else(|| eyre::eyre!("--type is required. Must be one of: stark, evm"))?;

                let proof = self
                    .verify_args
                    .proof
                    .ok_or_else(|| eyre::eyre!("--proof is required"))?;

                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);

                let verify_id = match proof_type {
                    ProofType::Stark => {
                        let program_id = self.verify_args.program_id.ok_or_else(|| {
                            eyre::eyre!("--program-id is required for STARK proof verification")
                        })?;
                        sdk.verify_stark(&program_id, proof)?
                    }
                    ProofType::Evm => {
                        sdk.verify_evm(self.verify_args.config_id.as_deref(), proof)?
                    }
                };

                if !self.verify_args.detach {
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
