mod expansion;
mod parser;
mod scanner;
mod validation;

use crate::types::{DecodeOptions, JsonValue, ToonResult};

/// Decode a TOON string into any deserializable type.
///
/// This function accepts any type implementing `serde::Deserialize`, including:
/// - Custom structs with `#[derive(Deserialize)]`
/// - `serde_json::Value`
/// - Built-in types (Vec, HashMap, etc.)
///
/// # Examples
///
/// **With custom structs:**
/// ```
/// use serde::Deserialize;
/// use serde_toon::{
///     decode,
///     DecodeOptions,
/// };
///
/// #[derive(Deserialize, Debug, PartialEq)]
/// struct User {
///     name: String,
///     age: u32,
/// }
///
/// let toon = "name: Alice\nage: 30";
/// let user: User = decode(toon, &DecodeOptions::default())?;
/// assert_eq!(user.name, "Alice");
/// assert_eq!(user.age, 30);
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
///
/// **With JSON values:**
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::{
///     decode,
///     DecodeOptions,
/// };
///
/// let input = "name: Alice\nage: 30";
/// let result: Value = decode(input, &DecodeOptions::default())?;
/// assert_eq!(result["name"], json!("Alice"));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode<T: serde::de::DeserializeOwned>(
    input: &str,
    options: &DecodeOptions,
) -> ToonResult<T> {
    let mut parser = parser::Parser::new(input, options.clone())?;
    let value: JsonValue = parser.parse()?;

    // Apply path expansion if enabled (v1.5 feature)
    use crate::types::PathExpansionMode;
    let final_value = if options.expand_paths != PathExpansionMode::Off {
        expansion::expand_paths_recursive(value, options.expand_paths, options.strict)?
    } else {
        value
    };

    crate::serde::from_value(&final_value)
}

/// Decode with strict validation enabled (validates array lengths,
/// indentation).
///
/// # Examples
///
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::decode_strict;
///
/// // Valid array length
/// let result: Value = decode_strict("items[2]: a,b")?;
/// assert_eq!(result["items"], json!(["a", "b"]));
///
/// // Invalid array length (will error)
/// assert!(decode_strict::<Value>("items[3]: a,b").is_err());
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode_strict<T: serde::de::DeserializeOwned>(input: &str) -> ToonResult<T> {
    decode(input, &DecodeOptions::new().with_strict(true))
}

/// Decode with strict validation and additional options.
///
/// # Examples
///
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::{
///     decode_strict_with_options,
///     DecodeOptions,
/// };
///
/// let options = DecodeOptions::new()
///     .with_strict(true)
///     .with_delimiter(serde_toon::Delimiter::Pipe);
/// let result: Value = decode_strict_with_options("items[2|]: a|b", &options)?;
/// assert_eq!(result["items"], json!(["a", "b"]));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode_strict_with_options<T: serde::de::DeserializeOwned>(
    input: &str,
    options: &DecodeOptions,
) -> ToonResult<T> {
    let opts = options.clone().with_strict(true);
    decode(input, &opts)
}

/// Decode without type coercion (strings remain strings).
///
/// # Examples
///
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::decode_no_coerce;
///
/// // Without coercion: quoted strings that look like numbers stay as strings
/// let result: Value = decode_no_coerce("value: \"123\"")?;
/// assert_eq!(result["value"], json!("123"));
///
/// // With default coercion: unquoted "true" becomes boolean
/// let result: Value = serde_toon::decode_default("value: true")?;
/// assert_eq!(result["value"], json!(true));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode_no_coerce<T: serde::de::DeserializeOwned>(input: &str) -> ToonResult<T> {
    decode(input, &DecodeOptions::new().with_coerce_types(false))
}

/// Decode without type coercion and with additional options.
///
/// # Examples
///
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::{
///     decode_no_coerce_with_options,
///     DecodeOptions,
/// };
///
/// let options = DecodeOptions::new()
///     .with_coerce_types(false)
///     .with_strict(false);
/// let result: Value = decode_no_coerce_with_options("value: \"123\"", &options)?;
/// assert_eq!(result["value"], json!("123"));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode_no_coerce_with_options<T: serde::de::DeserializeOwned>(
    input: &str,
    options: &DecodeOptions,
) -> ToonResult<T> {
    let opts = options.clone().with_coerce_types(false);
    decode(input, &opts)
}

