# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-01-20
- Added a value-only fast path for tabular decoding and routed `decode_to_value` through a direct Value decoder.
- Added a `toon` CLI crate with encode/decode auto-detection, stats reporting, and key-folding/expand-paths options.
- Added cross-crate benchmark harness plus TOON datasets for comparisons in `benchmarks/`.
- Updated workspace metadata (version bump, categories/keywords/homepage, CLI workspace member).
- Expanded README benchmark docs and clarified JSON round-trip examples.
