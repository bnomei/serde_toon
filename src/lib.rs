//! # Serde TOON
//!
//! `serde_toon` is a Serde-compatible implementation of TOON v3.0 with v1.5
//! optional features. The API mirrors `serde_json` helpers while preserving
//! TOON-specific encode/decode options.
//!
//! ## Example
//! ```rust
//! use serde::{Deserialize, Serialize};
//! use serde_toon::{from_str, to_string};
//! # use serde_toon::Error;
//! #[derive(Debug, Serialize, Deserialize, PartialEq)]
//! struct User {
//!     name: String,
//!     age: u32,
//! }
//!
//! # fn main() -> Result<(), serde_toon::Error> {
//! let user = User {
//!     name: "Ada".to_string(),
//!     age: 37,
//! };
//!
//! let toon = to_string(&user)?;
//! let round_trip: User = from_str(&toon)?;
//! assert_eq!(user, round_trip);
//! # Ok(())
//! # }
//! ```
#![warn(rustdoc::missing_crate_level_docs)]

mod constants;
mod decode;
mod encode;
mod serde;
mod types;
mod utils;

pub use serde_json::{Map, Number, Value};

pub use decode::{
    decode, decode_default, decode_no_coerce, decode_no_coerce_with_options, decode_strict,
    decode_strict_with_options,
};
pub use encode::{encode, encode_default, encode_value, encode_value_default};
pub use serde::{
    from_reader, from_reader_with_options, from_slice, from_slice_with_options, from_str,
    from_str_with_options, to_string, to_string_value, to_string_value_with_options,
    to_string_with_options, to_vec, to_vec_value, to_vec_value_with_options, to_vec_with_options,
    to_writer, to_writer_value, to_writer_value_with_options, to_writer_with_options,
};

pub use types::{
    is_identifier_segment, DecodeOptions, Delimiter, EncodeOptions, ErrorContext, Indent,
    KeyFoldingMode, PathExpansionMode, ToonError, ToonResult,
};

pub use types::{ToonError as Error, ToonResult as Result};

