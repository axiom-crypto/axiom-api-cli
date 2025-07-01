use std::process::Command;

use cargo_metadata::MetadataCommand;

fn main() {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output();
    let git_hash = match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", git_hash);

    let metadata = MetadataCommand::new()
        .exec()
        .expect("Failed to get cargo metadata");

    let mut openvm_version = "unknown".to_string();
    let mut openvm_commit = "unknown".to_string();

    for package in &metadata.packages {
        if package.name == "openvm-sdk" {
            if let Some(source) = &package.source {
                eprintln!("Found openvm-sdk source: {}", source.repr);

                if source.repr.starts_with("git+") {
                    if let Some(tag_start) = source.repr.find("tag=") {
                        let tag_part = &source.repr[tag_start + 4..];
                        if let Some(tag_end) = tag_part.find('#') {
                            openvm_version = tag_part[..tag_end].to_string();
                        } else {
                            openvm_version = tag_part.to_string();
                        }
                    }
                    if let Some(hash_start) = source.repr.find('#') {
                        openvm_commit = source.repr[hash_start + 1..].to_string();
                    }
                }
            } else {
                eprintln!("openvm-sdk package found but no source information");
            }
            break;
        }
    }

    eprintln!("Extracted OpenVM version: {}", openvm_version);
    eprintln!("Extracted OpenVM commit: {}", openvm_commit);

    println!("cargo:rustc-env=OPENVM_VERSION={}", openvm_version);
    println!("cargo:rustc-env=OPENVM_COMMIT={}", openvm_commit);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
