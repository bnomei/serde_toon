# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### [0.1.2] - 2026-02-04
- Added small scalar encode caches for non-tabular strings and numbers.
- Added byte-offset, line, and column locations for decode/validation errors.
- Reject zero indentation for encoder and CLI.
- Clarified missing array payload decode error message.
- Removed the public tabular placeholder module.
- Added API smoke tests and parallel feature coverage.
- Routed value decoding/validation through the arena parser when path expansion is off.
- Streamed encoder output to writers without buffering the full document.
- Streamed reader decoding without buffering the full input.
- Added auto-detect heuristics for JSON vs TOON decoding.
- Added content-based CLI auto-detection for unknown extensions.
- Added reader vs string decode benchmark coverage.
- Buffered `from_reader` inputs to preserve arena decoding semantics.
- Added auto-detect tests for JSON arrays and quoted keys.
- Added streaming error-location coverage for decode failures.
- Clarified `toon!` macro usage in README.
- Streamed CLI decode input and documented non-strict tab handling.
- Value decoder now borrows line slices to avoid per-line allocation.

## [0.1.1] - 2026-01-20
- Added a value-only fast path for tabular decoding and routed `decode_to_value` through a direct Value decoder.
- Added a `toon` CLI crate with encode/decode auto-detection, stats reporting, and key-folding/expand-paths options.
- Added cross-crate benchmark harness plus TOON datasets for comparisons in `benchmarks/`.
- Updated workspace metadata (version bump, categories/keywords/homepage, CLI workspace member).
- Expanded README benchmark docs and clarified JSON round-trip examples.
