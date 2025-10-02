use clap::Args;
use eyre::Result;

use crate::output::{OutputMode, print_json};

#[derive(Args, Debug)]
#[command(name = "version", about = "Display version information")]
pub struct VersionCmd {
    #[arg(long)]
    verbose: bool,
}

impl VersionCmd {
    pub fn run(self, output_mode: OutputMode) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");
        let commit = env!("GIT_COMMIT_HASH");

        match output_mode {
            OutputMode::Json => {
                if self.verbose {
                    let openvm_version = env!("OPENVM_VERSION");
                    let openvm_commit = env!("OPENVM_COMMIT");
                    print_json(&serde_json::json!({
                        "version": version,
                        "commit": commit,
                        "openvm_version": openvm_version,
                        "openvm_commit": openvm_commit,
                    }))?;
                } else {
                    print_json(&serde_json::json!({
                        "version": version,
                        "commit": commit,
                    }))?;
                }
            }
            OutputMode::Human => {
                println!("cargo-axiom v{version} ({commit})");

                if self.verbose {
                    let openvm_version = env!("OPENVM_VERSION");
                    let openvm_commit = env!("OPENVM_COMMIT");
                    println!("OpenVM compatibility: version {openvm_version} ({openvm_commit})");
                }
            }
        }

        Ok(())
    }
}
