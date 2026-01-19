#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PROFILE_SECONDS="${PROFILE_SECONDS:-30}"
PROFILE_FREQ="${PROFILE_FREQ:-100}"
PROFILE_DIR="${PROFILE_DIR:-benchmarks/profiles}"

mkdir -p "$PROFILE_DIR"

run_encode() {
  local name="$1"
  local input="${2:-}"
  local args=(cargo run --example profile_encode -- --seconds "$PROFILE_SECONDS" --freq "$PROFILE_FREQ" --out "$PROFILE_DIR/encode_${name}")
  if [[ "${PROFILE_BYTES:-}" == "1" ]]; then
    args+=(--bytes)
  fi
  if [[ -n "$input" ]]; then
    args+=(--input "$input")
  fi
  "${args[@]}"
}

run_decode() {
  local name="$1"
  local input="${2:-}"
  local args=(cargo run --example profile_decode -- --seconds "$PROFILE_SECONDS" --freq "$PROFILE_FREQ" --out "$PROFILE_DIR/decode_${name}")
  if [[ -n "$input" ]]; then
    args+=(--input "$input")
  fi
  "${args[@]}"
}

run_encode "github" ""
run_decode "github" ""

for name in characters specials universe jsonld; do
  input="benchmarks/data/peanuts_${name}.json"
  run_encode "peanuts_${name}" "$input"
  run_decode "peanuts_${name}" "$input"
done

for svg in "$PROFILE_DIR"/*.svg; do
  prefix="${svg%.svg}"
  uv run python benchmarks/flamegraph_to_csv.py "$svg" --out-prefix "$prefix"
done
