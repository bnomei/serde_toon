# src3 v3 architecture plan

## Goals
- Optimize the core use case: uniform arrays of objects, deterministic minimal quoting, explicit lengths, fixed row widths, and no repeated keys.
- Assume decoded input is valid; strict mode is for rejecting non-ideal forms, not for full validation.
- Keep the public API small and predictable.
- Support the full option surface with a fast-path + fallback design.

## Non-goals
- Exhaustive validation of malformed input.
- Legacy v1/v2 behaviors not in the v3 spec unless they serve the ideal path.
- Optimizing irregular or deeply nested data where repeated keys are not dominant.
- Replacing JSON for non-uniform or deeply nested structures.
- Replacing CSV for flat, strictly tabular data where nesting is not needed.
- General-purpose storage or public API interchange format.

## Ideal path definition
A document is considered ideal when:
- Root is an array of objects that can be emitted as a tabular array (uniform fields, scalar-only cells).
- Objects preserve encounter order (encoder-defined) for deterministic output.
- Strings are minimally quoted and only escape required characters.
- Numbers are canonicalized once at serialization time.
- Explicit array lengths match the item count and tabular rows are fixed width.
- UTF-8 is required; ASCII-only strings are an optional fast path.
- Indent size is fixed per document; active delimiter is fixed per header scope (nested headers may override per spec).
- Tabular rows and inline arrays always use the active delimiter in scope.

## Core use case focus
TOON is for tabular object arrays where:
- repeated keys are avoided by declaring fields once,
- deterministic, minimally quoted text is preferred,
- explicit lengths and fixed row widths catch truncation or malformed data,
- the structure stays human-readable without extra schema machinery.
TOON is not the answer for:
- irregular or deeply nested JSON where repeated keys are not dominant,
- flat, strictly tabular datasets where CSV is more compact,
- general-purpose storage or public API interchange.

## Document format alignment (SPEC)
- Root form: root array header vs single-line primitive vs object; empty doc decodes to `{}`; empty root object encodes to an empty document; strict rejects multiple root primitives.
- Line/indent: LF only, no trailing spaces/newline; indentSize spaces (default 2); exactly one space after `:` in `key: value` and after headers with inline values; no tabs for indentation.
- Headers: `[N<delim?>]{fields}:` with required colon; comma is default; bracket/brace delimiters must match; header sets the active delimiter for inline arrays, field lists, and rows (nested headers override).
- Arrays: inline primitive arrays; arrays of arrays as list items with inner headers; arrays of objects use tabular form when uniform + primitive-only, otherwise list items.
- Objects as list items: if first field is a tabular array, emit the tabular header on the hyphen line and rows at depth +2; otherwise first field goes on the hyphen line; bare `-` only for empty objects.
- Quoting: strings require quotes if empty, leading/trailing whitespace, true/false/null, numeric-like (including leading-zero decimals), contains colon/quote/backslash/brackets/braces/control chars, contains the relevant delimiter (document delimiter for object values, active delimiter for inline arrays/tabular cells), or equals/starts with `-`; only `\\`, `\"`, `\n`, `\r`, `\t` escapes are valid.
- Keys/fields: unquoted only if `^[A-Za-z_][A-Za-z0-9_.]*$`, otherwise quoted/escaped.
- Rows: tabular row vs key-value disambiguation uses first unquoted delimiter vs colon at row depth.
- Numbers: canonical decimal output (no exponent, no leading/trailing zeros, -0 -> 0); NaN/Infinity normalize to null; decoders accept exponent forms.
- Decoding tolerances: ignore blank lines outside arrays/tabular rows; accept trailing newline at EOF.

## High-level architecture
- Serde serializer builds a compact, canonical in-memory representation (arena-based) with precomputed string flags.
- Encoder writes directly to a byte buffer with size hints, emitting tabular arrays whenever detected.
- Parser builds an arena of nodes without heavy validation; strict mode performs cheap rejection checks.
- Deserializer reads from arena into typed values.

## Data model
- Node kinds: Null, Bool, Number, String, Array, Object, TabularArray.
- String storage: interned or arena-owned strings plus flags (needs_quote, needs_escape).
- Number storage: i64, u64, or canonical text (for floats and large ints).
- Object entries: Vec<(KeyId, NodeId)> with dedup checks only in strict mode.
- Tabular arrays: field list (KeyId list) plus row values in row-major order.

