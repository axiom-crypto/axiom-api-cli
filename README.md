# <img src="./favicon.ico" alt="Axiom Proving CLI" width="32" height="32" /> Axiom CLI Guide

## Setup

1. Install the Axiom CLI:

   ```
   cargo +1.86 install --locked --git https://github.com/axiom-crypto/axiom-api-cli.git --tag v1.0.5 cargo-axiom
   ```

   Or from source:

   ```bash
   git clone https://github.com/axiom-crypto/axiom-api-cli --tag v1.0.5
   cd axiom-api-cli/crates/cli
   cargo +1.86 install --locked --force --path .
   ```

2. Initialize with your API key:
   ```bash
   cargo axiom register --api-key <API_KEY>
   ```
   Alternatively, set the `AXIOM_API_KEY` environment variable in a `.env` file and then run `cargo axiom register` at the directory of the `.env` file.
   See `.env.example` for an example.

## Building Programs

1. Navigate to your program directory (containing a Rust workspace with an OpenVM guest program).

2. Build your program:

   ```bash
   cargo axiom build
   ```

   This uploads your code and triggers a reproducible build on Axiom's servers.

3. Check build status:
   ```bash
   cargo axiom build status --program-id <ID>
   ```

## Generating Proofs

1. Request a proof for your program:

   ```bash
   cargo axiom prove --program-id <ID> --input <INPUT>
   ```

2. Check proof generation status:

   ```bash
   cargo axiom prove status --proof-id <ID>
   ```

3. Download proof logs if needed:

   ```bash
   cargo axiom prove logs --proof-id <ID>
   ```

4. Download proof artifacts:
   ```bash
   cargo axiom prove download --proof-id <ID> --type evm
   ```

## Verifying Proofs

1. Verify a proof:

   ```bash
   cargo axiom verify --proof <PROOF_FILE>
   ```

2. Check verification status:
   ```bash
   cargo axiom verify status --verify-id <ID>
   ```

For more details, see the [Axiom API Documentation](https://proving-api-docs.axiom.xyz/api-reference/axiom-cli).
