# Serde TOON

[![Crates.io Version](https://img.shields.io/crates/v/serde_toon_format)](https://crates.io/crates/serde_toon_format)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/serde_toon/ci.yml?branch=main)](https://github.com/bnomei/serde_toon/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/serde_toon_format)](https://crates.io/crates/serde_toon_format)
[![License](https://img.shields.io/crates/l/serde_toon_format)](https://crates.io/crates/serde_toon_format)

Serde-compatible [TOON](https://toonformat.dev) v3.0 encoder/decoder with optional v1.5 features, validated by the [spec fixture suite](https://github.com/toon-format/spec) (275 tests).

```toml
[dependencies]
serde_toon_format = "0.1"
```

Official TOON site: https://toonformat.dev
Specification repo: https://github.com/toon-format/spec

## What is TOON

- Token-efficient format that targets ~40% fewer tokens and ~74% accuracy in mixed-structure benchmarks across 4 models (per TOON spec site).
- JSON data model with deterministic, lossless round-trips for objects, arrays, and primitives.
- LLM-friendly guardrails: explicit array lengths and field headers for reliable parsing.
- Minimal syntax: indentation over braces and minimal quoting, with tabular arrays for uniform objects.
- Multi-language ecosystem with spec-driven implementations (TypeScript, Python, Go, Rust, .NET, others).

## Why serde_toon_format

- TOON v3.0 implementation with optional v1.5 features (key folding and path expansion).
- Conformance-first: spec fixtures in `tests/fixtures` executed by `tests/conformance.rs`, plus sectioned spec tests in `tests/spec_*`.
- Performance-first: optimized encoder/decoder, streaming APIs (`to_writer`, `from_reader`), buffer APIs (`to_vec`, `from_slice`), optional parallel decode via `parallel`.
- Serde-native API, auto-detect macro (`toon!`), canonical encoding (`encode_canonical`), and strict validation (`validate_str`).

## Release notes

- v0.1.1 (2026-01-20): `toon!` now supports `encode_json:` (and options) to encode JSON strings directly to TOON.

## Quick encode/decode

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
let toon_from_json = toon!(encode_json: r#"{"name":"Grace Hopper"}"#)?;
let value = toon!("name: Ada Lovelace")?;
assert_eq!(toon, "name: Ada Lovelace\nage: 37");
assert_eq!(toon_from_json, "name: Grace Hopper");
assert_eq!(value, serde_json::json!({"name": "Ada Lovelace"}));
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

This TOON encoder/decoder is well optimized. You can compare it to other implementations with `serde_json` as a baseline.


### Median timings (lower is better)

| Library | GitHub Repos | Peanuts Characters | Peanuts Specials | Peanuts Universe | Peanuts JSON-LD |
| --- | --- | --- | --- | --- | --- |
| serde_toon_format encode | 241.64 µs | 21.057 µs | 14.334 µs | 29.048 µs | 33.826 µs |
| serde_toon_format decode | 92.94 µs | 10.432 µs | 24.949 µs | 18.683 µs | 64.089 µs |
| serde_json encode | 28.824 µs | 3.2412 µs | 2.4854 µs | 2.9901 µs | 6.8633 µs |
| serde_json decode | 46.657 µs | 4.1360 µs | 4.9380 µs | 5.1913 µs | 12.488 µs |

## TOON ecosystem snapshot (crates.io)

Descriptions are copied from crates.io; spec/version claims are only what the authors state there.

| Crate | Focus (crates.io description) | Spec version claim | Serde mentioned? |
| --- | --- | --- | --- |
| `serde_toon_format` | Serde-compatible TOON v3.0 encoder/decoder | v3.0 | Yes |
| `serde_toon` | Serde-compatible TOON serialization library | Not stated | Yes |
| `json2toon_rs` | JSON ↔ TOON converter, “full TOON v2.0 specification compliance” | v2.0 | Not stated |
| `toon-rs` | TOON encoder/decoder with serde integration | Not stated | Yes |
| `toon-format` | Token-efficient JSON alternative for LLM prompts | Not stated | Not stated |
| `toon` | Token-efficient JSON alternative for LLM prompts | Not stated | Not stated |
| `toon-rust` | Rust implementation, “half the tokens” | Not stated | Not stated |
| `rtoon` | Compact, human-readable TOON for LLM data | Not stated | Not stated |

Related tooling (not encoder/decoder crates): `toon-lsp` (LSP), `toon-macro` (macros), `toon-ld`/`toon-core` (linked data).

### Dataset structure notes

The TOON snippets below are truncated to a few representative rows; `...` marks omitted entries. Token counts use `cl100k_base` over minified JSON (`serde_json::to_string`) and default TOON encoding.

GitHub repos: large, uniform array of flat objects, which TOON compacts into a tabular array with a single header row.

```toon
[100]{id,name,repo,description,createdAt,updatedAt,pushedAt,stars,watchers,forks,defaultBranch}:
  28457823,freeCodeCamp,freeCodeCamp/freeCodeCamp,"freeCodeCamp.org's open-source codebase and curriculum. Learn math, programming, and computer science for free.","2014-12-24T17:49:19Z","2025-10-28T11:58:08Z","2025-10-28T10:17:16Z",430886,8583,42146,main
  ...
  106017343,tailwindcss,tailwindlabs/tailwindcss,A utility-first CSS framework for rapid UI development.,"2017-10-06T14:59:14Z","2025-10-28T12:25:13Z","2025-10-28T12:25:08Z",90816,615,4766,main
```

Token count (cl100k_base): JSON 11,348, TOON 8,838.

Peanuts characters: top-level object with context metadata plus a uniform `characters` array; TOON tabularizes the array while keeping context readable.

```toon
context:
  dataset: peanuts_characters
  focus: main_characters
  source: Wikipedia
sources[1]: "https://en.wikipedia.org/wiki/List_of_Peanuts_characters"
characters[12]{id,slug,displayName,introducedYear,lastAppearanceYear,species,role,traits}:
  1,charlie_brown,Charlie Brown,1950,2000,human,main,"The main character, an average yet emotionally mature, gentle, considerate, and often innocent boy who has an ever-changing mood and grace; he is regarded as an embarrassment and a loser by other children and is strongly disliked and rejected by most of them; he takes his frequent failures personally, yet rises out of nearly every downfall with renewed optimism and determination."
  ...
  12,rerun_van_pelt,Rerun Van Pelt,1973,2000,human,main,Younger brother of Linus and Lucy; frequently rides on the back of his mother's bicycle; often takes his siblings' places and roles.
```

Token count (cl100k_base): JSON 1,058, TOON 843.

Peanuts specials: similar top-level object, but each item includes nested arrays (`otherNetworks`), so TOON keeps it as a list block.

```toon
context:
  dataset: peanuts_specials
  focus: tv_specials
  source: Wikipedia
sources[1]: "https://en.wikipedia.org/wiki/Peanuts_filmography"
specials[13]:
  - id: 1
    slug: a_charlie_brown_christmas
    title: A Charlie Brown Christmas
    airDate: "1965-12-09"
    network: CBS
    otherNetworks[3]: ABC,Apple TV+,PBS
    notes: First Peanuts special
  ...
  - id: 13
    slug: be_my_valentine_charlie_brown
    title: "Be My Valentine, Charlie Brown"
    airDate: "1975-01-28"
    network: CBS
    otherNetworks[2]: ABC,Apple TV+
    notes: null
```

Token count (cl100k_base): JSON 744, TOON 896.

Peanuts universe: deep nested objects for `context` plus uniform arrays like `owners`, which TOON keeps compact with tabular rows.

```toon
context:
  title: Peanuts
  author: Charles M. Schulz
  launch:
    daily: "1950-10-02"
    sunday: "1952-01-06"
  ...
owners[3]{name,startYear,endYear}:
  United Feature Syndicate,1950,1978
  United Media,1978,2011
  Peanuts Worldwide,2011,null
```

Token count (cl100k_base): JSON 834, TOON 678.

Peanuts JSON-LD: JSON-LD graph uses `@` keys and a large `@graph` list; TOON quotes the `@` keys and keeps the graph as a list block.

```toon
"@context": "https://schema.org"
"@graph"[19]:
  - "@id": "https://www.peanuts.com/#organization"
    "@type": Organization
    name: Peanuts Worldwide
    url: "https://www.peanuts.com"
    datePublished: null
    category: rights_holder
    isPartOf: null
    author: null
    publisher: null
    genre[0]:
    description: Owner of the Peanuts brand and official website.
    sameAs[1]: "https://en.wikipedia.org/wiki/Peanuts"
  ...
  - "@id": "https://www.peanuts.com/#character-woodstock"
    "@type": Person
    name: Woodstock
    url: "https://en.wikipedia.org/wiki/Woodstock_(Peanuts)"
    datePublished: "1966-03-17"
    category: character
    isPartOf: "https://www.peanuts.com/#series"
    author: null
    publisher: null
    genre[1]: character
    description: "Snoopy's best friend; a tiny yellow bird. First seen in early 1966, Schulz did not give him a name until June 22, 1970."
    sameAs[1]: "https://en.wikipedia.org/wiki/Woodstock_(Peanuts)"
```

Token count (cl100k_base): JSON 2,550, TOON 2,938.

## License

MIT. See `LICENSE`.
