use axiom_sdk::{AxiomSdk, run::RunSdk};
use cargo_openvm::input::Input;
use clap::{Args, Subcommand};
use eyre::Result;

use crate::{formatting::Formatter, progress::CliProgressCallback};

#[derive(Args, Debug)]
pub struct RunCmd {
    #[command(subcommand)]
    command: Option<RunSubcommand>,

    #[clap(flatten)]
    run_args: RunArgs,
}

#[derive(Debug, Subcommand)]
enum RunSubcommand {
    /// Check the status of an execution
    Status {
        /// The execution ID to check status for
        #[clap(long, value_name = "ID")]
        execution_id: String,
    },
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// The ID of the program to execute
    #[arg(long, value_name = "ID")]
    program_id: Option<String>,

    /// Input data for the execution (file path or hex string)
    #[clap(long, value_parser, help = "Input to OpenVM program")]
    input: Option<Input>,

    /// Execution mode: pure (output only), meter (output + cost + instructions), segment (output + segments + instructions)
    #[clap(long, default_value = "pure", value_parser = ["pure", "meter", "segment"], help = "Execution mode")]
    mode: String,

    /// Run in detached mode (don't wait for completion)
    #[clap(long)]
    detach: bool,
}

impl RunCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let callback = CliProgressCallback::new();
        let sdk = AxiomSdk::new(config).with_callback(callback);

        match self.command {
            Some(RunSubcommand::Status { execution_id }) => {
                let execution_status = sdk.get_execution_status(&execution_id)?;
                Self::print_execution_status(&execution_status);
                Ok(())
            }
            None => {
                use crate::progress::CliProgressCallback;
                let callback = CliProgressCallback::new();
                let sdk = sdk.with_callback(callback);
                let args = axiom_sdk::run::RunArgs {
                    program_id: self.run_args.program_id,
                    input: self.run_args.input,
                    mode: self.run_args.mode,
                };
                let execution_id = sdk.execute_program(args)?;

                if !self.run_args.detach {
                    sdk.wait_for_execution_completion(&execution_id)
                } else {
                    println!("Execution started successfully! ID: {}", execution_id);
                    println!(
                        "To check the execution status, run: cargo axiom run status --execution-id {}",
                        execution_id
                    );
                    Ok(())
                }
            }
        }
    }

    fn print_execution_status(status: &axiom_sdk::run::ExecutionStatus) {
        Formatter::print_section("Execution Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Status", &status.status);
        Formatter::print_field("Mode", &status.mode);
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

        // Show mode-specific statistics
        match status.mode.as_str() {
            "meter" => {
                if status.cost.is_some() || status.total_cycle.is_some() {
                    Formatter::print_section("Execution Statistics");
                }
                if let Some(cost) = status.cost {
                    Formatter::print_field("Cost", &cost.to_string());
                }
                if let Some(total_cycle) = status.total_cycle {
                    Formatter::print_field("Total Cycles", &total_cycle.to_string());
                }
            }
            "segment" => {
                if status.num_segments.is_some() || status.total_cycle.is_some() {
                    Formatter::print_section("Execution Statistics");
                }
                if let Some(num_segments) = status.num_segments {
                    Formatter::print_field("Number of Segments", &num_segments.to_string());
                }
                if let Some(total_cycle) = status.total_cycle {
                    Formatter::print_field("Total Cycles", &total_cycle.to_string());
                }
            }
            "pure" => {
                // Pure mode only shows public values, no statistics
            }
            _ => {
                // For other modes, show cycles if available
                if let Some(total_cycle) = status.total_cycle {
                    Formatter::print_section("Execution Statistics");
                    Formatter::print_field("Total Cycles", &total_cycle.to_string());
                }
            }
        }
        // Format public values more nicely
        if let Some(public_values) = &status.public_values {
            if !public_values.is_null() {
                Formatter::print_section("Public Values");
                if let Ok(compact) = serde_json::to_string(public_values) {
                    println!("  {}", compact);
                }
            }
        }
    }
}
