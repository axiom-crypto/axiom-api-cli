use axiom_sdk::{run::RunSdk, AxiomSdk};
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
}

impl RunCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config);

        match self.command {
            Some(RunSubcommand::Status { execution_id }) => {
                let execution_status = sdk.get_execution_status(&execution_id)?;
                println!(
                    "Execution status: {}",
                    serde_json::to_string_pretty(&execution_status).unwrap()
                );
                Ok(())
            }
            None => {
                let args = axiom_sdk::run::RunArgs {
                    program_id: self.run_args.program_id,
                    input: self.run_args.input,
                };
                let execution_id = sdk.execute_program(args)?;
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
