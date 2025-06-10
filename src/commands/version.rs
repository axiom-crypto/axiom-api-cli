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
            let openvm_commit = "51f07d50d20174b23091f48e25d9ea421b4e2787";
            let openvm_version = "1.2.1-rc.0";
            println!(
                "OpenVM compatibility: version {} ({})",
                openvm_version, openvm_commit
            );
        }

        Ok(())
    }
}
