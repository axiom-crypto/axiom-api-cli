//! Client-side deferral submission prep: derive everything a caller needs to
//! submit a `verify_stark` parent job from a child stark proof.
//!
//! The deferral design makes the CALLER responsible for computing the
//! guest-visible stdin values — notably `input_commit` — from the child proof
//! with the Rust SDK; there is no server-side fingerprint endpoint. This
//! module packages that derivation behind the Axiom API surface so no keyset
//! internals leak into the interface:
//!
//! - the config's complete `openvm.toml` (`GET /configs/{id}/config`) carries
//!   the deferral circuit commit (`[[app_vm_config.deferral.circuits]]`);
//! - the config's `agg_vk` (`GET /configs/{id}/vk/agg`) is the child proof's
//!   `MultiStarkVerifyingKey`;
//! - the PARENT program's `baseline.json`
//!   (`GET /programs/{id}/download/baseline`) is the config-level
//!   `VerificationBaseline` template;
//! - the two remaining keyset values (`def_hook_commit` and the verify-stark
//!   circuit's cached commit) are deterministic fixed points of the config's
//!   keygen parameters, so they are DERIVED locally from the downloaded toml
//!   (exactly how backend keygen derives them: `DeferralAggProver::verify_stark`
//!   on the agg params + 100-bit hook params + memory dimensions + num user
//!   PVs) and cached under `~/.axiom/cache/` because the construction costs
//!   several CPU-minutes.
//!
//! Outputs mirror the backend's internal helper:
//! - the child proof re-encoded as openvm-codec bytes
//!   (`VersionedVmStarkProof::encode_to_vec`) — the ONLY encoding
//!   `cargo axiom prove --deferred-proof` uploads;
//! - the parent's API input body `{"input": ["0x01<hex>"]}` — one entry
//!   carrying the exact byte stream `StdIn::write(&input_commit)` produces;
//! - the raw 32-byte `input_commit` (hex) for logging.

use std::{borrow::Borrow, path::PathBuf, slice::from_ref};

use eyre::{Context, Result, eyre};
use openvm_continuations::CommitBytes;
use openvm_sdk::{
    F, StdIn,
    config::{AggregationConfig, AggregationSystemParams, AppConfig},
    fs::read_object_from_file,
    openvm_circuit::{
        arch::hasher::poseidon2::vm_poseidon2_hasher, system::program::trace::compute_exe_commit,
    },
    prover::DeferralAggProver,
    types::{VerificationBaselineJson, VersionedVmStarkProof},
};
use openvm_sdk_config::{SdkVmConfig, deferral::SupportedDeferral};
use openvm_stark_backend::{
    codec::{Decode, Encode},
    keygen::types::MultiStarkVerifyingKey,
    p3_field::PrimeField32,
};
use openvm_stark_sdk::config::{
    baby_bear_poseidon2::{BabyBearPoseidon2Config, Digest},
    hook_params_with_100_bits_security,
};
use openvm_verify_stark_circuit::extension::get_raw_deferral_results;
use openvm_verify_stark_host::{
    VmStarkProof,
    pvs::{VM_PVS_AIR_ID, VmPvs},
    vk::{VerificationBaseline, VmStarkVerifyingKey},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    AxiomSdk, authenticated_get, config::ConfigSdk, download_file, get_axiom_dir, get_config_id,
    looks_like_json,
};

/// Number of bytes in a serialized `CommitBytes` (BabyBear digest).
const COMMIT_NUM_BYTES: usize = 32;

/// Stack size for the derivation thread. OpenVM keygen-style constructions
/// (deep recursive circuit builders) overflow the default thread stack.
const DERIVATION_STACK_BYTES: usize = 64 * 1024 * 1024;

pub trait DeferralSdk {
    /// Derive the parent-job API inputs for a `verify_stark` deferral job from
    /// a child stark proof. Returns the child's `input_commit` as `0x`-hex.
    fn prepare_deferral(&self, args: &DeferralPrepareArgs) -> Result<String>;
}

