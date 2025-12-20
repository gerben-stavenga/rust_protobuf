#! /bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Build the codegen tool, to prevent rebuild when descriptor set is piped
cargo build -p protocrap-codegen --bin protocrap-codegen
# Generate the descriptor set
protoc --include_imports --descriptor_set_out=/tmp/protocrap-descriptor-set.bin protocrap/proto/descriptor.proto
RUST_BACKTRACE=full cargo run -p protocrap-codegen --bin protocrap-codegen -- /tmp/protocrap-descriptor-set.bin protocrap/src/descriptor2.pc.rs
