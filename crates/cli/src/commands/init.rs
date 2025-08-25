use std::{fs, path::Path, process::Command};

use clap::Parser;
use eyre::{OptionExt, Result, WrapErr, bail};
use toml_edit::{DocumentMut, Item, Table, Value};

const MAIN_RS_PREPEND: &str = r#"#[allow(unused_imports)]
use {
    openvm_k256::Secp256k1Point,
    openvm_p256::P256Point,
    openvm_pairing::{
        bls12_381::{Bls12_381Fp2, Bls12_381G1Affine},
        bn254::{Bn254Fp2, Bn254G1Affine},
    },
};

openvm::init!();

"#;

const OPENVM_TOML_TEMPLATE: &str = r#"
openvm_version = "v1.4"
[app_fri_params.fri_params]
log_blowup = 1
log_final_poly_len = 0
num_queries = 100
proof_of_work_bits = 16

[app_vm_config.system.config]
max_constraint_degree = 3
continuation_enabled = true
num_public_values = 32
profiling = false

[app_vm_config.system.config.memory_config]
addr_space_height = 3
pointer_max_bits = 29
clk_max_bits = 29
decomp = 17
max_access_adapter_n = 32
timestamp_max_bits = 29

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = "Null"

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 128
min_block_size = 4
layout = "U8"

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 536870912
min_block_size = 4
layout = "U8"

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 32
min_block_size = 4
layout = "U8"

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = { Native = { size = 4 } }

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = { Native = { size = 4 } }

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = { Native = { size = 4 } }

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = { Native = { size = 4 } }

[[app_vm_config.system.config.memory_config.addr_spaces]]
num_cells = 0
min_block_size = 1
layout = { Native = { size = 4 } }

[app_vm_config.rv32i]

[app_vm_config.io]

[app_vm_config.keccak]

[app_vm_config.sha256]

[app_vm_config.rv32m]
range_tuple_checker_sizes = [256, 8192]

[app_vm_config.bigint]
range_tuple_checker_sizes = [256, 8192]

[app_vm_config.modular]
supported_moduli = [
    "21888242871839275222246405745257275088696311157297823662689037894645226208583",
    "21888242871839275222246405745257275088548364400416034343698204186575808495617",
    "115792089237316195423570985008687907853269984665640564039457584007908834671663",
    "115792089237316195423570985008687907852837564279074904382605163141518161494337",
    "115792089210356248762697446949407573530086143415290314195533631308867097853951",
    "115792089210356248762697446949407573529996955224135760342422259061068512044369",
    "4002409555221667393417789825735904156556882819939007885332058136124031650490837864442687629129015664037894272559787",
    "52435875175126190479447740508185965837690552500527637822603658699938581184513",
]

[app_vm_config.fp2]
supported_moduli = [
    [
        "Bn254Fp2",
        "21888242871839275222246405745257275088696311157297823662689037894645226208583",
    ],
    [
        "Bls12_381Fp2",
        "4002409555221667393417789825735904156556882819939007885332058136124031650490837864442687629129015664037894272559787",
    ],
]

[app_vm_config.pairing]
supported_curves = ["Bn254", "Bls12_381"]

[[app_vm_config.ecc.supported_curves]]
struct_name = "Bn254G1Affine"
modulus = "21888242871839275222246405745257275088696311157297823662689037894645226208583"
scalar = "21888242871839275222246405745257275088548364400416034343698204186575808495617"
a = "0"
b = "3"

[[app_vm_config.ecc.supported_curves]]
struct_name = "Secp256k1Point"
modulus = "115792089237316195423570985008687907853269984665640564039457584007908834671663"
scalar = "115792089237316195423570985008687907852837564279074904382605163141518161494337"
a = "0"
b = "7"

[[app_vm_config.ecc.supported_curves]]
struct_name = "P256Point"
modulus = "115792089210356248762697446949407573530086143415290314195533631308867097853951"
scalar = "115792089210356248762697446949407573529996955224135760342422259061068512044369"
a = "115792089210356248762697446949407573530086143415290314195533631308867097853948"
b = "41058363725152142129326129780047268409114441015993725554835256314039467401291"