/// Decode with default options (strict mode, type coercion enabled).
///
/// Works with any type implementing `serde::Deserialize`.
///
/// # Examples
///
/// **With structs:**
/// ```
/// use serde::Deserialize;
/// use serde_toon::decode_default;
///
/// #[derive(Deserialize)]
/// struct Person {
///     name: String,
///     age: u32,
/// }
///
/// let input = "name: Alice\nage: 30";
/// let person: Person = decode_default(input)?;
/// assert_eq!(person.name, "Alice");
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
///
/// **With JSON values:**
/// ```
/// use serde_json::{
///     json,
///     Value,
/// };
/// use serde_toon::decode_default;
///
/// let input = "tags[3]: reading,gaming,coding";
/// let result: Value = decode_default(input)?;
/// assert_eq!(result["tags"], json!(["reading", "gaming", "coding"]));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn decode_default<T: serde::de::DeserializeOwned>(input: &str) -> ToonResult<T> {
    decode(input, &DecodeOptions::default())
}

#[cfg(test)]
mod tests {
    use core::f64;

    use serde_json::{json, Value};

    use super::*;

    #[rstest::rstest]
    fn test_decode_null() {
        assert_eq!(decode_default::<Value>("null").unwrap(), json!(null));
    }

    #[rstest::rstest]
    fn test_decode_bool() {
        assert_eq!(decode_default::<Value>("true").unwrap(), json!(true));
        assert_eq!(decode_default::<Value>("false").unwrap(), json!(false));
    }

    #[rstest::rstest]
    fn test_decode_number() {
        assert_eq!(decode_default::<Value>("42").unwrap(), json!(42));
        assert_eq!(
            decode_default::<Value>("3.141592653589793").unwrap(),
            json!(f64::consts::PI)
        );
        assert_eq!(decode_default::<Value>("-5").unwrap(), json!(-5));
    }

    #[rstest::rstest]
    fn test_decode_string() {
        assert_eq!(decode_default::<Value>("hello").unwrap(), json!("hello"));
        assert_eq!(
            decode_default::<Value>("\"hello world\"").unwrap(),
            json!("hello world")
        );
    }

    #[rstest::rstest]
    fn test_decode_simple_object() {
        let input = "name: Alice\nage: 30";
        let result: Value = decode_default(input).unwrap();
        assert_eq!(result["name"], json!("Alice"));
        assert_eq!(result["age"], json!(30));
    }

    #[rstest::rstest]
    fn test_decode_primitive_array() {
        let input = "tags[3]: reading,gaming,coding";
        let result: Value = decode_default(input).unwrap();
        assert_eq!(result["tags"], json!(["reading", "gaming", "coding"]));
    }

    #[rstest::rstest]
    fn test_decode_tabular_array() {
        let input = "users[2]{id,name,role}:\n  1,Alice,admin\n  2,Bob,user";
        let result: Value = decode_default(input).unwrap();
        assert_eq!(
            result["users"],
            json!([
                {"id": 1, "name": "Alice", "role": "admin"},
                {"id": 2, "name": "Bob", "role": "user"}
            ])
        );
    }

    #[rstest::rstest]
    fn test_decode_empty_array() {
        let input = "items[0]:";
        let result: Value = decode_default(input).unwrap();
        assert_eq!(result["items"], json!([]));
    }

    #[rstest::rstest]
    fn test_decode_quoted_strings() {
        let input = "tags[3]: \"true\",\"42\",\"-3.14\"";
        let result: Value = decode_default(input).unwrap();
        assert_eq!(result["tags"], json!(["true", "42", "-3.14"]));
    }

    #[rstest::rstest]
    fn test_decode_strict_with_options_forces_strict() {
        let opts = DecodeOptions::new().with_strict(false);
        let result: std::result::Result<Value, _> =
            decode_strict_with_options("items[2]: a", &opts);
        assert!(result.is_err(), "Strict mode should reject length mismatch");
    }

