# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### 2026-02-03
- Minor: Added byte-offset, line, and column locations for decode/validation errors.
- Minor: Reject zero indentation for encoder and CLI.
- Minor: Clarified missing array payload decode error message.
- Major: Removed the public tabular placeholder module.
- Minor: Added API smoke tests and parallel feature coverage.
- Minor: Routed value decoding/validation through the arena parser when path expansion is off.
- Minor: Streamed encoder output to writers without buffering the full document.
- Minor: Streamed reader decoding without buffering the full input.
- Minor: Added auto-detect heuristics for JSON vs TOON decoding.
- Minor: Added content-based CLI auto-detection for unknown extensions.
- Minor: Added auto-detect tests for JSON arrays and quoted keys.
- Minor: Clarified `toon!` macro usage in README.
- Minor: Streamed CLI decode input and documented non-strict tab handling.

## [0.1.1] - 2026-01-20
- Added a value-only fast path for tabular decoding and routed `decode_to_value` through a direct Value decoder.
- Added a `toon` CLI crate with encode/decode auto-detection, stats reporting, and key-folding/expand-paths options.
- Added cross-crate benchmark harness plus TOON datasets for comparisons in `benchmarks/`.
- Updated workspace metadata (version bump, categories/keywords/homepage, CLI workspace member).
- Expanded README benchmark docs and clarified JSON round-trip examples.