pub use utils::{
    literal::{is_keyword, is_literal_like},
    string::{escape_string, is_valid_unquoted_key, needs_quoting},
};

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor, path::PathBuf};

    use ::serde::{Deserialize, Serialize};
    use serde_json::{json, Value};

    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct User {
        name: String,
        age: u32,
    }

    #[rstest::rstest]
    fn test_round_trip_string_api() {
        let user = User {
            name: "Ada".to_string(),
            age: 37,
        };
        let encoded = to_string(&user).unwrap();
        let decoded: User = from_str(&encoded).unwrap();
        assert_eq!(decoded, user);
    }

    #[rstest::rstest]
    fn test_writer_reader_round_trip() {
        let user = User {
            name: "Turing".to_string(),
            age: 41,
        };
        let mut buffer = Vec::new();
        to_writer(&mut buffer, &user).unwrap();

        let mut reader = Cursor::new(buffer);
        let decoded: User = from_reader(&mut reader).unwrap();
        assert_eq!(decoded, user);
    }

    #[rstest::rstest]
    fn test_options_wiring() {
        let data = json!({"items": ["a", "b"]});
        let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
        let encoded = to_string_with_options(&data, &opts).unwrap();
        assert!(encoded.contains('|'));

        let decode_opts = DecodeOptions::new().with_strict(false);
        let decoded: Value = from_str_with_options("items[2]: a", &decode_opts).unwrap();
        assert_eq!(decoded, json!({"items": ["a"]}));
    }

    #[rstest::rstest]
    fn test_vec_and_slice_api() {
        let user = User {
            name: "Grace".to_string(),
            age: 60,
        };
        let bytes = to_vec(&user).unwrap();
        let decoded: User = from_slice(&bytes).unwrap();
        assert_eq!(decoded, user);
    }

    #[rstest::rstest]
    fn test_from_slice_invalid_utf8() {
        let err = from_slice::<Value>(&[0xff]).unwrap_err();
        assert!(matches!(err, ToonError::InvalidInput(_)));
    }

    #[rstest::rstest]
    fn test_tabular_arrays() {
        let cases = vec![
            json!({
                "users": [
                    {"id": 1, "name": "Alice"},
                    {"id": 2, "name": "Bob"}
                ]
            }),
            json!({
                "products": [
                    {"sku": "A1", "name": "Widget", "price": 9.99, "stock": 100},
                    {"sku": "B2", "name": "Gadget", "price": 19.99, "stock": 50}
                ]
            }),
            json!({
                "items": [
                    {"a": 1, "b": 2, "c": 3}
                ]
            }),
            json!({
                "data": (0..10).map(|i| json!({"id": i, "value": i * 2})).collect::<Vec<_>>()
            }),
        ];

        for case in cases {
            let encoded = encode_default(&case).unwrap();
            assert!(encoded.contains("{"));
            assert!(encoded.contains("}"));
            let decoded: Value = decode_default(&encoded).unwrap();
            assert_eq!(case, decoded);
        }
    }

    #[rstest::rstest]
    fn test_mixed_arrays() {
        let data = json!({
            "mixed": [1, "two", true, null, std::f64::consts::PI]
        });

        let encoded = encode_default(&data).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[rstest::rstest]
    fn test_empty_values() {
        let cases = vec![
            json!({"array": []}),
            json!({"object": {}}),
            json!({"string": ""}),
            json!({"null": null}),
        ];

        for case in cases {
            let encoded = encode_default(&case).unwrap();
            let decoded: Value = decode_default(&encoded).unwrap();
            assert_eq!(case, decoded);
        }
    }

    #[rstest::rstest]
    fn test_large_arrays() {
        let large_array = json!({
            "numbers": (0..1000).collect::<Vec<i32>>()
        });

        let encoded = encode_default(&large_array).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(large_array, decoded);

        let large_tabular = json!({
            "records": (0..500).map(|i| json!({
                "id": i,
                "name": format!("user_{}", i),
                "value": i * 2
            })).collect::<Vec<_>>()
        });

        let encoded = encode_default(&large_tabular).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(large_tabular, decoded);
    }

    #[rstest::rstest]
    fn test_special_characters_and_quoting() {
        let cases = vec![
            json!({"value": "true"}),
            json!({"value": "false"}),
            json!({"value": "42"}),
            json!({"value": "3.14"}),
            json!({"value": "hello, world"}),
            json!({"value": "hello|world"}),
            json!({"value": "say \"hello\""}),
            json!({"value": "line1\nline2"}),
            json!({"value": ""}),
            json!({"value": " hello "}),
        ];

        for case in cases {
            let encoded = encode_default(&case).unwrap();
            let decoded: Value = decode_default(&encoded).unwrap();
            assert_eq!(case, decoded, "Failed for: {case}");
        }
    }

    #[rstest::rstest]
    fn test_nested_structures() {
        let nested = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": "deep"
                    }
                }
            }
        });

        let encoded = encode_default(&nested).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(nested, decoded);
    }

    #[rstest::rstest]
    fn test_delimiter_variants() {
        let data = json!({"tags": ["a", "b", "c"]});

        let encoded = encode_default(&data).unwrap();
        assert!(encoded.contains("a,b,c"));
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);

        let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
        let encoded = encode(&data, &opts).unwrap();
        assert!(encoded.contains("a|b|c"));
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);

        let opts = EncodeOptions::new().with_delimiter(Delimiter::Tab);
        let encoded = encode(&data, &opts).unwrap();
        assert!(encoded.contains("a\tb\tc"));
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[rstest::rstest]
    fn test_delimiter_in_values() {
        let data = json!({"tags": ["a,b", "c|d", "e\tf"]});

        let encoded = encode_default(&data).unwrap();
        assert!(encoded.contains("\"a,b\""));
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);

        let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
        let encoded = encode(&data, &opts).unwrap();
        assert!(encoded.contains("\"c|d\""));
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[rstest::rstest]
    fn test_non_active_delimiters_in_tabular_arrays() {
        let data = r#""item-list"[1]{a,b}:
  ":",|
"#;
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["item-list"][0]["a"], ":");
        assert_eq!(decoded["item-list"][0]["b"], "|");

        let data = r#""item-list"[1]{a,b}:
  ":","|"
"#;
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["item-list"][0]["a"], ":");
        assert_eq!(decoded["item-list"][0]["b"], "|");

        let data = "\"item-list\"[1]{a,b}:\n  \":\",\"\\t\"\n";
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["item-list"][0]["a"], ":");
        assert_eq!(decoded["item-list"][0]["b"], "\t");

        let data = r#""item-list"[1|]{a|b}:
  ":"|","