#[derive(Debug)]
pub struct DeferralPrepareArgs {
    /// The child stark proof: either the JSON served by
    /// `GET /proofs/{id}/proof/stark` or its openvm-codec binary encoding.
    pub child_proof: PathBuf,
    /// The config ID (defaults from `~/.axiom/config.json`).
    pub config_id: Option<String>,
    /// The PARENT program ID (the `verify_stark` guest that will be proved).
    pub program_id: String,
    /// Output path: the child proof as openvm-codec bytes, ready to pass to
    /// `cargo axiom prove --deferred-proof`.
    pub out_child_bin: PathBuf,
    /// Output path: the parent submission's JSON input body.
    pub out_input_json: PathBuf,
}

/// The `openvm.toml` schema the backend's keygen serves from
/// `GET /configs/{id}/config`: an `AppConfig<SdkVmConfig>` (flattened) plus an
/// optional `agg_config` section defaulting to the standard aggregation params.
#[derive(Debug, Clone, Deserialize)]
struct OpenvmConfig {
    #[serde(flatten)]
    app_config: AppConfig<SdkVmConfig>,
    #[serde(default = "default_agg_config")]
    agg_config: AggregationConfig,
}

fn default_agg_config() -> AggregationConfig {
    AggregationConfig {
        params: AggregationSystemParams::default(),
    }
}

/// Deferral commits derived from a config toml, cached on disk because the
/// `DeferralAggProver` construction costs several CPU-minutes. Both fields are
/// bare hex (64 chars, no `0x`) of the 32-byte canonical commit encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedDeferralCommits {
    /// The keyset's deferral hook commit (`baseline.expected_def_hook_commit`
    /// override for the child vk).
    pub def_hook_commit: String,
    /// The verify-stark deferral circuit's cached commit
    /// (`deferral_circuit_cached_commits[0]` in keygen terms) — folded into
    /// each child's `input_commit`.
    pub circuit_cached_commit: String,
}

impl CachedDeferralCommits {
    fn def_hook_commit_digest(&self) -> Result<Digest> {
        Ok(decode_bare_hex_commit(&self.def_hook_commit)?.into())
    }

    fn circuit_cached_commit_digest(&self) -> Result<Digest> {
        Ok(decode_bare_hex_commit(&self.circuit_cached_commit)?.into())
    }
}

/// Cache file path for the deferral commits derived from this exact toml:
/// `~/.axiom/cache/deferral-commits-<sha256 of the toml bytes>.json`. Keyed on
/// the raw bytes so any config change (even formatting) re-derives.
pub fn deferral_commits_cache_path(toml_bytes: &[u8]) -> Result<PathBuf> {
    let digest = Sha256::digest(toml_bytes);
    Ok(get_axiom_dir()?
        .join("cache")
        .join(format!("deferral-commits-{}.json", hex::encode(digest))))
}

fn load_cached_commits(path: &std::path::Path) -> Option<CachedDeferralCommits> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn store_cached_commits(path: &std::path::Path, commits: &CachedDeferralCommits) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache dir {}", parent.display()))?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(commits)?)
        .with_context(|| format!("failed to write commit cache {}", path.display()))
}

/// Decode a bare-hex 32-byte commit (no `0x`), the encoding both the cache
/// file and backend keygen's commit artifacts use.
fn decode_bare_hex_commit(hex_str: &str) -> Result<CommitBytes> {
    let bytes = hex::decode(hex_str.trim()).context("invalid commit hex")?;
    let arr: [u8; COMMIT_NUM_BYTES] = bytes.as_slice().try_into().map_err(|_| {
        eyre!(
            "commit must be {COMMIT_NUM_BYTES} bytes, got {}",
            bytes.len()
        )
    })?;
    Ok(CommitBytes::new(arr))
}

fn commit_to_bare_hex(commit: &CommitBytes) -> String {
    hex::encode(commit.as_slice())
}

fn parse_openvm_config(toml_str: &str) -> Result<OpenvmConfig> {
    toml::from_str(toml_str).context("failed to parse the config's openvm.toml")
}