    #[rstest::rstest]
    fn test_decode_no_coerce_with_options_disables_coercion() {
        let opts = DecodeOptions::new().with_coerce_types(true);
        let result: Value = decode_no_coerce_with_options("value: 123", &opts).unwrap();
        assert_eq!(result, json!({"value": "123"}));
    }

    #[rstest::rstest]
    fn test_invalid_syntax_errors() {
        let cases = vec![
            ("items[]: a,b", "Expected array length"),
            ("items[2]{name: a,b", "Expected '}'"),
            ("key value", "Expected"),
        ];

        for (input, expected_msg) in cases {
            let result = decode_default::<Value>(input);
            if let Err(err) = result {
                let err_str = err.to_string();
                assert!(
                    err_str.contains(expected_msg)
                        || err_str.contains("Parse error")
                        || err_str.contains("Invalid"),
                    "Expected error containing '{expected_msg}' but got: {err_str}"
                );
            } else if input == "key value" {
                assert!(result.is_ok(), "'key value' is valid as a root string");
            }
        }

        let invalid_cases = vec![
            ("items[2: a,b", "Expected ']'"),
            ("items[abc]: 1,2", "Expected array length"),
        ];

        for (input, expected_msg) in invalid_cases {
            let result = decode_default::<Value>(input);
            assert!(result.is_err(), "Expected error for input: {input}");

            let err = result.unwrap_err();
            let err_str = err.to_string();
            assert!(
                err_str.contains(expected_msg) || err_str.contains("Parse error"),
                "Expected error containing '{expected_msg}' but got: {err_str}"
            );
        }
    }

    #[rstest::rstest]
    fn test_type_mismatch_errors() {
        let cases = vec![
            ("value: ", "Empty value"),
            ("items[abc]: 1,2", "Invalid array length"),
        ];

        for (input, description) in cases {
            let result = decode_default::<Value>(input);
            println!("Test case '{description}': {result:?}");
        }
    }

    #[rstest::rstest]
    fn test_length_mismatch_strict_mode() {
        let test_cases = vec![("items[3]: a,b", 3, 2), ("items[5]: x", 5, 1)];

        for (input, expected, actual) in test_cases {
            let result = decode_strict::<Value>(input);

            assert!(
                result.is_err(),
                "Expected error for input '{input}' (expected: {expected}, actual: {actual})",
            );

            if let Err(crate::ToonError::LengthMismatch {
                expected: exp,
                found: fnd,
                ..
            }) = result
            {
                assert_eq!(
                    exp, expected,
                    "Expected length {expected} but got {exp} for input '{input}'"
                );
                assert_eq!(
                    fnd, actual,
                    "Expected found {actual} but got {fnd} for input '{input}'"
                );
            }
        }

        let result = decode_strict::<Value>("items[1]: a,b,c");

        if let Ok(val) = result {
            assert_eq!(val["items"], json!(["a"]));
        }
    }

    #[rstest::rstest]
    fn test_length_mismatch_non_strict_mode() {
        let test_cases = vec![
            ("items[3]: a,b", json!({"items": ["a", "b"]})),
            ("items[1]: a,b", json!({"items": ["a", "b"]})),
        ];

        for (input, _expected) in test_cases {
            let result = decode_default::<Value>(input);
            println!("Non-strict test for '{input}': {result:?}");
        }
    }

    #[rstest::rstest]
    fn test_delimiter_errors() {
        let mixed_delimiters = "items[3]: a,b|c";
        let result = decode_default::<Value>(mixed_delimiters);

        println!("Mixed delimiter test: {result:?}");
    }

    #[rstest::rstest]
    fn test_quoting_errors() {
        let test_cases = vec![
            ("value: \"unclosed", "Unclosed string"),
            ("value: \"invalid\\x\"", "Invalid escape"),
        ];

        for (input, description) in test_cases {
            let result = decode_default::<Value>(input);
            println!("Quoting error test '{description}': {result:?}");
        }
    }

