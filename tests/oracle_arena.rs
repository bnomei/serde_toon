use serde_json::json;
use serde_toon::{from_str, from_str_with_options, validate_str, DecodeOptions, Value};

fn decode(input: &str) -> serde_toon::Result<Value> {
    from_str(input)
}
fn lenient(input: &str) -> serde_toon::Result<Value> {
    from_str_with_options(input, &DecodeOptions::new().with_strict(false))
}

#[test]
fn oracle_structural_cases() {
    assert!(decode("a[1]{x}:\n  1\n  b: 2").is_err());
    assert_eq!(decode("[1]{x}:\n  - x").unwrap(), json!([{"x":"- x"}]));
    assert!(decode("[1]:\n  -1").is_err());
    assert_eq!(decode("[1]:\n  - \u{a0}x").unwrap(), json!(["\u{a0}x"]));
    let nested = "[2]:\n  - m[2:]{x}:\n      a: 1\n      b: 2\n  - 0";
    assert_eq!(
        lenient(nested).unwrap(),
        json!([{"m":{"a":{"x":1},"b":{"x":2}}},0])
    );
    assert_eq!(
        decode("[1]:\n  - a: 1\n    # comment\n    b: 2").unwrap(),
        json!([{"a":1,"b":2}])
    );
}

#[test]
fn oracle_blank_spans() {
    assert!(decode("[1]:\n  - a:\n      b: 1\n\n      c: 2").is_err());
    assert!(decode("[1]:\n\n  - 1").is_ok());
    assert!(decode("a[1]:\n  - 1\n\nb: 2").is_ok());
}

#[test]
fn oracle_scalars_and_strings() {
    assert_eq!(decode("hello world").unwrap(), json!("hello world"));
    assert!(lenient("a: 1\nb").is_err());
    assert!(lenient("hello\nworld").is_err());
    assert!(decode("\"a\0b\"").is_err());
    assert!(decode("\"\\u+041\"").is_err());
    assert!(decode("\"\\uD800\"").is_err());
    assert_eq!(decode("\"a\tb\"").unwrap(), json!("a\tb"));
}

#[test]
fn oracle_headers_whitespace_and_validator() {
    assert!(decode("[0]{x} :").is_err());
    assert!(decode("[18446744073709551615]:").is_err());
    assert_eq!(decode("\u{a0}").unwrap(), json!("\u{a0}"));
    assert_eq!(
        decode("[1\t]{a\tb}:\n  \t1").unwrap(),
        json!([{"a":"","b":1}])
    );
    validate_str("hello").unwrap();
    validate_str("1e2").unwrap();
}

#[test]
fn flat_table_keys_and_values_stay_aligned_across_rows() {
    for width in [1, 16, 17] {
        let keys: Vec<_> = (0..width).rev().map(|i| format!("k{i}")).collect();
        let mut input = format!("[3]{{{}}}:\n", keys.join(","));
        let mut expected = Vec::new();
        for row in 0..3 {
            let values: Vec<_> = (0..width).map(|col| row * 100 + col).collect();
            input.push_str(&format!(
                "  {}\n",
                values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            expected.push(Value::Object(
                keys.iter()
                    .cloned()
                    .zip(values.into_iter().map(Value::from))
                    .collect(),
            ));
        }
        input.push_str("# trailing comment\n\n");
        let decoded = decode(&input).unwrap();
        assert_eq!(decoded, Value::Array(expected));
        for row in decoded.as_array().unwrap() {
            assert_eq!(
                row.as_object().unwrap().keys().collect::<Vec<_>>(),
                keys.iter().collect::<Vec<_>>()
            );
        }
    }
}