/// Extract the verify-stark deferral circuit commit from the config toml's
/// `[[app_vm_config.deferral.circuits]]` section. Keygen writes this section
/// (with the keyset-derived commit) into the complete `openvm.toml` it serves;
/// a toml without it predates deferral support.
fn deferral_circuit_commit(config: &OpenvmConfig, config_id: &str) -> Result<CommitBytes> {
    let deferral = config
        .app_config
        .app_vm_config
        .deferral
        .as_ref()
        .ok_or_else(|| missing_deferral_section_error(config_id))?;
    let circuit = deferral
        .circuits
        .first()
        .ok_or_else(|| missing_deferral_section_error(config_id))?;
    Ok(circuit.commit)
}

fn missing_deferral_section_error(config_id: &str) -> eyre::Report {
    eyre!(
        "config {config_id} was keygen'd before deferral support (its openvm.toml has no \
         [[app_vm_config.deferral.circuits]] section); rerun keygen for this config"
    )
}

/// All deferral commits derivable from a config toml.
pub struct DerivedDeferralCommits {
    pub def_hook_commit: CommitBytes,
    /// `deferral_circuit_cached_commits[0]`: the verify-stark circuit prover's
    /// cached commit, folded into each child's `input_commit`.
    ///
    /// NOTE: this is NOT the toml's deferral circuit commit — empirically
    /// verified against the standard-config keyset (see the ignored
    /// `derive_matches_standard_config_keyset_constants` test), the two values
    /// differ, so the cached commit cannot be read off the toml and must be
    /// derived. It falls out of the same `DeferralAggProver` construction the
    /// hook commit needs, so this costs nothing extra.
    pub circuit_cached_commit: CommitBytes,
    /// The deferral circuit commit (`make_config`), i.e. the value keygen bakes
    /// into the served toml's `[[app_vm_config.deferral.circuits]]` section.
    /// Derived alongside the others (no extra cost) as a keyset cross-check.
    pub circuit_commit: CommitBytes,
}

/// Derive the deferral commits from the config toml exactly as backend keygen
/// does (`keygen_lib::build_deferral_sdk`): construct
/// `DeferralAggProver::verify_stark` from the toml's aggregation params, the
/// 100-bit-security hook params, and the app system config's memory dimensions
/// and public-value count. Costs ~12s in a release build on a many-core
/// machine, but minutes in dev builds or on small machines; results are cached
/// by the caller. Runs on a large-stack thread (recursive circuit construction
/// overflows default stacks).
pub fn derive_deferral_commits(toml_str: &str) -> Result<DerivedDeferralCommits> {
    let toml_str = toml_str.to_string();
    std::thread::Builder::new()
        .stack_size(DERIVATION_STACK_BYTES)
        .spawn(move || -> Result<DerivedDeferralCommits> {
            let config = parse_openvm_config(&toml_str)?;
            let agg_params = config.agg_config.params.clone();
            let (memory_dimensions, num_user_pvs) = {
                let system_config = &config.app_config.app_vm_config.system.config;
                (
                    system_config.memory_config.memory_dimensions(),
                    system_config.num_public_values,
                )
            };

            let deferral_agg_prover = DeferralAggProver::verify_stark(
                &agg_params,
                hook_params_with_100_bits_security(),
                memory_dimensions,
                num_user_pvs,
            );

            let def_hook_commit: CommitBytes = deferral_agg_prover.def_hook_commit().into();

            // The deferral circuit set is exactly [VerifyStark]; index 0 is the
            // sole circuit. Mirrors GenericSdk::deferral_circuit_cached_commits(0).
            let circuit_cached_commit = deferral_agg_prover
                .multi_deferral_circuit_prover
                .single_circuit_provers
                .first()
                .ok_or_else(|| eyre!("deferral prover has no circuits"))?
                .def_circuit_prover
                .cached_commits()
                .first()
                .copied()
                .ok_or_else(|| eyre!("verify-stark circuit exposes no cached commits"))?;

            // Same derivation keygen uses for the served toml's circuit commit.
            let circuit_commit = deferral_agg_prover
                .multi_deferral_circuit_prover
                .make_config(vec![SupportedDeferral::VerifyStark])
                .circuits
                .first()
                .ok_or_else(|| eyre!("make_config returned no circuits"))?
                .commit;

            Ok(DerivedDeferralCommits {
                def_hook_commit,
                circuit_cached_commit,
                circuit_commit,
            })
        })
        .context("failed to spawn deferral derivation thread")?
        .join()
        .map_err(|_| eyre!("deferral derivation thread panicked"))?
}

