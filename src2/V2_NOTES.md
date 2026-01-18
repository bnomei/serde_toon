# Canonical fast path sketch

Goal: fastest possible canonical-only TOON encode/decode with Rayon, designed to
reject non-canonical input quickly and fall back to the full parser later.

## Canonical profile (strict)

- UTF-8, LF only, no trailing spaces or trailing newline.
- Fixed indentation: 2 spaces, no tabs in indentation.
- Default delimiter: comma; array headers may declare tab/pipe delimiters.
- Canonical spacing: `key: value` with a single space after `:`, `, ` between inline/tabular values, no spaces before delimiters.
- Strings: quote only when required by TOON rules.
- Numbers: canonical decimal form only (no exponent, no leading zeros, no
  trailing zeros, -0 -> 0, NaN and Infinity rejected).
- Objects: keys MUST be sorted (bytewise UTF-8 order for now).
- Arrays: canonical array/list/tabular forms only.
- Key folding and path expansion disabled.

Notes:
- Sorted key order diverges from current spec default (preserve insertion order).
- If any canonical rule fails, we return a canonical violation and (later) fall
  back to the existing full-spec decoder.

## Decode pipeline (fast path)

1. Preflight scan (single pass)
   - Validate line endings, indentation, delimiter, quoting markers.
   - Record line offsets and indentation depth for each line.
   - Reject any obvious non-canonical patterns early.

2. Structural scan (stage 1)
   - Build a compact token stream: line type, key span, value span, array header
     data, list markers.
   - Pre-compute subtree spans by indentation boundaries.

3. Parse (stage 2)
   - Parse subtrees in parallel using Rayon when subtree size exceeds a
     threshold.
   - Build an arena (node tape) with stable ordering; strings stored in a
     string table.
   - For objects, validate monotonic key order instead of sorting.

4. Serde decode
   - Implement a Deserializer over the arena to avoid building Value.

## Encode pipeline (fast path)

1. Serialize to canonical form
   - Custom Serde Serializer that emits TOON directly.
   - For maps, collect keys and sort, then encode in canonical order.

2. Parallel emission
   - For large arrays/objects, encode each element/field into a per-thread
     buffer, then join in order.
   - Preserve canonical list/tabular rules when emitting arrays of objects.

## Parallel strategy

- Decode: parallel subtree parsing based on indentation spans.
- Encode: parallel per-element encoding, deterministic join order.
- Thresholds tuned to avoid Rayon overhead on small inputs.

## Planned fallback integration

- canonical_decode(input) -> Ok(T) | Err(CanonicalViolation)
- decode(input) first attempts canonical_decode, then falls back to the current
  full parser when a canonical violation is detected (second phase).

## Sketch file layout

- src2/
  - README.md (this document)
  - canonical/
    - profile.rs     (canonical rule definitions)
    - scan.rs        (preflight + structural scan)
    - arena.rs       (node storage)
    - parse.rs       (stage 2 parser)
    - decode.rs      (canonical decoder entry)
    - encode.rs      (canonical encoder entry)
    - serde.rs       (Serde bridge)
    - validate.rs    (canonical validators)
  - parallel/
    - decode.rs      (Rayon subtree scheduler)
    - encode.rs      (Rayon emit/join helpers)
