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
                use crate::formatting::{Formatter, calculate_duration};

                Formatter::print_header("Executing Program");
                if let Some(ref program_id) = self.run_args.program_id {
                    Formatter::print_field("Program ID", program_id);
                }

                let args = axiom_sdk::run::RunArgs {
                    program_id: self.run_args.program_id,
                    input: self.run_args.input,
                };
                let execution_id = sdk.execute_program(args)?;
                Formatter::print_success(&format!("Execution initiated ({})", execution_id));

                if self.run_args.wait {
                    println!();

                    loop {
                        let execution_status = sdk.get_execution_status(&execution_id)?;

                        match execution_status.status.as_str() {
                            "Succeeded" => {
                                Formatter::clear_line_and_reset();
                                Formatter::print_success("Execution completed successfully!");

                                // Print completion information
                                Formatter::print_section("Execution Summary");
                                Formatter::print_field("Execution ID", &execution_status.id);
                                if let Some(total_cycle) = execution_status.total_cycle {
                                    Formatter::print_field(
                                        "Total Cycles",
                                        &total_cycle.to_string(),
                                    );
                                }
                                if let Some(total_tick) = execution_status.total_tick {
                                    Formatter::print_field("Total Ticks", &total_tick.to_string());
                                }

                                // Format public values more nicely
                                if let Some(public_values) = &execution_status.public_values {
                                    if !public_values.is_null() {
                                        Formatter::print_section("Public Values");
                                        if let Ok(formatted) =
                                            serde_json::to_string_pretty(public_values)
                                        {
                                            for line in formatted.lines() {
                                                println!("  {}", line);
                                            }
                                        }
                                    }
                                }

                                if let Some(launched_at) = &execution_status.launched_at {
                                    if let Some(terminated_at) = &execution_status.terminated_at {
                                        Formatter::print_section("Execution Stats");
                                        Formatter::print_field(
                                            "Created",
                                            &execution_status.created_at,
                                        );
                                        Formatter::print_field("Initiated", launched_at);
                                        Formatter::print_field("Finished", terminated_at);

                                        if let Ok(duration) =
                                            calculate_duration(launched_at, terminated_at)
                                        {
                                            Formatter::print_field("Duration", &duration);
                                        }
                                    }
                                }

                                // Save execution results to file
                                if let Some(results_path) =
                                    sdk.save_execution_results(&execution_status)
                                {
                                    Formatter::print_section("Saving Results");
                                    println!("  ✓ {}", results_path);
                                }

                                return Ok(());
                            }
                            "Failed" => {
                                Formatter::clear_line_and_reset();
                                let error_msg = execution_status
                                    .error_message
                                    .unwrap_or_else(|| "Unknown error".to_string());
                                eyre::bail!("Execution failed: {}", error_msg);
                            }
                            "Queued" => {
                                Formatter::print_status("Execution queued...");
                                std::thread::sleep(std::time::Duration::from_secs(10));
                            }
                            "InProgress" => {
                                Formatter::print_status("Execution in progress...");
                                std::thread::sleep(std::time::Duration::from_secs(10));
                            }
                            _ => {
                                Formatter::print_status(&format!(
                                    "Execution status: {}...",
                                    execution_status.status
                                ));
                                std::thread::sleep(std::time::Duration::from_secs(10));
                            }
                        }
                    }
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
        use crate::formatting::Formatter;

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