/// Parse a child stark proof from either encoding: the JSON artifact served by
/// proof download, or the openvm-codec binary (`encode_to_vec`). A codec
/// stream's leading bytes are a version-string length prefix and can never be
/// `{`, so the JSON sniff is unambiguous.
pub fn parse_child_proof(bytes: &[u8]) -> Result<VersionedVmStarkProof> {
    if looks_like_json(bytes) {
        serde_json::from_slice(bytes)
            .context("failed to parse child proof as VersionedVmStarkProof JSON")
    } else {
        VersionedVmStarkProof::decode(&mut &bytes[..])
            .map_err(|e| eyre!("failed to decode child proof as openvm-codec binary: {e}"))
    }
}

/// Compute a child program's `app_exe_commit` from its stark proof's VM public
/// values — the same computation openvm's `verify_vm_stark_proof_pvs` performs.
/// This is the one program-specific field of the child's
/// [`VerificationBaseline`].
fn child_app_exe_commit(proof: &VmStarkProof) -> Result<Digest> {
    let vm_pvs_air = proof
        .inner
        .public_values
        .get(VM_PVS_AIR_ID)
        .ok_or_else(|| {
            eyre!(
                "child stark proof has only {} public-value AIR(s) but VmPvs lives at \
                 VM_PVS_AIR_ID={VM_PVS_AIR_ID}; the child proof is malformed or from a \
                 different keyset",
                proof.inner.public_values.len(),
            )
        })?;
    let vm_pvs: &VmPvs<F> = vm_pvs_air.as_slice().borrow();
    Ok(compute_exe_commit(
        &vm_poseidon2_hasher(),
        &vm_pvs.program_commit,
        &vm_pvs.initial_root,
        vm_pvs.initial_pc,
    ))
}

/// Build the child [`VmStarkVerifyingKey`]: the config's `agg_vk` as `mvk`, the
/// parent program's config-level `baseline` with two child/deferral overrides
/// applied ([`child_baseline`]).
fn build_child_vk(
    mvk: MultiStarkVerifyingKey<BabyBearPoseidon2Config>,
    baseline: VerificationBaseline,
    child_app_exe_commit: Digest,
    expected_def_hook_commit: Digest,
) -> VmStarkVerifyingKey {
    VmStarkVerifyingKey {
        mvk,
        baseline: child_baseline(baseline, child_app_exe_commit, expected_def_hook_commit),
    }
}

/// Apply the child/deferral overrides to a config-level `baseline` template:
/// the child's `app_exe_commit`, and the deferral-aware
/// `expected_def_hook_commit` (the on-disk `baseline.json` always carries
/// `None`, but every child proof on a deferral config carries deferral public
/// values, so the child vk MUST be deferral-aware).
fn child_baseline(
    mut baseline: VerificationBaseline,
    child_app_exe_commit: Digest,
    expected_def_hook_commit: Digest,
) -> VerificationBaseline {
    baseline.app_exe_commit = child_app_exe_commit;
    baseline.expected_def_hook_commit = Some(expected_def_hook_commit);
    baseline
}

/// Convert a built [`StdIn`] into the API's `0x01`-prefixed input entries, one
/// entry per stdin buffer item.
///
/// Round-trip argument: `StdIn::write(&T)` serializes `T` into u32 words
/// (openvm serde), splits them into LE bytes, and stores ONE FIELD ELEMENT PER
/// BYTE (`write_bytes`). The API's `0x01` decoding is exactly `write_bytes` on
/// the raw bytes, so hex-encoding each buffer item's per-byte field elements
/// reproduces the identical stdin worker-side.
fn stdin_to_input_entries(stdin: &StdIn) -> Result<Vec<String>> {
    stdin
        .buffer
        .iter()
        .map(|fes| {
            let bytes = fes
                .iter()
                .map(|f| {
                    let v = f.as_canonical_u32();
                    u8::try_from(v).map_err(|_| {
                        eyre!(
                            "stdin field element {v} does not fit in a byte; not a write_bytes \
                             stream"
                        )
                    })
                })
                .collect::<Result<Vec<u8>>>()?;
            Ok(format!("0x01{}", hex::encode(bytes)))
        })
        .collect()
}

