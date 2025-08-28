use std::path::PathBuf;

use crate::{formatting::Formatter, progress::CliProgressCallback};
use axiom_sdk::{AxiomSdk, prove::ProveSdk};
use cargo_openvm::input::Input;
use clap::{Args, Subcommand};
use comfy_table;
use eyre::Result;

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
        #[clap(long = "type", value_parser = ["stark", "evm"])]
        proof_type: String,

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
    #[clap(long = "type", value_parser = ["stark", "evm"], default_value = "stark")]
    proof_type: String,

    /// Wait for the proof to complete and download artifacts
    #[clap(long)]
    wait: bool,
}

impl ProveCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config.clone()).with_callback(callback);

        match self.command {
            Some(ProveSubcommand::Status { proof_id }) => {
                let proof_status = sdk.get_proof_status(&proof_id)?;
                Self::print_proof_status(&proof_status);
                Ok(())
            }
            Some(ProveSubcommand::Download {
                proof_id,
                proof_type,
                output,
            }) => sdk.get_generated_proof(&proof_id, &proof_type, output),
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
            None => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);
                let args = axiom_sdk::prove::ProveArgs {
                    program_id: self.prove_args.program_id,
                    input: self.prove_args.input,
                    proof_type: Some(self.prove_args.proof_type),
                };
                let proof_id = sdk.generate_new_proof(args)?;

                if self.prove_args.wait {
                    sdk.wait_for_proof_completion(&proof_id)
                } else {
                    println!(
                        "To check the proof status, run: cargo axiom prove status --proof-id {proof_id}"
                    );
                    Ok(())
                }
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

        Formatter::print_section("Statistics");
        Formatter::print_field("Cells Used", &status.cells_used.to_string());
    }
}
