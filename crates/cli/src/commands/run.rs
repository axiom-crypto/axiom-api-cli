use axiom_sdk::{AxiomSdk, run::RunSdk};
use cargo_openvm::input::Input;
use clap::{Args, Subcommand};
use eyre::Result;

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
    #[arg(long)]
    program_id: Option<String>,

    /// Input data for the execution (file path or hex string)
    #[clap(long, value_parser, help = "Input to OpenVM program")]
    input: Option<Input>,

    /// Wait for the execution to complete
    #[clap(long)]
    wait: bool,
}

impl RunCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            Some(RunSubcommand::Status { execution_id }) => {
                let execution_status = sdk.get_execution_status(&execution_id)?;
                Self::print_execution_status(&execution_status);
                Ok(())
            }
            None => {
                let args = axiom_sdk::run::RunArgs {
                    program_id: self.run_args.program_id,
                    input: self.run_args.input,
                };
                let execution_id = sdk.execute_program(args)?;

                if self.run_args.wait {
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
        use axiom_sdk::formatting::Formatter;

        Formatter::print_section("Execution Status");
        Formatter::print_field("ID", &status.id);
        Formatter::print_field("Status", &status.status);
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

        if let Some(total_cycle) = status.total_cycle {
            Formatter::print_section("Execution Statistics");
            Formatter::print_field("Total Cycles", &total_cycle.to_string());
        }

        if let Some(total_tick) = status.total_tick {
            if status.total_cycle.is_none() {
                Formatter::print_section("Execution Statistics");
            }
            Formatter::print_field("Total Ticks", &total_tick.to_string());
        }

        // Format public values more nicely
        if let Some(public_values) = &status.public_values {
            if !public_values.is_null() {
                Formatter::print_section("Public Values");
                if let Ok(formatted) = serde_json::to_string_pretty(public_values) {
                    for line in formatted.lines() {
                        println!("  {}", line);
                    }
                }
            }
        }
    }
}
