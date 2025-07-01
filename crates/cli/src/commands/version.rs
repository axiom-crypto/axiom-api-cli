use clap::Args;
use eyre::Result;

#[derive(Args, Debug)]
#[command(name = "version", about = "Display version information")]
pub struct VersionCmd {
    #[arg(long)]
    verbose: bool,
}

impl VersionCmd {
    pub fn run(self) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");
        let commit = env!("GIT_COMMIT_HASH");

        println!("cargo-axiom v{} ({})", version, commit);

        if self.verbose {
            let openvm_version = env!("OPENVM_VERSION");
            let openvm_commit = env!("OPENVM_COMMIT");
            println!(
                "OpenVM compatibility: version {} ({})",
                openvm_version, openvm_commit
            );
        }

        Ok(())
    }
}
