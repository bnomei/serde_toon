# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-28

This breaking minor release upgrades the library and CLI from TOON 3.0 to the
TOON 4.1 Working Draft, including the intervening 3.1–4.0 changes. Both packages
are versioned `0.2.0`. The specification remains a Working Draft; this release
targets [spec revision d6db4b0](https://github.com/toon-format/spec/commit/d6db4b04303bdea132351ce45aed612311c850b2).

### Added

- Recursive nested field groups in tabular headers and keyed tabular objects.
- Decode-side full-line `#` comments and removal of a leading byte-order mark. Inline/trailing comments remain unsupported; encoders do not emit comments.
- Unicode control-character escaping and validation without Unicode normalization.
- The upstream 4.1 conformance suite: 179 encode and 359 decode fixtures, plus regressions from the specification and Oracle reviews.

### Changed

- Encoders now select tabular and keyed tabular forms wherever required, emit canonical empty arrays (`[]` and `key: []`), place a list-item object's first field on the hyphen line, and declare the document delimiter in every header.
- Decoders use the exact TOON number grammar and trim only U+0020 spaces from tokens. Host-value normalization, numeric range/rounding, and whitespace policies are documented in the README.
- Strict decoding rejects duplicate sibling keys and indentation depth jumps. Non-strict duplicate keys use last-write-wins semantics, and declared counts never truncate payloads.
- The CLI shares the library's lexical and indentation rules instead of preprocessing tabs. `--indentSize` is the documented indentation option.
- `validate_str` accepts valid root strings, exponent spellings, and final newlines; it still additionally rejects trailing spaces on non-comment lines.

### Removed

- `KeyFolding`, `ExpandPaths`, `with_key_folding`, `with_flatten_depth`, `with_expand_paths`, and the corresponding CLI flags. Dotted keys are always literal. There is no v3 compatibility mode.

### Fixed

- Scope termination, exact list markers, nested-field last-write-wins behavior, and comment/blank-line handling across arena and streaming decoding.
- Quoted-token termination, invalid escapes and literal controls, malformed headers, empty tab-separated cells, and strict/non-strict API parity.
- Float parsing now uses correctly rounded conversion. Declared lengths no longer drive unbounded preallocation.
- Avoid repeated header-key hashing for strict flat tabular rows and skip the header-span prepass when no interior blank lines exist, recovering decode performance without relaxing validation.

### Migration

- Update the library dependency to `serde_toon_format = "0.2"` (or retain your existing `serde_toon` dependency alias) and remove the deleted options and flags.
- Before upgrading stored v3 documents, decode them with a v3 decoder using their original options, then re-encode with this release. In particular, lines matching `^ *#` now become comments, and folded dotted paths no longer expand.
- Upgrade downstream decoders before sending output from this encoder: nested fields and keyed tabular objects require newer syntax support.

## [0.1.2] - 2026-02-04
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