## Encoding pipeline
1) Serde Serializer builds nodes into the arena.
2) Arrays are analyzed for tabular eligibility:
   - All items are objects.
   - Same field set, all scalar values.
3) If eligible, convert to TabularArray in-place and emit in tabular form.
4) If not eligible, emit as expanded list with no extra optimization for irregular shapes.
5) Estimate output size and pre-allocate buffer.
6) Encode to Vec<u8> using a writer that appends slices, not per-char writes.
7) Optional parallel encoding for large objects/arrays (feature-gated).

## Decoding pipeline
1) Fast scanner/parser builds nodes in the arena.
2) Minimal structural checks only (tokenization, header parsing, basic delimiter scoping).
3) Strict mode rejects:
   - Missing colons after keys; invalid or unterminated quoted strings.
   - Indentation not a multiple of indentSize or tabs used for indentation.
   - Blank lines inside arrays/tabular rows.
   - Count/width mismatches for inline arrays, list arrays, and tabular rows.
   - Header/row delimiter mismatches (detected via header + row parsing).
   - Path expansion conflicts when expandPaths is enabled.
4) If expandPaths is enabled, apply safe path expansion and conflict handling.
5) Deserializer maps nodes into typed output.

## Encoder/decoder focus
- Encoder: bias toward tabular emission; treat non-uniform arrays as a fallback path without special-casing for compactness.
- Decoder: fast-path tabular arrays and length/width checks; avoid expensive normalization for irregular structures.
- Both: prioritize deterministic output/interpretation over accommodating exotic or irregular inputs.
- Both: every option must be supported; fast paths must cleanly fall back to full-feature behavior.

## Strict mode semantics
- Strict mode is a rejection filter, not a validator.
- Strict mode does not attempt to recover or normalize; it fails fast.
- Non-strict mode accepts known non-ideal forms and emits best-effort results.

## Performance strategies
- Prioritize tabular arrays and row-major paths; keep non-uniform arrays on a simple baseline.
- Arena allocation for nodes, keys, and strings.
- Precomputed string flags (quote/escape) and canonical number formatting.
- Intern keys to reduce duplicates in tabular data.
- Size estimation for large containers to reduce reallocations.
- Optional parallel encoding with per-item buffers and join.
- Avoid extra UTF-8 validation where possible; keep output ASCII where allowed.

## Reference: existing optimizations
### src_v1 encoder
- Streaming writer to `Vec<u8>` with ASCII fast-path for single chars.
- Indent cache to avoid repeated string building.
- Active delimiter stack to decide quoting in arrays vs objects.
- Array classification: tabular (uniform objects) vs inline primitive vs nested list.
- Tabular array emission with header fields and row-major output.
- Key folding (v1.5) with flatten depth and conflict detection.
- Canonical number formatting via itoa/ryu, with exponent avoidance.
- Minimal string escaping with `escape_string_into`.

### src_v1 decoder
- Scanner with cached indentation and ASCII fast-path for peek/advance.
- Byte-wise unquoted string scan with non-ASCII detection.
- Number scanning by byte range, with suffix merge to preserve strings.
- `Token::Integer` vs `Token::Number` to reduce float parsing.
- Delimiter-aware scanning and delimiter stack in parser.
- Pre-allocate arrays/rows/objects using expected lengths from headers.
- Strict-mode checks embedded in parse flow (indentation, key validity).

### src_v2 canonical encoder
- CanonicalValue tree with precomputed string flags (needs_quote/needs_escape).
- SmolStr key intern + small string reuse.
- Thread-local caches for string analysis by delimiter.
- Vec pooling for values/entries to reduce allocations.
- Tabular detection with SmallVec + HashSet field checks, row-major flatten.
- Capacity estimation + quick hints for output buffer sizing.
- Parallel encoding of large objects/arrays with deterministic join.
- Canonical number normalization (no exponent, trim zeros).

### src_v2 canonical decoder
- Preflight scan using `memchr` to classify lines and reject early.
- Canonical whitespace/indentation/delimiter checks in scan stage.
- Key-order tracking per indent to enforce sorted keys (canonical mode).
- Arena view with spans for strings/numbers to avoid allocations.
- Pre-count items/fields and reserve arena vectors.
- Parallel parsing by indentation block spans (thresholded).
- Arena-based Serde deserializer avoids intermediate `Value`.

## Ideal-path technique mapping (suggestions)
### Encoding hot paths
- Specialize tabular arrays: detect uniform rows once, emit header + row-major values.
- Precompute string flags and reuse cached analysis for common small strings/keys.
- Intern repeated keys and reuse field lists across rows.
- Use direct byte writer with pre-sized buffer for tabular blocks.
- Parallelize large arrays/objects by chunking rows/fields into per-thread buffers.