/// Build the parent guest's stdin. The `verify_stark` parent guest reads
/// exactly one value: `let input_commit: Commit = read();` (`Commit = [u8; 32]`).
fn parent_stdin(input_commit: &[u8; COMMIT_NUM_BYTES]) -> StdIn {
    let mut stdin = StdIn::default();
    stdin.write(input_commit);
    stdin
}

/// Parent API input body: same schema as a plain proof submission JSON body.
#[derive(Debug, Serialize)]
struct InputBody {
    input: Vec<String>,
}

impl DeferralSdk for AxiomSdk {
    fn prepare_deferral(&self, args: &DeferralPrepareArgs) -> Result<String> {
        let config_id = get_config_id(args.config_id.as_deref(), &self.config)?;

        // --- Download everything through the public API surface. ---

        // The config's complete openvm.toml (carries the deferral circuit
        // commit) — also the cache key for the derived commits.
        let toml_bytes = self.download_config(Some(&config_id), None)?;
        let toml_str = std::str::from_utf8(&toml_bytes)
            .context("config openvm.toml is not valid UTF-8")?
            .to_string();
        let openvm_config = parse_openvm_config(&toml_str)?;
        let toml_commit = deferral_circuit_commit(&openvm_config, &config_id)?;

        // The config's aggregation verifying key (the child proof's mvk).
        let tmp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let agg_vk_path = tmp_dir.path().join("agg_vk");
        let downloader = self.get_proving_keys(Some(&config_id), "agg_vk")?;
        downloader
            .download_pk_with_callback(&agg_vk_path.to_string_lossy(), &*self.callback)
            .context("failed to download the config's agg verifying key")?;
        let mvk: MultiStarkVerifyingKey<BabyBearPoseidon2Config> =
            read_object_from_file(&agg_vk_path).context("failed to decode agg verifying key")?;

        // The PARENT program's baseline.json (config-level baseline template).
        self.callback.on_info(&format!(
            "Downloading baseline for parent program {}",
            args.program_id
        ));
        let baseline_url = format!(
            "{}/programs/{}/download/baseline",
            self.config.api_url, args.program_id
        );
        let baseline_bytes = download_file(
            authenticated_get(&self.config, &baseline_url)?,
            None,
            "Failed to download the parent program's baseline",
        )?;
        let baseline: VerificationBaseline =
            serde_json::from_slice::<VerificationBaselineJson>(&baseline_bytes)
                .context("failed to parse the parent program's baseline.json")?
                .into();

        // --- Child proof: parse (JSON or codec), re-encode to codec bytes. ---
        let child_bytes = std::fs::read(&args.child_proof)
            .with_context(|| format!("failed to read {}", args.child_proof.display()))?;
        let versioned = parse_child_proof(&child_bytes)?;
        let codec_bytes = versioned
            .encode_to_vec()
            .map_err(|e| eyre!("failed to codec-encode child proof: {e}"))?;
        if let Some(parent) = args.out_child_bin.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&args.out_child_bin, &codec_bytes)
            .with_context(|| format!("failed to write {}", args.out_child_bin.display()))?;
        let proof = VmStarkProof::try_from(versioned)
            .map_err(|e| eyre!("failed to decode child VmStarkProof: {e}"))?;

