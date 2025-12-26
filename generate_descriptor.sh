#! /bin/bash
set -euo pipefail

# Determine bazelisk binary for current platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH="amd64" ;;
    aarch64) ARCH="arm64" ;;
esac
BAZELISK="./tools/bazelisk-${OS}-${ARCH}"

if [ ! -x "$BAZELISK" ]; then
    echo "Error: Bazelisk not found at $BAZELISK"
    exit 1
fi

# Check if we should use protocrap_stable (for table layout changes)
CARGO_FLAGS=""
if [ "${1:-}" = "bootcrap" ]; then
    echo "Using protocrap_stable for bootstrap"
    CARGO_FLAGS="--no-default-features --features bootcrap"
else
    echo "Using current protocrap"
fi

# Build the codegen tool first (prevents rebuild during run)
cargo build -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS

# Build descriptor set via Bazel
echo "Building descriptor.proto via Bazel..."
$BAZELISK build //:descriptor_set

# Run codegen on the generated descriptor set
RUST_BACKTRACE=full cargo run -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS -- bazel-bin/descriptor.bin src/descriptor.pc.rs
