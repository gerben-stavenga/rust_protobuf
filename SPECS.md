# Protocrap Specification

## Wire Format Support

### Fully Supported

| Feature | Proto2 | Proto3 | Notes |
|---------|--------|--------|-------|
| Scalar types | ✓ | ✓ | int32, int64, uint32, uint64, sint32, sint64, fixed32, fixed64, sfixed32, sfixed64, float, double, bool |
| Strings | ✓ | ✓ | UTF-8 validated |
| Bytes | ✓ | ✓ | |
| Enums | ✓ | ✓ | Sign-extended as i32 |
| Nested messages | ✓ | ✓ | |
| Repeated fields | ✓ | ✓ | Both packed and unpacked |
| Optional fields | ✓ | ✓ | Has-bit tracking |
| Default values | ✓ | ✓ | Proto2 custom defaults supported |
| Field merging | ✓ | ✓ | Last value wins for scalars, merge for messages |
| Groups | ✓ | - | Proto2 only |

### Intentionally Unsupported

These features are **not supported by design** and will not be implemented:

| Feature | Behavior |
|---------|----------|
| Unknown fields | Silently discarded during decoding. Not preserved or round-tripped. |
| Extensions | Dropped (treated as unknown fields). |
| MessageSet encoding | Not supported. |

### Not Yet Implemented

| Feature | Status |
|---------|--------|
| Oneof | Fields not cleared when switching between oneof members |
| Maps | Parsed as repeated key-value pairs, full map semantics pending |

## Optionality Model

Protocrap uses **unified optionality**:

- All fields (proto2 and proto3) have has-bits
- Fields are serialized if their has-bit is set, even if the value is zero/default
- This differs from proto3 spec which omits default values

## Required Fields

Proto2 `required` fields are treated identically to `optional`:

- No presence validation on decode or encode
- No error if a required field is missing
- Reflection API could add validation if needed

This is by design - required fields are a proto2 mistake that proto3 removed.

## Restrictions

| Restriction | Limit |
|-------------|-------|
| Optional fields per message | 64 (has-bits stored in u64) |
| Struct size | 1KB max |
| Field numbers | 1..2047 (1-2 byte tags) |
| Field number gaps | Tolerated but create larger tables |

## JSON Format

JSON support is basic and lower priority than binary format.

### Supported
- Primitive types (numbers, strings, bools)
- Nested messages
- Repeated fields
- Duration and Timestamp (partial)

### Not Supported
- Enum string names (integers only)
- Any type
- FieldMask
- Struct/Value/ListValue
- Wrapper types
- Base64url for bytes (standard base64 only)
- Strict validation (overflow, format checks)

## Conformance Status

Against the official protobuf conformance test suite:

| Metric | Count |
|--------|-------|
| Binary successes | 2553 |
| Expected failures | 234 |
| Unexpected failures | 0 |
| Crashes | 0 |

### Binary Failure Breakdown

| Category | Count | Reason |
|----------|-------|--------|
| Unknown fields | 4 | By design |
| Oneof | 38 | Not implemented |
| Extensions/MessageSet | 5 | By design |
| Proto3 default omission | 18 | Unified optionality |

### JSON Failures

171 tests related to JSON edge cases, well-known types, and strict validation.

## Panic Safety

Protocrap never panics on malformed input. All errors are returned as `Result` types.
