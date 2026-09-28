use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use serde_toon::{
    decode_to_value_with_options, from_reader_with_options, from_slice_with_options,
    from_str_with_options, to_string_with_options, DecodeOptions, Delimiter, EncodeOptions, Indent,
};

#[derive(Debug, Deserialize)]
struct FixtureFile {
    tests: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCase {
    name: String,
    input: Value,
    #[serde(default)]
    expected: Value,
    #[serde(default)]
    should_error: bool,
    options: Option<FixtureOptions>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FixtureOptions {
    delimiter: Option<String>,
    indent_size: Option<usize>,
    strict: Option<bool>,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture_file(path: &Path) -> FixtureFile {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|err| panic!("failed to parse fixture {}: {err}", path.display()))
}

fn load_fixture_dir(category: &str) -> Vec<(PathBuf, FixtureFile)> {
    let root = fixture_root().join(category);
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", root.display()))
    {
        let entry = entry
            .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", root.display()));
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        entries.push((path.clone(), load_fixture_file(&path)));
    }
    entries.sort_by_key(|(path, _)| path.file_name().map(|name| name.to_os_string()));
    entries
}

fn encode_options(options: Option<&FixtureOptions>) -> EncodeOptions {
    let mut mapped = EncodeOptions::default();
    if let Some(options) = options {
        if let Some(indent) = options.indent_size {
            mapped.indent = Indent::Spaces(indent);
        }
        if let Some(delimiter) = options.delimiter.as_deref() {
            mapped.delimiter = match delimiter {
                "," => Delimiter::Comma,
                "\t" => Delimiter::Tab,
                "|" => Delimiter::Pipe,
                _ => panic!("unsupported delimiter in fixture options: {delimiter:?}"),
            };
        }
    }
    mapped
}

fn decode_options(options: Option<&FixtureOptions>) -> DecodeOptions {
    let mut mapped = DecodeOptions::default();
    if let Some(options) = options {
        if let Some(indent) = options.indent_size {
            mapped.indent = Indent::Spaces(indent);
        }
        if let Some(strict) = options.strict {
            mapped.strict = strict;
        }
    }
    mapped
}

// The spec compares numbers mathematically, not by serde_json's integer/float
// storage variant. Keep integer-to-integer comparisons exact.
fn json_model_eq(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => {
            if a.is_f64() && b.is_f64() {
                return a.as_f64() == b.as_f64();
            }
            let (float, integer) = if a.is_f64() { (a, b) } else { (b, a) };
            let float = float.as_f64().unwrap();
            let integer = integer
                .as_i64()
                .map(i128::from)
                .or_else(|| integer.as_u64().map(i128::from))
                .unwrap();
            float.fract() == 0.0
                && float >= i64::MIN as f64
                && float < 18_446_744_073_709_551_616.0
                && float as i128 == integer
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_model_eq(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ak, av), (bk, bv))| ak == bk && json_model_eq(av, bv))
        }
        _ => actual == expected,
    }
}

#[rstest::rstest]
fn conformance_encode_fixtures() {
    let mut executed = 0;
    for (path, fixture) in load_fixture_dir("encode") {
        for case in fixture.tests {
            executed += 1;
            let name = format!("{}::{}", path.display(), case.name);
            let options = encode_options(case.options.as_ref());
            if case.should_error {
                assert!(
                    to_string_with_options(&case.input, &options).is_err(),
                    "expected error for {name}"
                );
                continue;
            }
            let expected = case
                .expected
                .as_str()
                .unwrap_or_else(|| panic!("encode expected must be a string for {name}"));
            let actual = to_string_with_options(&case.input, &options)
                .unwrap_or_else(|err| panic!("encode failed for {name}: {err}"));
            assert_eq!(actual, expected, "encode mismatch for {name}");
            let mut written = Vec::new();
            serde_toon::to_writer_with_options(&mut written, &case.input, &options)
                .unwrap_or_else(|err| panic!("writer failed for {name}: {err}"));
            assert_eq!(written, expected.as_bytes(), "writer mismatch for {name}");
        }
    }
    assert!(executed > 0, "no encode fixtures executed");
    println!("{executed} upstream encode fixtures passed");
}

