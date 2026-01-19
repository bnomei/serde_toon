# Design

## Overview
This phase is structure-only. It sets up the module boundaries and public API signatures for v3 without implementing behavior. No code or tests from `src_v1` or `src_v2` are referenced during this phase.

## Module hierarchy (structure-only)
The structure below defines file locations and responsibilities as placeholders. Each module compiles with minimal types or stubs and no functional logic.

- `src/lib.rs`: public API surface, re-exports, and stub entry points.
- `src/options.rs`: encoder/decoder option types with defaults.
- `src/error.rs`: shared error types and a "not implemented" variant.
- `src/encode/mod.rs`: encoder module placeholder; owns encode stubs.
- `src/decode/mod.rs`: decoder module placeholder; owns decode stubs.
- `src/arena/mod.rs`: placeholder for future arena storage and ids.
- `src/tabular/mod.rs`: placeholder for future tabular helpers.
- `src/text/string.rs`: placeholder for future string analysis helpers.
- `src/num/number.rs`: placeholder for future number helpers.

## Public API surface (stubs)
Expose the v3 entry points and option types with signatures only:
- `to_string`, `to_vec`, `to_writer`
- `from_str`, `from_slice`, `from_reader`
- `EncodeOptions`, `DecodeOptions`

Each entry point returns a structured "not implemented" error until its behavior is implemented.

## Test enablement protocol (TDD)
- The test suite is currently 100% ignored.
- Tests remain ignored during the structure-only phase.
- As features are implemented later, enable one test at a time and drive changes from that test.
