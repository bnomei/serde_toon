# toon CLI

Command-line interface for encoding JSON to TOON and decoding TOON back to JSON.

## Install

Local install from this repo (workspace):

```bash
cargo install --path cli
```

Or from the repo root (explicit package selection):

```bash
cargo install --path . --package toon-cli
```

Run from the workspace without install:

```bash
cargo run -p toon-cli -- <input> [options]
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
- `--indent <number>` Indentation size (default: 2)
- `--stats` Show token count estimates and savings (encode only)
- `--no-strict` Disable strict validation when decoding
- `--keyFolding <mode>` Key folding mode: off, safe (default: off)
- `--flattenDepth <number>` Maximum segments to fold (default: Infinity) - requires --keyFolding safe
- `--expandPaths <mode>` Path expansion mode: off, safe (default: off)

## Buffered IO

This CLI currently buffers input and output in memory. It reads the full input into a string and builds the full output before writing. Plan for input size accordingly.