#[rstest::rstest]
fn conformance_decode_fixtures() {
    let mut executed = 0;
    let mut failures = Vec::new();
    for (path, fixture) in load_fixture_dir("decode") {
        for case in fixture.tests {
            executed += 1;
            let name = format!("{}::{}", path.display(), case.name);
            let input = case
                .input
                .as_str()
                .unwrap_or_else(|| panic!("decode input must be a string for {name}"));
            let options = decode_options(case.options.as_ref());
            let results = [
                ("value", decode_to_value_with_options(input, &options)),
                ("str", from_str_with_options::<Value>(input, &options)),
                (
                    "slice",
                    from_slice_with_options::<Value>(input.as_bytes(), &options),
                ),
                (
                    "reader",
                    from_reader_with_options::<Value, _>(input.as_bytes(), &options),
                ),
                (
                    "streaming",
                    serde_toon::from_reader_streaming_with_options::<Value, _>(
                        input.as_bytes(),
                        &options,
                    ),
                ),
            ];
            if case.should_error {
                for (api, result) in results {
                    if result.is_ok() {
                        failures.push(format!("expected error for {name} via {api}"));
                    }
                }
                continue;
            }
            let expected = &case.expected;
            for (api, result) in results {
                match result {
                    Ok(actual) if json_model_eq(&actual, expected) => {}
                    other => failures.push(format!(
                        "{name} via {api}: expected {expected}, got {other:?}"
                    )),
                }
            }
        }
    }
    assert!(executed > 0, "no decode fixtures executed");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("{executed} upstream decode fixtures passed across five APIs");
}

#[test]
fn decoder_boundary_regressions() {
    use serde_json::json;
    let cases = [
        (
            "items[0]:\n  - 7\n  - 9",
            false,
            Some(json!({"items": [7, 9]})),
        ),
        (
            "items[0]{x}:\n  7\n  9",
            false,
            Some(json!({"items": [{"x": 7}, {"x": 9}]})),
        ),
        (
            "items[0:]{x}:\n  a: 7\n  b: 9",
            false,
            Some(json!({"items": {"a": {"x": 7}, "b": {"x": 9}}})),
        ),
        (
            "items[2]: [],{}",
            true,
            Some(json!({"items": ["[]", "{}"]})),
        ),
        ("k: {}", true, Some(json!({"k": "{}"}))),
        ("items[3]:", false, Some(json!({"items": []}))),
        ("items[3:]{x}:", false, Some(json!({"items": {}}))),
        ("items[3:]{x}:", true, None),
        ("items[ 0]:", true, None),
        ("items[0 ]:", true, None),
        ("items[0,]:", true, None),
        ("  []", true, None),
        ("items[1]{x}:\n    7", true, None),
        ("items[1|]{x\ty}:\n  7", true, None),
        ("k: \"a\" \"b\"", false, None),
        ("items[0]{\"\\q\"}:", false, None),
        ("items[0]{\"a\" x}:", false, None),
        (
            "\u{feff}# comment\nk: \u{feff}x",
            true,
            Some(json!({"k": "\u{feff}x"})),
        ),
    ];
    for (input, strict, expected) in cases {
        let options = DecodeOptions::new().with_strict(strict);
        for result in [
            decode_to_value_with_options(input, &options),
            serde_toon::from_reader_streaming_with_options::<Value, _>(input.as_bytes(), &options),
        ] {
            match &expected {
                Some(expected) => assert_eq!(
                    &result.unwrap_or_else(|err| panic!("{input:?}: {err}")),
                    expected,
                    "{input:?}"
                ),
                None => assert!(
                    result.is_err(),
                    "expected error for {input:?}, got {result:?}"
                ),
            }
        }
    }
}

#[test]
fn control_characters_round_trip() {
    for ch in '\0'..='\u{1f}' {
        let value = Value::String(ch.to_string());
        let text = serde_toon::to_string(&value).unwrap();
        assert!(text.starts_with('"'), "unquoted control character: {ch:?}");
        assert_eq!(serde_toon::decode_to_value(&text).unwrap(), value);
    }
}

#[test]
fn numeric_comparison_does_not_round_integers() {
    use serde_json::json;
    assert!(json_model_eq(&json!(1.0), &json!(1)));
    assert!(json_model_eq(&json!(-0.0), &json!(0)));
    assert!(!json_model_eq(
        &json!(9007199254740993_u64),
        &json!(9007199254740992_f64)
    ));
    assert!(!json_model_eq(
        &json!(u64::MAX),
        &json!(18_446_744_073_709_551_616.0)
    ));
}

#[test]
fn root_scalars_round_trip_across_apis() {
    use serde_json::json;
    for value in [
        json!("\u{feff}x"),
        json!("\u{a0}"),
        json!("hello world"),
        json!(51.248178375505404_f64),
        json!(1e20),
        json!(1e300),
        json!(f64::MIN_POSITIVE),
    ] {
        let encoded = serde_toon::to_string(&value).unwrap();
        assert!(!encoded.starts_with('\u{feff}'));
        let streamed: Value = serde_toon::from_reader_streaming_with_options(
            std::io::BufReader::with_capacity(1, encoded.as_bytes()),
            &DecodeOptions::default(),
        )
        .unwrap();
        assert_eq!(streamed, value, "{encoded:?}");
        assert_eq!(serde_toon::decode_to_value(&encoded).unwrap(), value);
        assert_eq!(serde_toon::from_str::<Value>(&encoded).unwrap(), value);
        serde_toon::validate_str(&encoded).unwrap();
    }
}
