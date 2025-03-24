## Running fibonacci on cloud proving
1. make the fibonacci program, see
1. `cargo axiom init`. This will ask for an API key.
1. inside the fibonacci directory, run `cargo axiom build --config-id 62d8a37c-f3ed-42d0-8b4e-75b440ebb7ec`.
1. If the above succeeds, it will print out a program ID. Check the status by: `cargo axiom build status --program-id 61d20fa4-6ee4-4094-91d4-f17cda7a6047`.
1. To submit a proving request: `cargo axiom prove --program-id <program-id> --input XXX`.
1. Check status: `cargo axiom prove status --proof-id <proof-id>`
1. Download proof: `cargo axiom prove download --proof-id <proof-id> --type [stark|root|evm]`