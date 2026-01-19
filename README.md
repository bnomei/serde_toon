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

## License

MIT. See `LICENSE`.