[[app_vm_config.ecc.supported_curves]]
struct_name = "Bls12_381G1Affine"
modulus = "4002409555221667393417789825735904156556882819939007885332058136124031650490837864442687629129015664037894272559787"
scalar = "52435875175126190479447740508185965837690552500527637822603658699938581184513"
a = "0"
b = "4"

[leaf_fri_params.fri_params]
log_blowup = 1
log_final_poly_len = 0
num_queries = 100
proof_of_work_bits = 16

[compiler_options]
word_size = 8
enable_cycle_tracker = false

[agg_config]
max_num_user_public_values = 32
profiling = false
root_max_constraint_degree = 9

[agg_config.leaf_fri_params]
log_blowup = 1
log_final_poly_len = 0
num_queries = 100
proof_of_work_bits = 16

[agg_config.internal_fri_params]
log_blowup = 2
log_final_poly_len = 0
num_queries = 44
proof_of_work_bits = 16

[agg_config.root_fri_params]
log_blowup = 3
log_final_poly_len = 0
num_queries = 30
proof_of_work_bits = 16

[agg_config.compiler_options]
word_size = 8
enable_cycle_tracker = false
"#;

#[derive(Debug, Parser)]
#[command(name = "init", about = "Initialize a new OpenVM project")]
pub struct InitCmd {
    #[clap(flatten)]
    init_args: InitArgs,
}

impl InitCmd {
    pub fn run(self) -> Result<()> {
        execute(self.init_args)
    }
}

#[derive(Debug, Parser)]
pub struct InitArgs {
    /// Path to create the package in
    #[clap(value_name = "PATH")]
    path: Option<String>,

    /// Set the package name, default is the directory name
    #[clap(long, value_name = "NAME")]
    name: Option<String>,
}

