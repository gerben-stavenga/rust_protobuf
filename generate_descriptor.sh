#! /bin/bash
set -euo pipefail

# Determine bazelisk binary for current platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH="amd64" ;;
    aarch64) ARCH="arm64" ;;
esac
# Try to read the desired Bazelisk version from Rust code; fall back to a known default.
BAZELISK_VERSION=$(grep -m1 "BAZELISK_VERSION" codegen/src/lib.rs 2>/dev/null | sed -E "s/.*\"([0-9.]+)\".*/\1/")
BAZELISK_VERSION=${BAZELISK_VERSION:-1.25.0}

# Construct filename and cache path (use XDG cache or ~/.cache like Rust code uses)
# Cache directory: ${XDG_CACHE_HOME:-$HOME/.cache}/protocrap/bazelisk-<version>
XDG_CACHE_HOME=${XDG_CACHE_HOME:-$HOME/.cache}
CACHE_DIR="$XDG_CACHE_HOME/protocrap/bazelisk-${BAZELISK_VERSION}"
mkdir -p "$CACHE_DIR"

EXT=""
if [ "$(uname -s | tr '[:upper:]' '[:lower:]')" = "darwin" ]; then
    OS_NAME="darwin"
else
    OS_NAME="$OS"
fi
if [ "$(uname -s)" = "CYGWIN_NT-"* ] || [ "$(uname -s)" = "MINGW"* ] || [ "$(uname -s)" = "MSYS_NT-"* ]; then
    EXT=".exe"
fi

BAZELISK_FILENAME="bazelisk-${OS_NAME}-${ARCH}${EXT}"
BAZELISK="$CACHE_DIR/$BAZELISK_FILENAME"

if [ ! -x "$BAZELISK" ]; then
    echo "Bazelisk not found at $BAZELISK — downloading v${BAZELISK_VERSION} into cache..."
    URL="https://github.com/bazelbuild/bazelisk/releases/download/v${BAZELISK_VERSION}/${BAZELISK_FILENAME}"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$BAZELISK" "$URL" || { echo "Failed to download bazelisk from $URL"; exit 1; }
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$BAZELISK" "$URL" || { echo "Failed to download bazelisk from $URL"; exit 1; }
    else
        echo "Neither curl nor wget found; cannot download bazelisk" >&2
        exit 1
    fi
    chmod +x "$BAZELISK"
    echo "Downloaded bazelisk to $BAZELISK"
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
