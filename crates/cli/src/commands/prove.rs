use std::path::PathBuf;

use axiom_sdk::{AxiomSdk, ProofType, input::Input, prove::ProveSdk};
use clap::{Args, Subcommand};
use comfy_table;
use eyre::Result;

use crate::{formatting::Formatter, progress::CliProgressCallback};

fn validate_priority(s: &str) -> Result<u8, String> {
    let priority: u8 = s.parse().map_err(|_| "Priority must be a number")?;
    if (1..=10).contains(&priority) {
        Ok(priority)
    } else {
        Err("Priority must be between 1 and 10".to_string())
    }
}

fn validate_num_gpus(s: &str) -> Result<usize, String> {
    let num_gpus: usize = s.parse().map_err(|_| "Number of GPUs must be a number")?;
    if (1..=10000).contains(&num_gpus) {
        Ok(num_gpus)
    } else {
        Err("Number of GPUs must be between 1 and 10000".to_string())
    }
}

#[derive(Args, Debug)]
pub struct ProveCmd {
    #[command(subcommand)]
    command: Option<ProveSubcommand>,

    #[clap(flatten)]
    prove_args: ProveArgs,
}

#[derive(Debug, Subcommand)]
enum ProveSubcommand {
    /// Check the status of a proof
    Status {
        /// The proof ID to check status for
        #[clap(long, value_name = "ID")]
        proof_id: String,

        /// Wait for the proof to complete
        #[clap(long)]
        wait: bool,

        /// Don't save the proof artifact on completion
        #[clap(long)]
        no_save: bool,
    },
    /// Download logs for a proof
    Logs {
        /// The proof ID to download logs for
        #[clap(long, value_name = "ID")]
        proof_id: String,
    },
    /// Download proof artifacts
    Download {
        /// The proof ID to download artifacts for
        #[clap(long, value_name = "ID")]
        proof_id: String,

        /// The type of proof artifact to download (stark, or evm)
        #[clap(long = "type")]
        proof_type: ProofType,

        /// Output file path (defaults to proof_id-type.json)
        #[clap(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// List all proofs for a program
    List {
        /// The ID of the program to list proofs for
        #[arg(long, value_name = "ID")]
        program_id: String,
    },
    /// Cancel a running proof
    Cancel {
        /// The proof ID to cancel
        #[clap(long, value_name = "ID")]
        proof_id: String,
    },
}

#[derive(Args, Debug)]
pub struct ProveArgs {
    /// The ID of the program to generate a proof for
    #[arg(long, value_name = "ID")]
    program_id: Option<String>,

    /// Input data for the proof (file path or hex string)
    #[clap(long, value_parser, help = "Input to OpenVM program")]
    input: Option<Input>,

    /// The type of proof to generate (stark or evm)
    #[clap(long = "type", default_value = "stark")]
    proof_type: ProofType,

    /// Run in detached mode (don't wait for completion)
    #[clap(long)]
    detach: bool,

    /// Num GPUs to use for this proof (1-10000)
    #[clap(long, value_parser = validate_num_gpus)]
    num_gpus: Option<usize>,

    /// Priority for this proof (1-10, higher = more priority)
    #[clap(long, value_parser = validate_priority)]
    priority: Option<u8>,
}

impl ProveCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config.clone()).with_callback(callback);

        match self.command {
            Some(ProveSubcommand::Status {
                proof_id,
                wait,
                no_save,
            }) => {
                if wait {
                    sdk.wait_for_proof_completion(&proof_id, !no_save)?;
                } else {
                    let proof_status = sdk.get_proof_status(&proof_id)?;
                    Self::print_proof_status(&proof_status);
                }
                Ok(())
            }
            Some(ProveSubcommand::Download {
                proof_id,
                proof_type,
                output,
            }) => {
                let output_path = output.or_else(|| match sdk.get_proof_status(&proof_id) {
                    Ok(proof_status) => {
                        let proof_dir = std::path::PathBuf::from("axiom-artifacts")
                            .join(format!("program-{}", proof_status.program_uuid))
                            .join("proofs")
                            .join(&proof_id);
                        Some(proof_dir.join(format!("{}-proof.json", proof_type)))
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not fetch proof status: {}", e);
                        eprintln!("Using fallback path for proof output");
                        let proof_dir = std::path::PathBuf::from("axiom-artifacts")
                            .join("proofs")
                            .join(&proof_id);
                        Some(proof_dir.join(format!("{}-proof.json", proof_type)))
                    }
                });
                sdk.get_generated_proof(&proof_id, &proof_type, output_path)?;
                Ok(())
            }
            Some(ProveSubcommand::Logs { proof_id }) => sdk.get_proof_logs(&proof_id),
            Some(ProveSubcommand::List { program_id }) => {
                let proof_status_list = sdk.list_proofs(&program_id)?;

                // Create a new table
                let mut table = comfy_table::Table::new();
                table.set_header(["ID", "State", "Proof type", "Created At"]);

                // Add rows to the table
                for proof_status in proof_status_list {
                    let get_value = |s: &str| {
                        if s.is_empty() {
                            "-".to_string()
                        } else {
                            s.to_string()
                        }
                    };
                    let id = get_value(&proof_status.id);
                    let status = get_value(&proof_status.state);
                    let proof_type = get_value(&proof_status.proof_type);
                    let created_at = get_value(&proof_status.created_at);

                    table.add_row([id, status, proof_type, created_at]);
                }

                // Print the table
                println!("{table}");
                Ok(())
            }
            Some(ProveSubcommand::Cancel { proof_id }) => {
                let message = sdk.cancel_proof(&proof_id)?;
                println!("✓ {}", message);

                // Wait for cancellation to complete
                sdk.wait_for_proof_cancellation(&proof_id)?;
                Ok(())
            }
            None => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);
                let args = axiom_sdk::prove::ProveArgs {
                    program_id: self.prove_args.program_id,
                    input: self.prove_args.input,
                    proof_type: Some(self.prove_args.proof_type),
                    num_gpus: self.prove_args.num_gpus,
                    priority: self.prove_args.priority,
                };
                let proof_id = sdk.generate_new_proof(args)?;

                if !self.prove_args.detach {
                    sdk.wait_for_proof_completion(&proof_id, true)?;
                } else {
                    println!(
                        "To check the proof status, run: cargo axiom prove status --proof-id {proof_id}"
                    );
                }
                Ok(())
            }
        }
    }

    fn print_proof_status(status: &axiom_sdk::prove::ProofStatus) {
        Formatter::print_section("Proof Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("State", &status.state);
        Formatter::print_field("Proof Type", &status.proof_type);
        Formatter::print_field("Program ID", &status.program_uuid);
        Formatter::print_field("Created By", &status.created_by);
        Formatter::print_field("Created At", &status.created_at);

        if let Some(launched_at) = &status.launched_at {
            Formatter::print_field("Launched At", launched_at);
        }

        if let Some(terminated_at) = &status.terminated_at {
            Formatter::print_field("Terminated At", terminated_at);
        }

        if let Some(error_message) = &status.error_message {
            Formatter::print_field("Error", error_message);
        }

        Formatter::print_section("Configuration");
        Formatter::print_field("Num GPUs", &status.num_gpus.to_string());
        Formatter::print_field("Priority", &status.priority.to_string());

        Formatter::print_section("Statistics");
        Formatter::print_field("Cells Used", &status.cells_used.to_string());
        if let Some(num_instructions) = status.num_instructions {
            Formatter::print_field("Total Cycles", &num_instructions.to_string());
        }
    }
}
