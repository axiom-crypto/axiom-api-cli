use std::path::PathBuf;

use crate::formatting::Formatter;
use axiom_sdk::{
    AxiomSdk,
    verify::{ProofType, VerifySdk},
};
use clap::{Args, Subcommand};
use eyre::Result;

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

        /// Wait for the verification to complete
        #[clap(long)]
        wait: bool,
    },
    /// Verify a STARK proof
    Stark {
        /// The program ID to use for verification
        #[clap(long, value_name = "ID")]
        program_id: String,

        /// Path to the proof file
        #[clap(long, value_name = "FILE")]
        proof: PathBuf,

        /// Wait for the verification to complete
        #[clap(long)]
        wait: bool,
    },
    /// Check the status of a verification
    Status {
        /// The verification ID to check status for
        #[clap(long, value_name = "ID")]
        verify_id: String,

        /// The proof type (evm or stark)
        #[clap(long, value_name = "TYPE")]
        proof_type: ProofType,
    },
}

impl VerifyCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            VerifySubcommand::Evm {
                config_id,
                proof,
                wait,
            } => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let verify_id =
                    sdk.verify_evm_base(config_id.as_deref(), proof, Some(&callback))?;

                if wait {
                    sdk.wait_for_evm_verify_completion_base(&verify_id, Some(&callback))
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
                wait,
            } => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let verify_id = sdk.verify_stark_base(&program_id, proof, Some(&callback))?;

                if wait {
                    sdk.wait_for_stark_verify_completion_base(&verify_id, Some(&callback))
                } else {
                    println!(
                        "To check the verification status, run: cargo axiom verify status --verify-id {verify_id} --proof-type stark"
                    );
                    Ok(())
                }
            }
            VerifySubcommand::Status {
                verify_id,
                proof_type,
            } => {
                let verify_status = match proof_type {
                    ProofType::Evm => sdk.get_evm_verification_result(&verify_id)?,
                    ProofType::Stark => sdk.get_stark_verification_result(&verify_id)?,
                };
                Self::print_verify_status(&verify_status, proof_type);
                Ok(())
            }
        }
    }

    fn print_verify_status(status: &axiom_sdk::verify::VerifyStatus, proof_type: ProofType) {
        Formatter::print_section("Verification Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Proof Type", &proof_type.to_string().to_uppercase());
        Formatter::print_field("Result", &status.result);
        Formatter::print_field("Created At", &status.created_at);
    }
}
