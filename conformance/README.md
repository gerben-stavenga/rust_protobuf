# Protobuf Conformance Tests

This directory contains the conformance test setup for protocrap using Google's official protobuf conformance test suite.

## Setup

1. Build the conformance test binary:
```bash
cargo build -p protocrap-conformance
```

## Running Tests

Run all conformance tests:
```bash
bazelisk run @protobuf//conformance:conformance_test_runner -- --enforce_recommended ../target/debug/conformance-protocrap
```

Run with specific test suite:
```bash
bazelisk run @protobuf//conformance:conformance_test_runner -- --failure_list failure_list.txt ../target/debug/conformance-protocrap
```

## Protocol

The conformance test protocol works as follows:

1. Test runner writes to stdin: `[u32 length][ConformanceRequest bytes]`
2. Our binary reads request, executes test, writes to stdout: `[u32 length][ConformanceResponse bytes]`
3. Test runner validates response
4. Repeat for all test cases

## Files

- `conformance.proto` - Defines the test protocol messages
- `test_messages_protoX.proto` - Defines test message schemas
- `build.rs` - Generates Rust code from proto files
- `src/main.rs` - Conformance test binary implementation