        // --- Deferral commits: cache hit or expensive local derivation. ---
        let cache_path = deferral_commits_cache_path(&toml_bytes)?;
        let cached = match load_cached_commits(&cache_path) {
            Some(cached) => cached,
            None => {
                self.callback.on_info(&format!(
                    "first run for config {config_id}: deriving deferral commits (may take \
                     several minutes)..."
                ));
                let derived = derive_deferral_commits(&toml_str)?;
                // Keyset cross-check: the derived circuit commit must equal the
                // commit keygen baked into the served toml. A mismatch means
                // this CLI's openvm pin diverged from the config's keyset.
                if derived.circuit_commit != toml_commit {
                    return Err(eyre!(
                        "derived deferral circuit commit 0x{} does not match the config's \
                         openvm.toml commit 0x{}; the CLI's pinned openvm version does not \
                         match config {config_id}'s keyset",
                        commit_to_bare_hex(&derived.circuit_commit),
                        commit_to_bare_hex(&toml_commit),
                    ));
                }
                let cached = CachedDeferralCommits {
                    def_hook_commit: commit_to_bare_hex(&derived.def_hook_commit),
                    circuit_cached_commit: commit_to_bare_hex(&derived.circuit_cached_commit),
                };
                store_cached_commits(&cache_path, &cached)?;
                self.callback.on_info(&format!(
                    "deferral commits cached at {}",
                    cache_path.display()
                ));
                cached
            }
        };
        let def_hook_commit: Digest = cached.def_hook_commit_digest()?;
        let circuit_cached_commit: Digest = cached.circuit_cached_commit_digest()?;

        // --- Derive input_commit exactly as the proving worker does. ---
        // get_raw_deferral_results verifies the child proof, so a wrong-keyset
        // child fails HERE, before any job is submitted.
        let exe_commit = child_app_exe_commit(&proof)?;
        let child_vk = build_child_vk(mvk, baseline, exe_commit, def_hook_commit);
        let raw = get_raw_deferral_results(&child_vk, from_ref(&proof), circuit_cached_commit)
            .map_err(|e| eyre!("get_raw_deferral_results failed: {e}"))?;
        let input_commit: [u8; COMMIT_NUM_BYTES] = raw[0]
            .input
            .clone()
            .try_into()
            .map_err(|v: Vec<u8>| eyre!("input_commit must be 32 bytes, got {}", v.len()))?;

