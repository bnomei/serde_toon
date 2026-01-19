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
struct Spec06Adapter;

impl Spec06Adapter {
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
#[case(json!([1, 2]), Some("[2]: 1,2"), SpecOptions::default())]
#[case(json!({"items": [1, 2]}), Some("items[2]: 1,2"), SpecOptions::default())]
#[case(
    json!({"items": [{"a": 1, "b": 2}]}),
    Some("items[1]{a,b}:\n  1,2"),
    SpecOptions::default()
)]
#[case(
    json!({"items": ["a", "b"]}),
    Some("items[2|]: a|b"),
    SpecOptions::default().with_delimiter('|')
)]
#[case(
    json!({"items": [{"a": 1, "b": 2}]}),
    Some("items[1|]{a|b}:\n  1|2"),
    SpecOptions::default().with_delimiter('|')
)]
#[case(
    json!({"items": [{"a-b": 1, "b": 2}]}),
    Some("items[1]{\"a-b\",b}:\n  1,2"),
    SpecOptions::default()
)]
fn spec06_header_syntax_encode(
    #[case] input: Value,
    #[case] expected: Option<&'static str>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec06Adapter::encode(&input, &options)
                .unwrap_or_else(|err| panic!("encode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec06Adapter::encode(&input, &options).is_err());
        }
    }
}

#[rstest]
#[case("items[2]: 1,2", Some(json!({"items": [1, 2]})), SpecOptions::default())]
#[case("items[2|]: a|b", Some(json!({"items": ["a", "b"]})), SpecOptions::default())]
#[case(
    "items[1|]{a|b}:\n  1|2",
    Some(json!({"items": [{"a": 1, "b": 2}]})),
    SpecOptions::default()
)]
#[case(
    "items[1]{\"a-b\",b}:\n  1,2",
    Some(json!({"items": [{"a-b": 1, "b": 2}]})),
    SpecOptions::default()
)]
#[case("items[0]:", Some(json!({"items": []})), SpecOptions::default())]
#[case("items[-1]:", None, SpecOptions::default().with_strict(true))]
#[case("items[1]{a|b}:\n  1,2", None, SpecOptions::default().with_strict(true))]
#[case("items[1]", None, SpecOptions::default().with_strict(true))]
#[case("items[1]{a,b}", None, SpecOptions::default().with_strict(true))]
fn spec06_header_syntax_decode(
    #[case] input: &str,
    #[case] expected: Option<Value>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec06Adapter::decode(input, &options)
                .unwrap_or_else(|err| panic!("decode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec06Adapter::decode(input, &options).is_err());
        }
    }
}

#[rstest]
#[case("items[1]: 1", true)]
#[case("items[1]", false)]
#[case("items[1]{a|b}:", false)]
#[case("items[-1]:", false)]
#[case("items[1]{a,b}", false)]
fn spec06_header_syntax_validate(#[case] input: &str, #[case] valid: bool) {
    let result = Spec06Adapter::validate(input);
    if valid {
        assert!(result.is_ok());
    } else {
        assert!(result.is_err());
    }
}
