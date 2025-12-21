#! /bin/bash
set -euo pipefail
# Build the codegen tool, to prevent rebuild when descriptor set is piped
cargo build -p protocrap-codegen --bin protocrap-codegen --features bootcrap
# Generate the descriptor set
protoc --include_imports --descriptor_set_out=/tmp/protocrap-descriptor-set.bin proto/descriptor.proto
RUST_BACKTRACE=full cargo run -p protocrap-codegen --bin protocrap-codegen --features bootcrap -- /tmp/protocrap-descriptor-set.bin src/descriptor2.pc.rs
