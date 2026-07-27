use std::path::PathBuf;

use axiom_sdk::{
    AxiomSdk,
    deferral::{DeferralPrepareArgs, DeferralSdk},
};
use clap::{Args, Subcommand};
use eyre::Result;

use crate::progress::CliProgressCallback;

#[derive(Args, Debug)]
pub struct DeferralCmd {
    #[command(subcommand)]
    command: DeferralSubcommand,
}

#[derive(Debug, Subcommand)]
enum DeferralSubcommand {
    /// Derive the parent-job inputs for a verify_stark deferral job from a
    /// completed child stark proof.
    ///
    /// Downloads what it needs from the Axiom API (the config's openvm.toml
    /// and agg verifying key, and the parent program's verification baseline),
    /// verifies the child proof locally, and writes the two artifacts a parent
    /// submission needs: the child proof re-encoded as openvm-codec bytes
    /// (pass to `cargo axiom prove --deferred-proof`) and the parent's input
    /// JSON body (pass to `cargo axiom prove --input`).
    ///
    /// The first run for a config derives its deferral commitments locally
    /// (several CPU-minutes); the result is cached under ~/.axiom/cache/ and
    /// reused on later runs.
    Prepare {
        /// The child stark proof: the JSON from
        /// `cargo axiom prove download --type stark`, or its openvm-codec
        /// binary encoding
        #[clap(long, value_name = "FILE")]
        child_proof: PathBuf,

        /// The config ID (defaults to the configured config_id)
        #[clap(long, value_name = "ID")]
        config_id: Option<String>,

        /// The PARENT program ID (the verify_stark guest the parent job will
        /// prove)
        #[clap(long, value_name = "ID")]
        program_id: String,

        /// Output: the child proof as openvm-codec bytes, ready for
        /// `cargo axiom prove --deferred-proof`
        #[clap(long, value_name = "FILE")]
        out_child_bin: PathBuf,

        /// Output: the parent submission's input JSON body, ready for
        /// `cargo axiom prove --input`
        #[clap(long, value_name = "FILE")]
        out_input_json: PathBuf,
    },
}

impl DeferralCmd {
    pub fn run(self) -> Result<()> {
        let config = axiom_sdk::load_config()?;
        let sdk = AxiomSdk::new(config).with_callback(CliProgressCallback::new());

        match self.command {
            DeferralSubcommand::Prepare {
                child_proof,
                config_id,
                program_id,
                out_child_bin,
                out_input_json,
            } => {
                let args = DeferralPrepareArgs {
                    child_proof,
                    config_id,
                    program_id,
                    out_child_bin: out_child_bin.clone(),
                    out_input_json: out_input_json.clone(),
                };
                let input_commit = sdk.prepare_deferral(&args)?;
                println!("input_commit: {input_commit}");
                println!(
                    "Submit the parent job with:\n  cargo axiom prove --program-id <parent> \
                     --input {} --deferred-proof {}",
                    out_input_json.display(),
                    out_child_bin.display()
                );
                Ok(())
            }
        }
    }
}