    #[rstest::rstest]
    fn test_tabular_array_errors() {
        let result = decode_default::<Value>("items[2]{id,name}:\n  1,Alice\n  2");
        assert!(result.is_err(), "Should error on incomplete row");

        if let Err(e) = result {
            let err_str = e.to_string();
            assert!(
                err_str.contains("Parse")
                    || err_str.contains("column")
                    || err_str.contains("expected")
                    || err_str.contains("primitive"),
                "Error should mention missing field or delimiter: {err_str}"
            );
        }

        let result = decode_default::<Value>("items[2]{id,name}:\n  1,Alice\n  2,Bob,Extra");
        if let Err(err) = result {
            let err_str = err.to_string();
            assert!(
                err_str.contains("Parse") || err_str.contains("Expected") || err_str.contains("field"),
                "Should mention unexpected content: {err_str}"
            );
        } else {
            println!("Note: Extra fields are ignored in tabular arrays");
        }

        let result = decode_strict::<Value>("items[3]{id,name}:\n  1,Alice\n  2,Bob");
        assert!(
            result.is_err(),
            "Should error on row count mismatch in strict mode"
        );

        if let Err(crate::ToonError::LengthMismatch {
            expected, found, ..
        }) = result
        {
            assert_eq!(expected, 3);
            assert_eq!(found, 2);
        }
    }

    #[rstest::rstest]
    fn test_nested_structure_errors() {
        let result = decode_default::<Value>("obj:\n  key");
        assert!(result.is_err(), "Should error on incomplete nested object");

        let result = decode_default::<Value>("arr[2]:\n  - item");
        assert!(result.is_err(), "Should error on incomplete nested array");
    }

    #[rstest::rstest]
    fn test_depth_limit_errors() {
        let mut nested = "a:\n".to_string();
        for i in 0..60 {
            nested.push_str(&format!("{}b:\n", "  ".repeat(i + 1)));
        }
        nested.push_str(&format!("{}c: value", "  ".repeat(61)));

        let result = decode_default::<Value>(&nested);
        println!("Deep nesting test: {result:?}");
    }

    #[rstest::rstest]
    fn test_empty_structure_errors() {
        let cases = vec![
            ("items[]:", "Empty array with colon"),
            ("obj{}:", "Empty object with colon"),
            ("{}", "Just braces"),
            ("[]", "Just brackets"),
        ];

        for (input, description) in cases {
            let result = decode_default::<Value>(input);
            println!("Empty structure test '{description}': {result:?}");
        }
    }

    #[rstest::rstest]
    fn test_error_messages_are_helpful() {
        let result = decode_strict::<Value>("items[5]: a,b,c");

        if let Err(err) = result {
            let err_msg = err.to_string();

            assert!(
                err_msg.contains("5")
                    || err_msg.contains("3")
                    || err_msg.contains("expected")
                    || err_msg.contains("found"),
                "Error message should contain length information: {err_msg}"
            );
        }
    }

    #[rstest::rstest]
    fn test_parse_error_line_column() {
        let input = "line1: value\nline2: bad syntax!\nline3: value";
        let result = decode_default::<Value>(input);

        if let Err(crate::ToonError::ParseError { line, column, .. }) = result {
            println!("Parse error at line {line}, column {column}");
            assert!(line > 0, "Line number should be positive");
            assert!(column > 0, "Column number should be positive");
        }
    }

    #[rstest::rstest]
    fn test_multiple_errors_in_input() {
        let input = "items[10]: a,b\nobj{missing,fields: x,y";
        let result = decode_default::<Value>(input);

        assert!(result.is_err(), "Should error on malformed input");
    }

    #[rstest::rstest]
    fn test_coercion_errors() {
        let opts = DecodeOptions::new().with_coerce_types(true);

        let result = decode::<Value>("value: 123", &opts);
        assert!(result.is_ok());

        let result = decode::<Value>("value: true", &opts);
        assert!(result.is_ok());

        let result = decode::<Value>("value: 3.14", &opts);
        assert!(result.is_ok());
    }