        // --- Parent input body. ---
        let entries = stdin_to_input_entries(&parent_stdin(&input_commit))?;
        let body = InputBody { input: entries };
        if let Some(parent) = args.out_input_json.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&args.out_input_json, serde_json::to_vec_pretty(&body)?)
            .with_context(|| format!("failed to write {}", args.out_input_json.display()))?;

        let input_commit_hex = format!("0x{}", hex::encode(input_commit));
        self.callback.on_field("input_commit", &input_commit_hex);
        self.callback.on_success(&format!(
            "wrote {} and {}",
            args.out_child_bin.display(),
            args.out_input_json.display()
        ));
        Ok(input_commit_hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the exact byte encoding `StdIn::write(&[u8; 32])` produces, i.e.
    /// what the parent's `0x01` input entry must carry. openvm serde
    /// serializes each `u8` of a fixed-size array as its own u32 word, and
    /// `StdIn::write` flattens words to LE bytes — so a 32-byte commit becomes
    /// 128 bytes: each commit byte followed by three zero bytes. NOT the 32
    /// raw bytes. (Mirrors the backend's internal deferral_prepare pin test.)
    #[test]
    fn stdin_write_commit_encoding_is_one_word_per_byte() {
        let commit: [u8; COMMIT_NUM_BYTES] = std::array::from_fn(|i| (i as u8) + 1);
        let stdin = parent_stdin(&commit);
        assert_eq!(stdin.buffer.len(), 1, "one write => one stream entry");

        let expected: Vec<u8> = commit.iter().flat_map(|&b| [b, 0, 0, 0]).collect();
        let entries = stdin_to_input_entries(&stdin).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], format!("0x01{}", hex::encode(&expected)));
        assert_eq!(entries[0].len(), 2 + 2 + 2 * 128, "0x + 01 + 128 bytes hex");
    }

    /// The input body round-trips through the API's own parsing rules: JSON
    /// object with an "input" list of `0x01`-prefixed hex strings whose byte
    /// decoding reproduces the stdin.
    #[test]
    fn input_body_round_trips_through_api_input_decoding() {
        let commit: [u8; COMMIT_NUM_BYTES] = std::array::from_fn(|i| 255 - i as u8);
        let original = parent_stdin(&commit);
        let body = InputBody {
            input: stdin_to_input_entries(&original).unwrap(),
        };
        let json = serde_json::to_string(&body).unwrap();

        // Re-parse as the API/worker would: strip 0x01, hex-decode, write_bytes.
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut reconstructed = StdIn::default();
        for entry in parsed["input"].as_array().unwrap() {
            let s = entry.as_str().unwrap();
            assert!(s.starts_with("0x01"), "byte-stream entries only");
            let bytes = hex::decode(&s[4..]).unwrap();
            reconstructed.write_bytes(&bytes);
        }

        let flatten = |s: &StdIn| -> Vec<u32> {
            s.buffer
                .iter()
                .flat_map(|fes| fes.iter().map(F::as_canonical_u32))
                .collect()
        };
        assert_eq!(flatten(&original), flatten(&reconstructed));
    }

    /// The commit cache round-trips: what `store_cached_commits` writes,
    /// `load_cached_commits` reads back identically, and the hex fields decode
    /// to the original digests. Also pins that the cache path is keyed on the
    /// toml BYTES (different bytes => different path).
    #[test]
    fn commit_cache_round_trips() {
        let commit = CommitBytes::from([1u32, 2, 3, 4, 5, 6, 7, 8]);
        let cached = CachedDeferralCommits {
            def_hook_commit: commit_to_bare_hex(&commit),
            circuit_cached_commit: commit_to_bare_hex(&commit),
        };

        let dir = tempfile::tempdir().unwrap();
        // Exercise the create_dir_all path with a nested location.
        let path = dir.path().join("cache").join("deferral-commits-test.json");
        store_cached_commits(&path, &cached).unwrap();
        let loaded = load_cached_commits(&path).unwrap();
        assert_eq!(loaded, cached);

        let digest: Digest = loaded.def_hook_commit_digest().unwrap();
        assert_eq!(CommitBytes::from(digest), commit);

        // Missing file and corrupt file are both cache misses, not errors.
        assert!(load_cached_commits(&dir.path().join("nope.json")).is_none());
        std::fs::write(dir.path().join("bad.json"), b"not json").unwrap();
        assert!(load_cached_commits(&dir.path().join("bad.json")).is_none());

        // Path is keyed on the toml bytes.
        let p1 = deferral_commits_cache_path(b"toml-a").unwrap();
        let p2 = deferral_commits_cache_path(b"toml-b").unwrap();
        assert_ne!(p1, p2);
        assert_eq!(p1, deferral_commits_cache_path(b"toml-a").unwrap());
        assert!(
            p1.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("deferral-commits-")
        );
    }

    /// A keygen-served toml (deferral section present) parses and exposes the
    /// circuit commit; a pre-deferral toml errors with the rerun-keygen
    /// message.
    #[test]
    fn toml_deferral_section_parse_and_clear_error_when_absent() {
        let with_deferral = r#"
[app_vm_config.rv32i]
[app_vm_config.rv32m]
[app_vm_config.io]

[[app_vm_config.deferral.circuits]]
def_type = "VerifyStark"
commit = "0x0028f0c4c21eb53a99c7480ecc941110425c5dea91fb74de6d458b29492ebaf7"
"#;
        let config = parse_openvm_config(with_deferral).unwrap();
        let commit = deferral_circuit_commit(&config, "cfg_test").unwrap();
        assert_eq!(
            commit_to_bare_hex(&commit),
            "0028f0c4c21eb53a99c7480ecc941110425c5dea91fb74de6d458b29492ebaf7"
        );

        let without_deferral = r#"
[app_vm_config.rv32i]
[app_vm_config.rv32m]
[app_vm_config.io]
"#;
        let config = parse_openvm_config(without_deferral).unwrap();
        let err = deferral_circuit_commit(&config, "cfg_test").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("keygen'd before deferral support") && msg.contains("rerun keygen"),
            "error must tell the user to rerun keygen, got: {msg}"
        );
        assert!(msg.contains("cfg_test"), "error names the config: {msg}");
    }

    /// The child-proof sniffing accepts BOTH encodings the docs promise: the
    /// downloaded JSON artifact and the openvm-codec binary. Inner proof bytes
    /// are opaque at this level, so a dummy envelope is used; equality of the
    /// re-parsed envelope proves the decode path.
    #[test]
    fn parse_child_proof_accepts_json_and_codec() {
        let versioned = VersionedVmStarkProof {
            version: "v9.9-test".to_string(),
            proof: vec![1, 2, 3],
            user_pvs_proof: vec![4, 5],
            deferral_merkle_proofs: None,
        };

        let json_bytes = serde_json::to_vec(&versioned).unwrap();
        let from_json = parse_child_proof(&json_bytes).unwrap();
        assert_eq!(from_json.version, versioned.version);
        assert_eq!(from_json.proof, versioned.proof);

        let codec_bytes = versioned.encode_to_vec().unwrap();
        let from_codec = parse_child_proof(&codec_bytes).unwrap();
        assert_eq!(from_codec.version, versioned.version);
        assert_eq!(from_codec.proof, versioned.proof);

        // Garbage that is neither fails with the codec error (not a panic).
        assert!(parse_child_proof(&[0xff, 0xfe, 0xfd]).is_err());
    }

    /// One-time probe against the standard config's known keyset constants
    /// (openvm v2.0.0, default aggregation params, 100-bit hook params,
    /// default memory dimensions and public values — what
    /// openvm_standard.toml yields). Verifies:
    /// - the derived `def_hook_commit` matches the backend keyset's artifact
    ///   (the constant guarded by the backend's deferral e2e test);
    /// - the derived circuit commit matches the commit keygen bakes into the
    ///   served openvm.toml;
    /// - EMPIRICAL ANSWER (run 2026-07-27, openvm v2.0.0):
    ///   `deferral_circuit_cached_commits[0]` does NOT equal the toml's
    ///   circuit commit — the cached commit is the circuit's PCS trace
    ///   commitment while the toml commit hashes six vk-commit components —
    ///   so the cached commit must always be derived; there is no toml
    ///   shortcut.
    ///
    /// Ignored by default: the DeferralAggProver construction is full circuit
    /// keygen (~12s release on a many-core machine, minutes otherwise). Run:
    ///   cargo test -p axiom-sdk --release derive_matches_standard_config -- --ignored --nocapture
    #[test]
    #[ignore = "expensive DeferralAggProver construction"]
    fn derive_matches_standard_config_keyset_constants() {
        const EXPECTED_DEF_HOOK_COMMIT: &str =
            "003d2f6e11db9ed346a6b595d4c8e358f7e434b4bd6fa378741898d861209fb4";
        const EXPECTED_CIRCUIT_COMMIT: &str =
            "0028f0c4c21eb53a99c7480ecc941110425c5dea91fb74de6d458b29492ebaf7";
        // Observed on the first probe run; pinned so keyset drift is loud.
        const EXPECTED_CIRCUIT_CACHED_COMMIT: &str =
            "001d294a5c49bd8a5a1ec12238795ea7c6d25326ab44d6aa493b4b73742e8dff";

        let toml_str = include_str!("../test-fixtures/openvm_standard.toml");
        let derived = derive_deferral_commits(toml_str).unwrap();

        let hook_hex = commit_to_bare_hex(&derived.def_hook_commit);
        let cached_hex = commit_to_bare_hex(&derived.circuit_cached_commit);
        let circuit_hex = commit_to_bare_hex(&derived.circuit_commit);
        println!("def_hook_commit:        {hook_hex}");
        println!("circuit_cached_commit:  {cached_hex}");
        println!("circuit_commit (toml):  {circuit_hex}");
        println!(
            "cached_commit == toml circuit commit? {}",
            cached_hex == circuit_hex
        );

        assert_eq!(
            hook_hex, EXPECTED_DEF_HOOK_COMMIT,
            "def_hook_commit drifted"
        );
        assert_eq!(
            circuit_hex, EXPECTED_CIRCUIT_COMMIT,
            "deferral circuit commit drifted"
        );
        assert_eq!(
            cached_hex, EXPECTED_CIRCUIT_CACHED_COMMIT,
            "verify-stark circuit cached commit drifted"
        );
        assert_ne!(
            cached_hex, circuit_hex,
            "cached commit and toml circuit commit are distinct values; if this ever \
             starts failing, revisit DerivedDeferralCommits' docs"
        );
    }
}