pub fn execute(args: InitArgs) -> Result<()> {
    println!("Initializing OpenVM project...");

    // Check if cargo openvm is installed
    let check_status = Command::new("cargo")
        .arg("openvm")
        .arg("--help")
        .output()
        .map_err(|_| eyre::eyre!("cargo openvm is not installed. Please install it first."))?;

    if !check_status.status.success() {
        bail!("cargo openvm is not installed or not working properly.");
    }

    // Build the cargo openvm init command
    let mut cmd = Command::new("cargo");
    cmd.arg("openvm").arg("init");

    // Add path if provided
    if let Some(path) = &args.path {
        cmd.arg(path);
    }

    // Add name if provided
    if let Some(name) = &args.name {
        cmd.arg("--name").arg(name);
    }

    // Execute cargo openvm init
    let status = cmd.status()?;
    if !status.success() {
        bail!("Failed to initialize OpenVM project");
    }

    // Determine the project directory
    let project_dir = if let Some(path) = &args.path {
        Path::new(path).to_path_buf()
    } else {
        std::env::current_dir()?
    };

    // Modify src/main.rs to prepend the required imports
    let main_rs_path = project_dir.join("src").join("main.rs");
    if main_rs_path.exists() {
        let existing_content = fs::read_to_string(&main_rs_path)?;
        let new_content = format!("{}{}", MAIN_RS_PREPEND, existing_content);
        fs::write(&main_rs_path, new_content)?;
    }

    // Modify Cargo.toml to add additional dependencies
    let cargo_toml_path = project_dir.join("Cargo.toml");
    if cargo_toml_path.exists() {
        let cargo_content = fs::read_to_string(&cargo_toml_path)?;
        let mut doc = cargo_content.parse::<DocumentMut>()?;

        // Extract tag from existing openvm dependency
        let extracted_tag = doc
            .get("dependencies")
            .and_then(|deps| deps.get("openvm"))
            .and_then(|openvm| openvm.get("tag"))
            .and_then(|tag| tag.as_str())
            .map(|s| s.to_string());

        // Get or create dependencies table
        let deps = doc["dependencies"]
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| eyre::eyre!("Failed to access dependencies table"))?;

        // Add each dependency with proper TOML structure
        let git_url = "https://github.com/openvm-org/openvm.git";

        // Helper to create a dependency entry
        let create_dep = |tag: Option<&str>| -> Item {
            let mut table = toml_edit::InlineTable::new();
            table.insert("git", git_url.into());
            if let Some(t) = tag {
                table.insert("tag", t.into());
            }
            table.insert("default-features", false.into());
            Item::Value(Value::InlineTable(table))
        };

        let tag_as_str = extracted_tag.as_deref();

        deps["openvm-algebra-guest"] = create_dep(tag_as_str);
        deps["openvm-ecc-guest"] = create_dep(tag_as_str);

        // For openvm-pairing with features
        let mut pairing_table = toml_edit::InlineTable::new();
        pairing_table.insert("git", git_url.into());
        if let Some(t) = &extracted_tag {
            pairing_table.insert("tag", t.into());
        }
        let features = toml_edit::Array::from_iter(["bn254", "bls12_381"]);
        pairing_table.insert("features", Value::Array(features));
        deps["openvm-pairing"] = Item::Value(Value::InlineTable(pairing_table));

        // For packages with different names
        let create_dep_with_package = |package: &str, tag: Option<&str>| -> Item {
            let mut table = toml_edit::InlineTable::new();
            table.insert("git", git_url.into());
            if let Some(t) = tag {
                table.insert("tag", t.into());
            }
            table.insert("package", package.into());
            Item::Value(Value::InlineTable(table))
        };

        deps["openvm-k256"] = create_dep_with_package("k256", tag_as_str);
        deps["openvm-p256"] = create_dep_with_package("p256", tag_as_str);

        // Write back preserving formatting
        fs::write(&cargo_toml_path, doc.to_string())?;
    }

    // Create .env.example if it doesn't exist
    let env_example_path = project_dir.join(".env.example");
    if !env_example_path.exists() {
        fs::write(&env_example_path, "AXIOM_API_KEY=\n")?;
    }

    // Create .env if it doesn't exist
    let env_path = project_dir.join(".env");
    if !env_path.exists() {
        fs::write(&env_path, "AXIOM_API_KEY=\n")?;
    }

    // Update .gitignore to include .env and ./openvm
    let gitignore_path = project_dir.join(".gitignore");
    if gitignore_path.exists() {
        let gitignore_content = fs::read_to_string(&gitignore_path)?;
        let additions = vec![".env", "./openvm"];
        let mut updated_content = gitignore_content;

        for addition in &additions {
            if !updated_content.contains(addition) {
                updated_content.push_str(&format!("{}\n", addition));
            }
        }

        fs::write(&gitignore_path, updated_content)?;
    }

    // Create or replace openvm.toml file
    let openvm_toml_path = project_dir.join("openvm.toml");
    fs::write(&openvm_toml_path, OPENVM_TOML_TEMPLATE)?;

    // Run `cargo fetch` so that `Cargo.lock` will be created
    let toolchain_file_content = include_str!("../../../../rust-toolchain.toml");
    let doc = toolchain_file_content
        .parse::<toml_edit::Document<_>>()
        .context("Failed to parse rust-toolchain.toml")?;
    let required_version_str = doc["toolchain"]["channel"]
        .as_str()
        .ok_or_eyre("Could not find 'toolchain.channel' in rust-toolchain.toml")?;
    let _ = Command::new("cargo")
        .current_dir(&project_dir)
        .arg(format!("+{}", required_version_str))
        .arg("fetch")
        .status();

    // Attempt to stage and commit initialized files. Ignore failures (e.g., not a git repo or nothing to commit).
    let _ = Command::new("git")
        .current_dir(&project_dir)
        .args(["add", "."])
        .status();
    let _ = Command::new("git")
        .current_dir(&project_dir)
        .args(["commit", "-q", "-m", "initial commit"])
        .status();

    Ok(())
}