### Decoding hot paths
- Preflight line scan to validate canonical whitespace/indentation fast.
- Parse into arena with string/number spans; delay allocations until needed.
- For tabular arrays, map row fields by index (avoid per-row key lookups).
- Strict mode as cheap rejection (length mismatch, bad quoting, field count).
- Prefer ASCII fast paths; fall back to Unicode checks only when needed.

## Module layout (proposal)
- src/lib.rs: public API and options re-exports. (§1.1)
- src/options.rs: minimal encode/decode options. (§1.2)
- src/arena/mod.rs: arena storage + ids. (§1.3)
- src/arena/node.rs: node kinds and tabular node. (§1.4)
- src/tabular/mod.rs: row-major view + helpers. (§1.5)
- src/tabular/detect.rs: uniform detection and conversion. (§1.6)
- src/encode/mod.rs: encoder entry points. (§1.7)
- src/encode/serializer.rs: Serde Serializer -> arena nodes. (§1.8)
- src/encode/emit.rs: byte writer + tabular emission. (§1.9)
- src/decode/mod.rs: decoder entry points. (§1.10)
- src/decode/scan.rs: preflight scan + delimiter/indent checks. (§1.11)
- src/decode/parser.rs: fast parser into arena. (§1.12)
- src/decode/serde.rs: Deserializer from arena. (§1.13)
- src/text/string.rs: quoting, escaping, string analysis. (§1.14)
- src/num/number.rs: canonical number formatting. (§1.15)

## Module performance notes (v1/v2-derived)
§1.1 `src/lib.rs`: keep wrappers thin; avoid extra allocations or intermediate `Value` creation.
§1.2 `src/options.rs`: use compact, `Copy`-friendly options (e.g., `u8` delimiter) and precompute derived flags.
§1.3 `src/arena/mod.rs`: arena allocation with pre-reserve from scanned counts; reuse vectors/pools where possible.
§1.4 `src/arena/node.rs`: store string/number spans or small enums to avoid heap allocations for scalars.
§1.5 `src/tabular/mod.rs`: row-major storage with field list `KeyId` reuse; map by index to avoid per-row lookups.
§1.6 `src/tabular/detect.rs`: uniform detection with early exits; SmallVec/HashSet field checks; convert in-place.
§1.7 `src/encode/mod.rs`: size hints and optional parallel encoding thresholds for large containers.
§1.8 `src/encode/serializer.rs`: precompute string flags; intern keys; reserve vectors based on container sizes.
§1.9 `src/encode/emit.rs`: streaming writer to `Vec<u8>`; indent cache; delimiter stack; ASCII fast path for small writes.
§1.10 `src/decode/mod.rs`: choose fast vs full paths based on scan results; reuse arena between calls when possible.
§1.11 `src/decode/scan.rs`: `memchr` preflight; indentation caching; delimiter/quote checks before full parse.
§1.12 `src/decode/parser.rs`: byte-wise unquoted scans; `Token::Integer` vs `Token::Number`; pre-reserve arrays/rows.
§1.13 `src/decode/serde.rs`: arena-based deserializer to avoid intermediate `Value`; tabular rows mapped by index.
§1.14 `src/text/string.rs`: cached string analysis by delimiter; escape only when needed; ASCII fast path.
§1.15 `src/num/number.rs`: itoa/ryu canonical formatting; avoid exponent; trim zeros deterministically.

## API surface (proposal)
- encode: to_string, to_vec, to_writer
- decode: from_str, from_slice, from_reader
- encoder options (spec): indent, delimiter, keyFolding, flattenDepth
- decoder options (spec): indent, strict, expandPaths
- non-spec knobs: keep feature-gated and off the public API surface (e.g., parallel thresholds)

## Open decisions
- Key ordering policy for objects: preserve encounter order (spec); decide whether to expose optional canonical sorting as a non-conformant mode.
- Unsafe fast path for String creation (from_utf8_unchecked) behind a feature.
- Arena reuse strategy for batch encoding/decoding.

## Phased delivery
1) Scaffold src3 modules and minimal API with stubs.
2) Implement encoder + tabular detection + benchmarks.
3) Implement decoder + strict rejection rules.
4) Optimize memory + parallel thresholds.
5) Compare performance and refine thresholds.
