# Serde TOON

[![Crates.io Version](https://img.shields.io/crates/v/serde_toon)](https://crates.io/crates/serde_toon)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/serde_toon/ci.yml?branch=main)](https://github.com/bnomei/serde_toon/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/serde_toon)](https://crates.io/crates/serde_toon)
[![License](https://img.shields.io/crates/l/serde_toon)](https://crates.io/crates/serde_toon)

Serde-compatible TOON v3.0 encoder/decoder with optional v1.5 features.

```toml
[dependencies]
serde_toon = "0.1"
```

## Auto-detect struct, JSON or TOON

```rust
use serde::Serialize;
use serde_toon::toon;

#[derive(Serialize)]
struct User {
    name: String,
    age: u32,
}

let user = User {
    name: "Ada Lovelace".to_string(),
    age: 37,
};
let toon = toon!(encode: user)?;
let json_value = toon!(r#"{"name":"Grace Hopper"}"#)?;
let toon_value = toon!("name: Ada Lovelace")?;
assert_eq!(toon, "name: Ada Lovelace\nage: 37");
assert_eq!(json_value, serde_json::json!({"name": "Grace Hopper"}));
assert_eq!(toon_value, serde_json::json!({"name": "Ada Lovelace"}));
# Ok::<(), serde_toon::Error>(())
```

## Example

Encode to TOON:

```rust
use serde::{Deserialize, Serialize};
use serde_toon::to_string;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    name: String,
    age: u32,
}

let user = User {
    name: "Ada Lovelace".to_string(),
    age: 37,
};

let toon = to_string(&user)?;
assert_eq!(toon, "name: Ada Lovelace\nage: 37");
# Ok::<(), serde_toon::Error>(())
```

TOON format string:

```toon
name: Ada Lovelace
age: 37
```

Decode back:

```rust
use serde_toon::from_str;

let toon = "name: Ada Lovelace\nage: 37";
let round_trip: User = from_str(toon)?;
assert_eq!(
    round_trip,
    User {
        name: "Ada Lovelace".to_string(),
        age: 37
    }
);
# Ok::<(), serde_toon::Error>(())
```

## JSON string round-trip

```rust
use serde_toon::{from_str, to_string_from_json_str};

let json = r#"{"name":"Grace Hopper","field":"computer science","year":1952}"#;
let toon = to_string_from_json_str(json)?;
assert_eq!(
    toon,
    "name: Grace Hopper\nfield: computer science\nyear: 1952"
);

let back_to_json =
    serde_json::to_string(&from_str::<serde_json::Value>(&toon)?)
        .map_err(|err| serde_toon::Error::serialize(format!("json encode failed: {err}")))?;
assert_eq!(back_to_json, json);
# Ok::<(), serde_toon::Error>(())
```

## Untyped values

```rust
use serde_json::Value;

let value: Value = serde_toon::from_str("name: Margaret Hamilton\nage: 32")?;
assert_eq!(value, serde_json::json!({"name": "Margaret Hamilton", "age": 32}));
# Ok::<(), serde_toon::Error>(())
```

## Custom options

```rust
use serde_toon::{Delimiter, EncodeOptions, Indent, KeyFolding};

let opts = EncodeOptions::new()
    .with_indent(Indent::spaces(4))
    .with_delimiter(Delimiter::Pipe)
    .with_key_folding(KeyFolding::Safe)
    .with_flatten_depth(Some(2));
let toon = serde_toon::to_string_with_options(&serde_json::json!({"items": ["a", "b"]}), &opts)?;
assert_eq!(toon, "items[2|]: a|b");
# Ok::<(), serde_toon::Error>(())
```

```rust
use serde_toon::{DecodeOptions, ExpandPaths, Indent};

let opts = DecodeOptions::new()
    .with_indent(Indent::spaces(4))
    .with_strict(false)
    .with_expand_paths(ExpandPaths::Safe);
let value: serde_json::Value = serde_toon::from_str_with_options("a.b: 1", &opts)?;
assert_eq!(value, serde_json::json!({"a": {"b": 1}}));
# Ok::<(), serde_toon::Error>(())
```

## Performance tips

- For large outputs, prefer `to_vec` or `to_writer` to avoid extra UTF-8 checks.

## Benchmarks

GitHub baseline (median):

- github_repos/encode_toon: 241.64 µs
- github_repos/decode_toon: 92.94 µs
- github_repos/encode_json: 28.824 µs
- github_repos/decode_json: 46.657 µs

Peanuts baseline (median):

- peanuts_characters/encode_toon: 21.057 µs
- peanuts_characters/decode_toon: 10.432 µs
- peanuts_characters/encode_json: 3.2412 µs
- peanuts_characters/decode_json: 4.1360 µs

- peanuts_specials/encode_toon: 14.334 µs
- peanuts_specials/decode_toon: 24.949 µs
- peanuts_specials/encode_json: 2.4854 µs
- peanuts_specials/decode_json: 4.9380 µs

- peanuts_universe/encode_toon: 29.048 µs
- peanuts_universe/decode_toon: 18.683 µs
- peanuts_universe/encode_json: 2.9901 µs
- peanuts_universe/decode_json: 5.1913 µs

- peanuts_jsonld/encode_toon: 33.826 µs
- peanuts_jsonld/decode_toon: 64.089 µs
- peanuts_jsonld/encode_json: 6.8633 µs
- peanuts_jsonld/decode_json: 12.488 µs

## Profiling

Generate encode/decode flamegraphs and CSV summaries for the GitHub and peanuts
datasets:

```bash
bash benchmarks/run_profiles.sh
```

Requires `uv` on PATH for the CSV conversion step (stdlib-only Python script).

Tune runtime or output location:

```bash
PROFILE_SECONDS=60 PROFILE_FREQ=200 PROFILE_DIR=benchmarks/profiles bash benchmarks/run_profiles.sh
```

Profile encode via `to_vec` instead of `to_string`:

```bash
PROFILE_BYTES=1 bash benchmarks/run_profiles.sh
```

## License

MIT. See `LICENSE`.
