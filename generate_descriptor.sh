#! /bin/bash
set -euo pipefail

# Check if we should use protocrap_stable (for table layout changes)
CARGO_FLAGS=""
if [ "${1:-}" = "bootcrap" ]; then
    echo "🔧 Using protocrap_stable for bootstrap"
    CARGO_FLAGS="--no-default-features --features bootcrap"
else
    echo "🔧 Using current protocrap"
fi

# Build the codegen tool, to prevent rebuild when descriptor set is piped
cargo build -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS

# Generate the descriptor set
protoc --include_imports --descriptor_set_out=/tmp/protocrap-descriptor-set.bin proto/descriptor.proto

# Run codegen
RUST_BACKTRACE=full cargo run -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS -- /tmp/protocrap-descriptor-set.bin src/descriptor.pc.rs
