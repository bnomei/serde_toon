# Changelog

## [Unreleased]

## [0.2.0] - 2026-09-28

First crates.io release of `serde_toon_format_cli`, providing the `toon` binary.

- Targets the TOON 4.1 Working Draft through `serde_toon_format` 0.2.0.
- Encodes JSON and decodes TOON, including nested tabular fields, keyed tabular objects, and full-line comments on decode.
- Removes key-folding, flatten-depth, and path-expansion flags. Dotted keys are literal.
- Uses the library's streaming decoder and whitespace rules without CLI-only tab preprocessing.
- Documents `--indentSize` and retains `--indent-size` and `--indent` aliases.

See the [workspace changelog](../CHANGELOG.md) for compatibility changes and migration guidance.
