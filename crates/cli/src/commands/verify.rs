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
        let sdk = AxiomSdk::new(config.clone());

        match self.command {
            Some(VerifySubcommand::Status { verify_id }) => {
                let verify_status = sdk.get_verification_result(&verify_id)?;
                Self::print_verify_status(&verify_status);
                Ok(())
            }
            None => {
                use crate::formatting::Formatter;
                use axiom_sdk::config::ConfigSdk;

                let proof = self
                    .proof
                    .ok_or_eyre("Proof file is required. Use --proof to specify.")?;

                // Get config metadata for additional information
                let config_id = axiom_sdk::get_config_id(self.config_id.as_deref(), &config)?;
                let config_metadata = sdk.get_vm_config_metadata(Some(&config_id))?;

                // Print information about what we're verifying
                Formatter::print_header("Proof Verification");
                Formatter::print_field("Proof File", &proof.display().to_string());
                Formatter::print_field("Config ID", &config_id);
                Formatter::print_field("OpenVM Version", &config_metadata.openvm_version);

                println!("\nInitiating verification...");

                let verify_id = sdk.verify_proof(self.config_id.as_deref(), proof)?;
                Formatter::print_success(&format!("Verification request sent: {verify_id}"));

                if self.wait {
                    loop {
                        let verify_status = sdk.get_verification_result(&verify_id)?;

                        match verify_status.result.as_str() {
                            "verified" => {
                                Formatter::clear_line();
                                Formatter::print_success("Verification completed successfully!");

                                // Print completion information
                                Formatter::print_section("Verification Summary");
                                Formatter::print_field("Verification Result", "✓ VERIFIED");
                                Formatter::print_field("Verification ID", &verify_status.id);
                                Formatter::print_field("Completed At", &verify_status.created_at);

                                return Ok(());
                            }
                            "failed" => {
                                Formatter::clear_line();
                                println!("\nVerification failed!");

                                // Print failure information
                                Formatter::print_section("Verification Summary");
                                Formatter::print_field("Verification Result", "✗ FAILED");
                                Formatter::print_field("Verification ID", &verify_status.id);
                                Formatter::print_field("Completed At", &verify_status.created_at);

                                eyre::bail!("Proof verification failed");
                            }
                            "processing" => {
                                Formatter::print_status("Verifying proof...");
                                std::thread::sleep(std::time::Duration::from_secs(10));
                            }
                            _ => {
                                Formatter::print_status(&format!(
                                    "Verification status: {}...",
                                    verify_status.result
                                ));
                                std::thread::sleep(std::time::Duration::from_secs(10));
                            }
                        }
                    }
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
        use crate::formatting::Formatter;

        Formatter::print_section("Verification Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Result", &status.result);
        Formatter::print_field("Created At", &status.created_at);
    }
}
