# toon CLI

Command-line interface for encoding JSON to TOON and decoding TOON back to JSON.
Version 0.2.0 targets the TOON 4.1 Working Draft. See the
[release notes and migration guide](../CHANGELOG.md#020---2026-09-28) before upgrading from 0.1.x.

## Install

Local install from this repo (workspace):

```bash
cargo install --path cli
```

Install the released package from crates.io:

```bash
cargo install serde_toon_format_cli --version 0.2.0 --locked
```

Run from the workspace without install:

```bash
cargo run -p serde_toon_format_cli -- <input> [options]
```

## Usage

```bash
toon <input> [options]
```

Input is optional; omit it or pass `-` to read from stdin.

## Options

- `-o, --output <file>` Output file path (prints to stdout if omitted)
- `-e, --encode` Force encode mode (overrides auto-detection)
- `-d, --decode` Force decode mode (overrides auto-detection)
- `--delimiter <char>` Array delimiter: , (comma), \t (tab), | (pipe)
- `--indentSize <number>` Indentation size (default: 2; aliases: `--indent-size`, `--indent`)
- `--stats` Show token count estimates and savings (encode only)
- `--no-strict` Disable strict count/width validation and accept leading indentation tabs as one indentation unit each. Comment classification still happens before indentation handling; tab-prefixed `#` lines are data, not comments. Uses the library's whitespace policy (see the repository README).

## Buffered IO

- Encode: buffers JSON input in memory before encoding.
- Decode: streams TOON input through the library's streaming decoder, without preprocessing that changes the document.
- Stats: `--stats` buffers the full TOON output to compute token estimates.
