# Protocrap TODO List

## High Priority

### Code Quality & Completeness
- [ ] **Enum default values** (`codegen/src/generator.rs:428`)
  - Implement support for enum default values in code generation
  - Currently returns `None` for enum defaults

- [ ] **Better error handling** (`src/containers.rs:72`)
  - Replace panic on allocation failure with proper error propagation
  - Consider using Result types or custom error handling strategy

- [ ] **Serde null handling** (`src/serde.rs:412`)
  - Handle null values properly when deserializing message fields
  - Currently has TODO comment but may silently fail

### Code Generation Improvements
- [ ] **Name resolution hardcoding** (`codegen/src/static_gen.rs:13`)
  - Replace hardcoded name resolution in `full_name()` function
  - Should use proper scope/package resolution from descriptor
  - Currently has manual mapping for ExtensionRange, ReservedRange, etc.

- [ ] **Suppress false dead_code warnings in codegen**
  - Add `#![allow(dead_code)]` to codegen modules
  - Functions like `generate_file_set`, `generate_message`, etc. ARE used but Rust's analysis misses them
  - These are internal implementation details called through the public `generate()` API

### Performance
- [ ] **Optimize write_tag** (`src/wire.rs:241`)
  - Current implementation just calls write_varint
  - Could be optimized for common tag sizes

## Medium Priority

### Feature Completeness
- [ ] **Map support**
  - README states maps aren't supported (treated as repeated key/value pairs)
  - Decide: fully implement map syntax or document current limitation better
  - MessageOptions.map_entry field exists in descriptor but not used in codegen

- [ ] **Unknown fields handling**
  - Currently skipped during parsing (documented design decision)
  - Consider if there are use cases that need this

- [ ] **Extensions support**
  - Currently not supported (documented design decision)
  - Verify this is acceptable for target use cases

### Documentation
- [ ] **API documentation**
  - Add rustdoc comments to public APIs
  - Document design patterns (arena usage, push-based parsing, etc.)
  - Add examples to README

- [ ] **Usage guide**
  - Expand README with real-world usage examples
  - Document the reflection API and DescriptorPool
  - Add guide for async usage patterns

- [ ] **Migration guide**
  - Document differences from prost/protobuf
  - Explain when to use protocrap vs alternatives

## Low Priority

### Testing
- [ ] **Expand test coverage**
  - More edge cases in codegen-tests
  - Test with larger/more complex schemas
  - Async parsing tests

- [ ] **Benchmarks**
  - Complete benchmark suite comparing to prost
  - Document performance characteristics
  - Memory usage profiling

### Tooling
- [ ] **Better error messages**
  - Improve decode error reporting (currently just "decode error")
  - Add context about what failed and why

- [ ] **Cargo integration**
  - Consider build.rs helper for proto compilation
  - Publishing to crates.io checklist

## Completed ✓
- [x] Fix alignment bugs in has_bit operations (2024-12-22)
- [x] Fix decode table has_bit calculation (2024-12-22)
- [x] Support latest descriptor.proto (2024-12-22)
- [x] Bootstrap mode in generate_descriptor.sh (2024-12-22)

## Notes

### Design Decisions (Not TODOs)
These are intentional limitations per the design philosophy:
- No generic-heavy code (type erasure preferred)
- Max 64 optional fields per message
- Field numbers 1-2047 only
- No extensions or unknown field preservation
- Struct sizes up to 1KB

### Won't Fix
- Complex macro-based codegen (against design principles)
- Generic explosion for type safety (performance/compile-time trade-off)
