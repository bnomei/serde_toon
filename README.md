# Serde TOON

[![Crates.io Version](https://img.shields.io/crates/v/serde_toon_format)](https://crates.io/crates/serde_toon_format)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/serde_toon/ci.yml?branch=main)](https://github.com/bnomei/serde_toon/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/serde_toon_format)](https://crates.io/crates/serde_toon_format)
[![License](https://img.shields.io/crates/l/serde_toon_format)](https://crates.io/crates/serde_toon_format)

Serde-compatible [TOON](https://toonformat.dev) v3.0 encoder/decoder with optional v1.5 features, validated by the [spec fixture suite](https://github.com/toon-format/spec) (275 tests).

```toml
[dependencies]
serde_toon_format = "0.1"
# library name `serde_toon`
```

## What is TOON

- Token-efficient format that targets ~40% fewer tokens and ~74% accuracy in mixed-structure benchmarks across 4 models.
- JSON data model with deterministic, lossless round-trips for objects, arrays, and primitives.
- LLM-friendly guardrails: explicit array lengths and field headers for reliable parsing.
- Minimal syntax: indentation over braces and minimal quoting, with tabular arrays for uniform objects.

## Why serde_toon_format crate

- TOON v3.0 implementation with optional v1.5 features (key folding and path expansion).
- Conformance-first: spec fixtures in `tests/fixtures` executed by `tests/conformance.rs`, plus sectioned spec tests in `tests/spec_*`.
- Performance-first: optimized encoder/decoder, streaming APIs (`to_writer`, `from_reader`), buffer APIs (`to_vec`, `from_slice`), optional parallel decode via `parallel`.
- Serde-native API, auto-detect macro (`toon!`), canonical encoding (`encode_canonical`), and strict validation (`validate_str`).

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

let back_to_json = serde_json::to_string(&from_str::<serde_json::Value>(&toon)?)?;

assert_eq!(back_to_json, json);
# Ok::<(), Box<dyn std::error::Error>>(())
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
- Enable the `parallel` feature for very large, uniform tabular arrays (many rows and fields); it helps most on big datasets where per-row work dominates the overhead.

## CLI

This repo includes a `toon` CLI in `cli/` mirroring the API of the [original TOON CLI](https://toonformat.dev/cli/).

```bash
cargo install --path cli
```

```bash
toon <input> [options]
```

> [!Note]
> While the original TOON CLI will stream its output, the CLI based on this crate is buffered: it reads the full input into memory and builds the full output before writing. We trade memory consumption for performance.

## Benchmarks

This TOON encoder/decoder is well-optimized. You can compare it to other rust-based implementations with `serde_json` as a baseline.

### Median timings (lower is better)

| Library                     | GitHub_Repos (40.0 KB) | Peanuts_Characters (5.1 KB) | Peanuts_Specials (3.4 KB) | Peanuts_Universe (4.0 KB) | Peanuts_JSON-LD (12.3 KB) |
|-----------------------------|-----------------------|----------------------------|---------------------------|---------------------------|---------------------------|
| serde_json encode           | 28.824 us    | 3.2412 us          | 2.4854 us        | 2.9901 us        | 6.8633 us       |
| serde_json decode           | 46.657 us    | 4.1360 us          | 4.9380 us        | 5.1913 us        | 12.488 us       |
| 👉 serde_toon_format encode | 241.64 us    | 21.057 us          | 14.334 us        | 29.048 us        | 33.826 us       |
| 👉 serde_toon_format decode    | 92.94 us     | 10.432 us          | 24.949 us        | 18.683 us        | 64.089 us       |

### GitHub_Repos dataset comparison across TOON crates (criterion)

Run with `cargo bench --manifest-path benchmarks/toon/Cargo.toml`.

We split results into **Value-only** and **Typed** to keep comparisons fair and explicit. Value-only uses `serde_json::Value` as a shared denominator so every crate can participate even if it doesn’t offer a typed/serde API. Typed uses `Vec<GitHubRepo>` to show real-world serde integration and the extra cost of struct mapping. This makes it clear whether a library is fast at raw format parsing or fast end-to-end with typed data.

The performance tables below use default settings; see the Defaults section for exact options, and any deviations needed for a crate to run are called out in the Notes.

#### Value-only (`serde_json::Value`):

| Library | Spec | Serde | Encode | Decode | Notes |
| --- | --- | -- | --- | --- | --- |
|  🏁 serde_json |  |  | 31.37 us | 120.92 us | BASELINE |
| json2toon_rs | v2.0 | - | 131.62 us | 220.72 us | decode to `Value`; likely faster due to a lean, value-only path with minimal validation |
| toon-rust | ? | - | 188.39 us | 36.22 us  | decode to `Value`, strict=false; likely faster due to skipped strict validation |
| 👉 serde_toon_format | v3.0 | ✅ | 244.81 us | 174.89 us | |
| serde_toon | ? | ✅ | 305.83 us | 315.63 us | decode patched: quote unquoted `t`/`f`/`n` + digit-leading strings |
| toon-rs | ? | ✅ | 328.82 us | 255.43 us | decode to `Value` |
| toon-format | ? | - | 486.41 us | 339.05 us | |
| rtoon | ? | - | 496.36 us | 238.96 us | decode patched: quote digit-leading strings |
| toon | ? | - | 66356 us | - | encode-only API |


#### Typed (`Vec<GitHubRepo>`):

| Library | Spec | Serde | Encode | Decode | Notes |
| --- | --- | --- | --- | --- | --- |
|  🏁 serde_json |  |  | 29.33 us | 46.64 us  | BASELINE |
| 👉 serde_toon_format | v3.0 | ✅ | 238.71 us | 95.82 us  | |
| toon-rust | ? | - | 259.25 us | - | typed decode failed (array length mismatch) |
| serde_toon | ? | ✅ | 268.38 us | 402.34 us | decode via `Value`; patched quotes for `t`/`f`/`n` + digit-leading strings |
| toon-format | ? | - | 474.47 us | 271.59 us | |
| toon-rs | ? | ✅ | 324.29 us | 260.61 us | decode via `Value` → struct |
| rtoon | ? | - | 566.21 us | 247.10 us | decode patched: quote digit-leading strings |

#### Defaults (bench)

Defaults listed here are the settings applied in the benchmarks (defaults unless noted in the table/Notes).

| Library | indent | delim              | strict | key_folding | flatten | expand_paths |
| --- | --- |--------------------| --- | --- | --- | --- |
| 👉 serde_toon_format | 2 | enc:comma/dec:auto | dec:true | Off | None | Off |
| toon-format | 2 | enc:comma/dec:auto | dec:true | Off | MAX | Off |
| toon-rs | 2 | comma              | false | Off | None | Off |
| rtoon | 2 | enc:comma/dec:auto | dec:true | — | — | — |
| toon-rust | 2 | comma              | false | — | — | — |
| json2toon_rs | 2 | comma              | true | — | — | — |
| serde_toon | 2 | comma              | — | — | — | — |
| toon | 2 | comma              | — | — | — | — |

#### Strict + Validated

**Strict + Validated** is a separate pass/fail check. We run each decoder in its strict/validated mode when available, and we do not apply any patches or lenient settings for that table. If a crate can’t parse the dataset under strict validation, it’s marked ❌. 

| Library | Pass (strict+validated) |
| --- | --- |
| 👉 serde_toon_format | ✅ |
| toon-rs | ✅ |
| serde_toon | ❌ |
| toon-format | ❌ |
| toon-rust | ❌ |
| rtoon | ❌ |
| json2toon_rs | ❌ |
| toon | ❌ |

### Dataset structure notes

The TOON snippets below are truncated to a few representative rows; `...` marks omitted entries. Token counts use `cl100k_base` over minified JSON (`serde_json::to_string`) and default TOON encoding.

#### GitHub_Repos
large, uniform array of flat objects, which TOON compacts into a tabular array with a single header row.

> Token count (cl100k_base): JSON 11,348, TOON 8,838.

```toon
[100]{id,name,repo,description,createdAt,updatedAt,pushedAt,stars,watchers,forks,defaultBranch}:
  28457823,freeCodeCamp,freeCodeCamp/freeCodeCamp,"freeCodeCamp.org's open-source codebase and curriculum. Learn math, programming, and computer science for free.","2014-12-24T17:49:19Z","2025-10-28T11:58:08Z","2025-10-28T10:17:16Z",430886,8583,42146,main
  ...
  106017343,tailwindcss,tailwindlabs/tailwindcss,A utility-first CSS framework for rapid UI development.,"2017-10-06T14:59:14Z","2025-10-28T12:25:13Z","2025-10-28T12:25:08Z",90816,615,4766,main
```

#### Peanuts characters
top-level object with context metadata plus a uniform `characters` array; TOON tabularizes the array while keeping context readable.

> Token count (cl100k_base): JSON 1,058, TOON 843.

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

#### Peanuts specials
similar top-level object, but each item includes nested arrays (`otherNetworks`), so TOON keeps it as a list block.

> Token count (cl100k_base): JSON 744, TOON 896.

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

#### Peanuts universe
deep nested objects for `context` plus uniform arrays like `owners`, which TOON keeps compact with tabular rows.

> Token count (cl100k_base): JSON 834, TOON 678.

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

#### Peanuts JSON-LD
JSON-LD graph uses `@` keys and a large `@graph` list; TOON quotes the `@` keys and keeps the graph as a list block.

> Token count (cl100k_base): JSON 2,550, TOON 2,938.

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

## License

MIT. See `LICENSE`.