"#;
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["item-list"][0]["a"], ":");
        assert_eq!(decoded["item-list"][0]["b"], ",");
    }

    #[rstest::rstest]
    fn test_non_active_delimiters_in_inline_arrays() {
        let data = r#"tags[3]: a,|,c"#;
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["tags"], json!(["a", "|", "c"]));

        let data = "tags[3|]: a|\",\"|c";
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["tags"], json!(["a", ",", "c"]));

        let data = r#"items[4]: |,|,|,"#;
        let decoded: Value = decode_default(data).unwrap();
        assert_eq!(decoded["items"], json!(["|", "|", "|", ""]));
    }

    #[rstest::rstest]
    fn test_delimiter_mismatch_error() {
        let data = r#""item-list"[1|]{a,b}:
  ":",|
"#;
        let result: std::result::Result<Value, _> = decode_default(data);
        assert!(result.is_err(), "Mismatched delimiters should error");
    }

    #[rstest::rstest]
    fn test_numeric_edge_cases() {
        let numbers = json!({
            "zero": 0,
            "negative": -42,
            "large": 9999999999i64,
            "small": -9999999999i64,
            "decimal": std::f64::consts::PI,
            "scientific": 1.23e10,
            "tiny": 0.0000001
        });

        let encoded = encode_default(&numbers).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();

        assert_eq!(decoded["zero"], json!(0));
        assert_eq!(decoded["negative"], json!(-42));
    }

    #[rstest::rstest]
    fn test_comprehensive_round_trips() {
        let test_cases = vec![
            json!(null),
            json!(true),
            json!(false),
            json!(42),
            json!(-42),
            json!(std::f64::consts::PI),
            json!("hello"),
            json!(""),
            json!({"key": "value"}),
            json!({"a": 1, "b": 2, "c": 3}),
            json!({"nested": {"key": "value"}}),
            json!({"array": [1, 2, 3]}),
            json!({"mixed": [1, "two", true, null]}),
            json!({"users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]}),
            json!({"empty_array": []}),
            json!({"empty_object": {}}),
        ];

        for (i, case) in test_cases.iter().enumerate() {
            let encoded =
                encode_default(case).unwrap_or_else(|e| panic!("Failed to encode case {i}: {e:?}"));
            let decoded: Value = decode_default::<Value>(&encoded)
                .unwrap_or_else(|e| panic!("Failed to decode case {i}: {e}"));
            assert_eq!(
                case, &decoded,
                "Round-trip failed for case {i}: Original: {case}, Decoded: {decoded}"
            );
        }
    }

    #[rstest::rstest]
    fn test_real_world_github_data() {
        let github_repos = json!({
            "repositories": [
                {
                    "id": 28457823,
                    "name": "freeCodeCamp",
                    "full_name": "freeCodeCamp/freeCodeCamp",
                    "stars": 430886,
                    "watchers": 8583,
                    "forks": 42146,
                    "language": "TypeScript",
                    "has_issues": true,
                    "has_wiki": true
                },
                {
                    "id": 132750724,
                    "name": "build-your-own-x",
                    "full_name": "codecrafters-io/build-your-own-x",
                    "stars": 430877,
                    "watchers": 6332,
                    "forks": 40453,
                    "language": "Markdown",
                    "has_issues": true,
                    "has_wiki": false
                }
            ]
        });

        let encoded = encode_default(&github_repos).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(github_repos, decoded);

        assert!(encoded.contains("repositories[2]{"));
        assert!(encoded.contains("}:"));
    }

    #[rstest::rstest]
    fn test_real_world_e_commerce_data() {
        let order = json!({
            "order_id": "ORD-12345",
            "customer": {
                "id": 5678,
                "name": "John Doe",
                "email": "john@example.com"
            },
            "items": [
                {
                    "sku": "WIDGET-001",
                    "name": "Premium Widget",
                    "quantity": 2,
                    "price": 29.99,
                    "discount": 0.1
                },
                {
                    "sku": "GADGET-042",
                    "name": "Super Gadget",
                    "quantity": 1,
                    "price": 149.99,
                    "discount": 0.0
                }
            ],
            "shipping": {
                "method": "express",
                "cost": 15.50,
                "address": "123 Main St, City, State 12345"
            },
            "total": 224.46
        });

        let encoded = encode_default(&order).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(decoded["order_id"], order["order_id"]);
        assert_eq!(decoded["customer"], order["customer"]);
        assert_eq!(decoded["shipping"], order["shipping"]);
        assert_eq!(decoded["total"], order["total"]);
        assert_eq!(decoded["items"].as_array().unwrap().len(), 2);
    }

    #[rstest::rstest]
    fn test_unicode_strings() {
        let unicode = json!({
            "emoji": "😀🎉🦀",
            "chinese": "你好世界",
            "arabic": "مرحبا",
            "mixed": "Hello 世界 🌍"
        });

        let encoded = encode_default(&unicode).unwrap();
        let decoded: Value = decode_default(&encoded).unwrap();
        assert_eq!(unicode, decoded);
    }

    #[derive(Deserialize, Debug)]
    struct FixtureFile {
        tests: Vec<TestCase>,
    }

    #[derive(Deserialize, Debug, Clone)]
    struct TestCase {
        name: String,
        input: Value,
        expected: Value,
        #[serde(default)]
        options: TestOptions,
        #[serde(default, rename = "shouldError")]
        should_error: bool,
    }

    #[derive(Deserialize, Debug, Clone, Default)]
    #[serde(rename_all = "camelCase")]
    struct TestOptions {
        strict: Option<bool>,
        expand_paths: Option<String>,
        delimiter: Option<String>,
        indent: Option<usize>,
        key_folding: Option<String>,
        flatten_depth: Option<usize>,
    }

    fn collect_fixture_files(path: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(path)
            .unwrap_or_else(|_| panic!("Missing fixture directory: {path}"))
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        files
    }

    #[rstest::rstest]
    fn test_decode_spec_fixtures() {
        for path in collect_fixture_files("spec/tests/fixtures/decode") {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("Failed to read fixture {}", path.display()));

            let file_data: FixtureFile = serde_json::from_str(&contents)
                .unwrap_or_else(|e| panic!("Failed to parse JSON fixture {}: {e}", path.display()));

            let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");

            for test in file_data.tests {
                let test_name = format!("[decode] {file_name}: {}", test.name);

                let mut opts = DecodeOptions::new();
                if let Some(strict) = test.options.strict {
                    opts = opts.with_strict(strict);
                }
                if let Some(indent) = test.options.indent {
                    opts = opts.with_indent(Indent::Spaces(indent));
                }
                if let Some(expand_paths_str) = &test.options.expand_paths {
                    let mode = match expand_paths_str.as_str() {
                        "safe" => PathExpansionMode::Safe,
                        "off" => PathExpansionMode::Off,
                        _ => panic!("Invalid expandPaths value: {expand_paths_str}"),
                    };
                    opts = opts.with_expand_paths(mode);
                }

                let toon_input = test.input.as_str().unwrap_or_else(|| {
                    panic!("Test '{test_name}': input field is not a string")
                });

                let result = decode::<Value>(toon_input, &opts);

                if test.should_error {
                    if let Ok(actual_json) = result {
                        panic!(
                            "Test '{test_name}' should have FAILED, but it succeeded with: {:?}",
                            actual_json
                        );
                    }
                } else {
                    let actual_json = result.unwrap_or_else(|e| {
                        panic!(
                            "Test '{test_name}' should have SUCCEEDED, but it FAILED with: {e:?}"
                        )
                    });

                    if actual_json != test.expected {
                        panic!(
                            "Test '{test_name}' succeeded, but the JSON output was incorrect.\n\
Expected: {:?}\nActual: {actual_json:?}",
                            test.expected
                        );
                    }
                }
            }
        }
    }

    #[rstest::rstest]
    fn test_encode_spec_fixtures() {
        for path in collect_fixture_files("spec/tests/fixtures/encode") {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("Failed to read fixture {}", path.display()));

            let file_data: FixtureFile = serde_json::from_str(&contents)
                .unwrap_or_else(|e| panic!("Failed to parse JSON fixture {}: {e}", path.display()));

            let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");

            for test in file_data.tests {
                let test_name = format!("[encode] {file_name}: {}", test.name);

                let mut opts = EncodeOptions::new();
                if let Some(indent) = test.options.indent {
                    opts = opts.with_indent(Indent::Spaces(indent));
                }
                if let Some(delim_str) = &test.options.delimiter {
                    let delim = match delim_str.as_str() {
                        "," => Delimiter::Comma,
                        "\t" => Delimiter::Tab,
                        "|" => Delimiter::Pipe,
                        _ => panic!("Invalid delimiter in fixture: {delim_str}"),
                    };
                    opts = opts.with_delimiter(delim);
                }
                if let Some(key_folding_str) = &test.options.key_folding {
                    let mode = match key_folding_str.as_str() {
                        "safe" => KeyFoldingMode::Safe,
                        "off" => KeyFoldingMode::Off,
                        _ => panic!("Invalid keyFolding value: {key_folding_str}"),
                    };
                    opts = opts.with_key_folding(mode);
                }
                if let Some(flatten_depth) = test.options.flatten_depth {
                    opts = opts.with_flatten_depth(flatten_depth);
                }

                let result = encode(&test.input, &opts);

                let expected_toon = test.expected.as_str().unwrap_or_else(|| {
                    panic!("Test '{test_name}': expected field is not a string")
                });

                let encoded_toon = result.unwrap_or_else(|e| {
                    panic!(
                        "Test '{test_name}' should have SUCCEEDED, but it FAILED with: {e:?}"
                    )
                });

                let normalized_result = encoded_toon.replace("\r\n", "\n");
                let normalized_expected = expected_toon.replace("\r\n", "\n");

                if normalized_result != normalized_expected {
                    panic!(
                        "Test '{test_name}' succeeded, but the TOON output was incorrect.\n\
Expected:\n{normalized_expected}\nActual:\n{normalized_result}",
                    );
                }
            }
        }
    }
}
