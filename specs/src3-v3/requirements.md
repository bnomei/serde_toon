# Requirements

## Scope
- In scope: structure-only scaffolding for v3 (src3): module tree, public API surface, option and error types, and test enablement protocol.
- Out of scope: any encoding/decoding behavior, fast-path or fallback logic, strict-mode rules, performance work, or data model implementation.
- Out of scope: importing, referencing, or mirroring code/tests from `src_v1` or `src_v2` during this phase.

## System name
The TOON v3 library (src3).

## Requirements (EARS)
- The TOON v3 library shall compile with a v3 module hierarchy rooted in `src/` and expose the public API surface for encoding and decoding entry points.
- The TOON v3 library shall define encoder and decoder option types with defaults to support compilation and future configuration.
- The TOON v3 library shall define a structured error type with a "not implemented" variant for stubbed entry points.
- When a v3 entry point is called during the structure-only phase, the TOON v3 library shall return a structured "not implemented" error instead of panicking.
- The TOON v3 library shall not depend on `src_v1`, `src_v2`, or their tests in this phase.
- While the test suite remains fully ignored, the project shall preserve all tests and enable them one by one as features are implemented.
