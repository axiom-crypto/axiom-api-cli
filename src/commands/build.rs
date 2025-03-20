use crate::config;
use anyhow::{Context, Result};
use clap::Parser;
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::blocking::Client;
use std::fs::File;
use std::path::Path;
use tar::Builder;
use walkdir;

#[derive(Debug, Parser)]
#[command(name = "build", about = "Build the project on Axiom Proving Service")]
pub struct BuildCmd {
    #[clap(flatten)]
    build_args: BuildArgs,
}

impl BuildCmd {
    pub fn run(self) -> Result<()> {
        execute(self.build_args)
    }
}

#[derive(Debug, Parser)]
pub struct BuildArgs {
    /// The configuration ID to use for the build
    #[clap(long, value_name = "ID")]
    config_id: Option<String>,
}

fn is_rust_project() -> bool {
    Path::new("Cargo.toml").exists() && Path::new("Cargo.lock").exists()
}

fn create_tar_archive() -> Result<String> {
    let tar_path = "program.tar.gz";
    let tar_file = File::create(tar_path)?;
    let enc = GzEncoder::new(tar_file, Compression::default());
    let mut builder = Builder::new(enc);

    // Get the current directory name
    let current_dir = std::env::current_dir()?;
    let dir_name = current_dir
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Failed to get current directory name"))?
        .to_string_lossy()
        .to_string();

    // Walk through the directory and add files to the archive
    let walker = walkdir::WalkDir::new(".")
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();

            // Skip dotfiles, target directory, and the tar file itself
            !(file_name.starts_with(".")
                || path.starts_with("./target")
                || path.starts_with("./openvm")
                || path.starts_with("./program.tar.gz"))
        });

    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            // Create path with the parent directory name
            let relative_path = path.strip_prefix(".").unwrap();
            let archive_path = format!("{}/{}", dir_name, relative_path.display());

            let mut file = File::open(path)?;
            builder.append_file(archive_path, &mut file)?;
        } else if path.is_dir() {
            // Skip directories that start with dot or are "target"
            let dir_name_str = path.file_name().unwrap_or_default().to_string_lossy();
            if dir_name_str.starts_with(".") || dir_name_str == "target" {
                continue;
            }

            // Create directory in the archive
            let relative_path = path.strip_prefix(".").unwrap();
            let archive_path = format!("{}/{}", dir_name, relative_path.display());
            builder.append_dir(archive_path, path)?;
        }
    }

    builder.finish()?;
    Ok(tar_path.to_string())
}

pub fn execute(args: BuildArgs) -> Result<()> {
    // Check if we're in a Rust project
    if !is_rust_project() {
        return Err(anyhow::anyhow!(
            "Not in a Rust project. Make sure Cargo.toml and Cargo.lock exist."
        ));
    }

    // Get the config_id from args, return error if not provided
    let config_id = args
        .config_id
        .ok_or_else(|| anyhow::anyhow!("Config ID is required. Use --config-id to specify."))?;

    // Create tar archive of the current directory
    println!("Creating archive of the project...");
    let tar_path = create_tar_archive().context("Failed to create project archive")?;

    // Use the staging API URL
    let config = config::load_config()?;
    let url = format!("{}/programs?config_uuid={}", config.api_url, config_id);

    println!("Sending build request for config ID: {}", config_id);

    // Make the POST request with multipart form data
    let client = Client::new();
    let api_key = config::get_api_key()?;

    let form = reqwest::blocking::multipart::Form::new()
        .file("program", &tar_path)
        .context("Failed to attach program archive")?;

    let response = client
        .post(url)
        .header("APS-API-Key", api_key)
        .multipart(form)
        .send()
        .context("Failed to send build request")?;

    // Clean up the tar file
    std::fs::remove_file(tar_path).ok();

    // Check if the request was successful
    if response.status().is_success() {
        println!(
            "Build request sent successfully: {}",
            response.text().unwrap()
        );
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Build request failed with status: {}",
            response.status()
        ))
    }
}
