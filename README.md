# Serde TOON

Serde-compatible TOON v3.0 encoder/decoder with optional v1.5 features.

```toml
[dependencies]
serde_toon = "0.1"
```

## Example

```rust
use serde::{Deserialize, Serialize};
use serde_toon::{from_str, to_string};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    name: String,
    age: u32,
}

let user = User {
    name: "Ada".to_string(),
    age: 37,
};

let toon = to_string(&user)?;
assert_eq!(toon, "name: Ada\nage: 37");
let round_trip: User = from_str(&toon)?;
assert_eq!(user, round_trip);
# Ok::<(), serde_toon::Error>(())
```

## Untyped values

```rust
use serde_toon::Value;

let value: Value = serde_toon::from_str("name: Margaret\nage: 32")?;
assert_eq!(value, serde_json::json!({"name": "Margaret", "age": 32}));
# Ok::<(), serde_toon::Error>(())
```

## Custom options

```rust
use serde_toon::{Delimiter, EncodeOptions};

let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
let toon = serde_toon::to_string_with_options(&serde_json::json!({"items": ["a", "b"]}), &opts)?;
assert_eq!(toon, "items[2|]: a|b");
# Ok::<(), serde_toon::Error>(())
```

Other options include indentation (`Indent::Spaces(n)` or `with_spaces`), key folding
(`KeyFoldingMode::Safe` with `with_flatten_depth`), and decode controls like
`with_strict(false)`, `with_coerce_types(false)`, `with_delimiter(...)`, or
`with_expand_paths(PathExpansionMode::Safe)`.

## Benchmarks

Benchmarks live in `benches/encode_decode.rs` and cover typed structs across:

- `uniform_repos` (synthetic uniform array)
- `deep_tree` (nested tree)
- `semi_uniform_rows` (untagged enum mix)
- `config_map` (nested map-heavy config)
- `github_repos` (fixture from `benchmarks/data/github-repos.json`)

Each dataset is measured in `encode`, `decode`, and `roundtrip` groups for both TOON and JSON.

Baseline results (no `parallel` feature). Avg time uses the Criterion median from the latest run.
Parallel-feature numbers will be added next.

Encode avg time:

| Dataset | JSON | TOON | TOON (parallel) |
| --- | --- | --- | --- |
| uniform_repos | 817.04 us | 8.829 ms | 9.591 ms |
| deep_tree | 33.86 us | 236.65 us | 255.43 us |
| semi_uniform_rows | 216.01 us | 1.912 ms | 1.914 ms |
| config_map | 2.30 us | 10.09 us | 10.97 us |
| github_repos | 33.04 us | 237.19 us | 235.23 us |

Decode avg time:

| Dataset | JSON | TOON | TOON (parallel) |
| --- | --- | --- | --- |
| uniform_repos | 1.635 ms | 8.505 ms | 8.647 ms |
| deep_tree | 91.40 us | 417.68 us | 424.02 us |
| semi_uniform_rows | 879.50 us | 2.621 ms | 2.668 ms |
| config_map | 59.31 us | 56.31 us | 57.74 us |
| github_repos | 48.19 us | 273.64 us | 277.46 us |

Roundtrip avg time:

| Dataset | JSON | TOON | TOON (parallel) |
| --- | --- | --- | --- |
| uniform_repos | 2.366 ms | 16.928 ms | 16.222 ms |
| deep_tree | 120.42 us | 655.63 us | 651.17 us |
| semi_uniform_rows | 1.094 ms | 4.739 ms | 4.582 ms |
| config_map | 60.75 us | 66.02 us | 66.60 us |
| github_repos | 79.16 us | 503.12 us | 508.32 us |

Run without parallel (serial):

```
cargo bench --bench encode_decode --no-default-features
```

Run with TOON's `parallel` feature (now default):

```
cargo bench --bench encode_decode
```

Filter to a specific dataset or group using Criterion's pattern matching, for example:

```
cargo bench github_repos
cargo bench encode::github_repos
```

Parallel behavior is controlled by the feature flag and internal heuristics.
Encoding heuristics can be tuned via environment variables:

- `TOON_ESTIMATE_MIN_FIELDS` (default 16)
- `TOON_ESTIMATE_MIN_ITEMS` (default 32)
- `TOON_TABULAR_MIN_ROWS` (default 2)

Criterion writes JSON outputs under `target/criterion/<group>/<format>/<dataset>/new/`.
Example: `target/criterion/encode/toon/github_repos/new/estimates.json`.

## License

MIT. See `LICENSE`.
