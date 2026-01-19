use rstest::rstest;
use serde_json::{json, Value};
use serde_toon::{
    DecodeOptions, Delimiter, EncodeOptions, ExpandPaths as ToonExpandPaths, Indent,
    KeyFolding as ToonKeyFolding,
};

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
struct SpecOptions {
    delimiter: Option<char>,
    indent: Option<usize>,
    key_folding: Option<KeyFolding>,
    flatten_depth: Option<usize>,
    strict: Option<bool>,
    expand_paths: Option<ExpandPaths>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum KeyFolding {
    Off,
    Safe,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum ExpandPaths {
    Off,
    Safe,
}

#[allow(dead_code)]
impl SpecOptions {
    fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = Some(delimiter);
        self
    }

    fn with_indent(mut self, indent: usize) -> Self {
        self.indent = Some(indent);
        self
    }

    fn with_key_folding_safe(mut self) -> Self {
        self.key_folding = Some(KeyFolding::Safe);
        self
    }

    fn with_flatten_depth(mut self, depth: usize) -> Self {
        self.flatten_depth = Some(depth);
        self
    }

    fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    fn with_expand_paths_safe(mut self) -> Self {
        self.expand_paths = Some(ExpandPaths::Safe);
        self
    }
}

#[allow(dead_code)]
struct Spec05Adapter;

impl Spec05Adapter {
    fn encode(_input: &Value, _options: &SpecOptions) -> Result<String, String> {
        let options = map_encode_options(_options);
        serde_toon::to_string_with_options(_input, &options).map_err(|err| err.to_string())
    }

    fn decode(_input: &str, _options: &SpecOptions) -> Result<Value, String> {
        let options = map_decode_options(_options);
        serde_toon::from_str_with_options(_input, &options).map_err(|err| err.to_string())
    }

    fn validate(_input: &str) -> Result<(), String> {
        serde_toon::validate_str(_input).map_err(|err| err.to_string())
    }
}

fn map_encode_options(options: &SpecOptions) -> EncodeOptions {
    let mut encoded = EncodeOptions::default();
    if let Some(delimiter) = options.delimiter {
        encoded.delimiter = match delimiter {
            ',' => Delimiter::Comma,
            '\t' => Delimiter::Tab,
            '|' => Delimiter::Pipe,
            _ => Delimiter::Comma,
        };
    }
    if let Some(indent) = options.indent {
        encoded.indent = Indent::Spaces(indent);
    }
    if let Some(KeyFolding::Safe) = options.key_folding {
        encoded.key_folding = ToonKeyFolding::Safe;
    }
    encoded.flatten_depth = options.flatten_depth;
    encoded
}

fn map_decode_options(options: &SpecOptions) -> DecodeOptions {
    let mut decoded = DecodeOptions::default();
    if let Some(indent) = options.indent {
        decoded.indent = Indent::Spaces(indent);
    }
    if let Some(strict) = options.strict {
        decoded.strict = strict;
    }
    if let Some(ExpandPaths::Safe) = options.expand_paths {
        decoded.expand_paths = ToonExpandPaths::Safe;
    }
    decoded
}

#[rstest]
#[case(json!({"a": 1}), Some("a: 1"), SpecOptions::default())]
#[case(json!({"a": {"b": 1}}), Some("a:\n  b: 1"), SpecOptions::default())]
#[case(json!({"items": [1, 2]}), Some("items[2]: 1,2"), SpecOptions::default())]
#[case(
    json!({"items": [[1, 2]]}),
    Some("items[1]:\n  - [2]: 1,2"),
    SpecOptions::default()
)]
#[case(
    json!({"items": [{"a": 1}, {"a": 2}]}),
    Some("items[2]{a}:\n  1\n  2"),
    SpecOptions::default()
)]
#[case(
    json!({"items": [{"a": 1}, "x"]}),
    Some("items[2]:\n  - a: 1\n  - x"),
    SpecOptions::default()
)]
fn spec05_concrete_syntax_encode(
    #[case] input: Value,
    #[case] expected: Option<&'static str>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec05Adapter::encode(&input, &options)
                .unwrap_or_else(|err| panic!("encode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec05Adapter::encode(&input, &options).is_err());
        }
    }
}

#[rstest]
#[case("[2]: 1,2", Some(json!([1, 2])), SpecOptions::default())]
#[case("value", Some(json!("value")), SpecOptions::default())]
#[case("42", Some(json!(42)), SpecOptions::default())]
#[case("true", Some(json!(true)), SpecOptions::default())]
#[case("hello\nworld", Some(json!({"hello": null, "world": null})), SpecOptions::default().with_strict(false))]
#[case("a: 1\nb: 2", Some(json!({"a": 1, "b": 2})), SpecOptions::default())]
#[case("\n\n", Some(json!({})), SpecOptions::default())]
#[case("hello\nworld", None, SpecOptions::default().with_strict(true))]
fn spec05_concrete_syntax_decode(
    #[case] input: &str,
    #[case] expected: Option<Value>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec05Adapter::decode(input, &options)
                .unwrap_or_else(|err| panic!("decode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec05Adapter::decode(input, &options).is_err());
        }
    }
}

#[rstest]
#[case("[2]: 1,2", true)]
#[case("hello\nworld", false)]
fn spec05_concrete_syntax_validate(#[case] input: &str, #[case] valid: bool) {
    let result = Spec05Adapter::validate(input);
    if valid {
        assert!(result.is_ok());
    } else {
        assert!(result.is_err());
    }
}