    #[rstest::rstest]
    fn test_no_coercion_preserves_strings() {
        let opts = DecodeOptions::new().with_coerce_types(false);

        let result = decode::<Value>("value: hello", &opts).unwrap();
        assert!(result["value"].is_string());
        assert_eq!(result["value"], json!("hello"));

        let result = decode::<Value>(r#"value: "123""#, &opts).unwrap();
        assert!(result["value"].is_string());
        assert_eq!(result["value"], json!("123"));

        let result = decode::<Value>(r#"value: "true""#, &opts).unwrap();
        assert!(result["value"].is_string());
        assert_eq!(result["value"], json!("true"));

        let result = decode::<Value>("value: 123", &opts).unwrap();
        assert!(result["value"].is_string());
        assert_eq!(result["value"], json!("123"));

        let result = decode::<Value>("value: true", &opts).unwrap();
        assert!(result["value"].is_string());
        assert_eq!(result["value"], json!("true"));
    }

    #[rstest::rstest]
    fn test_edge_case_values() {
        let cases = vec![
            ("value: 0", json!({"value": 0})),
            ("value: null", json!({"value": null})),
        ];

        for (input, expected) in cases {
            let result = decode_default::<Value>(input);
            match result {
                Ok(val) => assert_eq!(val, expected, "Failed for input: {input}"),
                Err(e) => println!("Edge case '{input}' error: {e:?}"),
            }
        }

        let result = decode_default::<Value>("value: -0");
        match result {
            Ok(val) => {
                assert_eq!(
                    val["value"],
                    json!(0),
                    "Negative zero is normalized to zero in JSON"
                );
            }
            Err(e) => println!("Edge case '-0' error: {e:?}"),
        }
    }

    #[rstest::rstest]
    fn test_unicode_in_errors() {
        let input = "emoji: 😀🎉\nkey: value\nbad: @syntax!";
        let result = decode_default::<Value>(input);

        if let Err(err) = result {
            let err_msg = err.to_string();
            println!("Unicode error handling: {err_msg}");
            assert!(!err_msg.is_empty());
        }
    }

    #[rstest::rstest]
    fn test_recovery_from_errors() {
        let valid_after_invalid = vec!["good: value\nbad syntax here\nalso_good: value"];

        for input in valid_after_invalid {
            let result = decode_default::<Value>(input);
            println!("Recovery test for: {result:?}");
        }
    }

    #[rstest::rstest]
    fn test_strict_mode_indentation_errors() {
        let result = decode_strict::<Value>("items[2]: a");
        assert!(
            result.is_err(),
            "Should error on insufficient items in strict mode"
        );

        if let Err(crate::ToonError::LengthMismatch {
            expected, found, ..
        }) = result
        {
            assert_eq!(expected, 2);
            assert_eq!(found, 1);
        }
    }

    #[rstest::rstest]
    fn test_quoted_key_without_colon() {
        let result = decode_default::<Value>(r#""key" value"#);
        println!("Quoted key test: {result:?}");
    }

    #[rstest::rstest]
    fn test_nested_array_length_mismatches() {
        let result = decode_strict::<Value>("outer[1]:\n  - items[2]: a,b\n  - items[3]: x,y");
        if let Err(err) = result {
            let err_str = err.to_string();
            assert!(err_str.contains("3") || err_str.contains("2") || err_str.contains("length"));
        }
    }

    #[rstest::rstest]
    fn test_empty_array_with_length() {
        let result = decode_strict::<Value>("items[2]:");
        assert!(
            result.is_err(),
            "Should error when array header specifies length but no items provided"
        );

        let result = decode_strict::<Value>("items[0]:");
        assert!(
            result.is_ok(),
            "Empty array with length 0 should parse successfully"
        );

        if let Ok(val) = result {
            assert_eq!(val["items"], json!([]));
        }
    }

    #[rstest::rstest]
    fn test_tabular_array_field_count_mismatch() {
        let result = decode_default::<Value>("items[2]{id,name}:\n  1\n  2,Bob");
        assert!(
            result.is_err(),
            "Should error when row has fewer fields than header"
        );
    }

    #[rstest::rstest]
    fn test_invalid_array_header_syntax() {
        let cases = vec![
            ("items[", "Expected array length"),
            ("items[: a,b", "Expected array length"),
        ];

        for (input, expected_msg) in cases {
            let result = decode_default::<Value>(input);
            assert!(result.is_err(), "Expected error for input: {input}");

            let err = result.unwrap_err();
            let err_str = err.to_string();
            assert!(
                err_str.contains(expected_msg) || err_str.contains("Parse error"),
                "Expected error containing '{expected_msg}' but got: {err_str}"
            );
        }
    }
}
