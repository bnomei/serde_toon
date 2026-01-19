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
struct Spec13Adapter;

impl Spec13Adapter {
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
#[case(json!({"a": 1, "b": 2}), Some("a: 1\nb: 2"), SpecOptions::default())]
#[case(json!({"a": {"b": 1}}), Some("a:\n  b: 1"), SpecOptions::default())]
#[case(
    json!({"text": "a\nb\rc\t\"d\"\\e"}),
    Some("text: \"a\\nb\\rc\\t\\\"d\\\"\\\\e\""),
    SpecOptions::default()
)]
#[case(
    json!({"comma": "a,b", "colon": "a:b", "bracket": "a[b]"}),
    Some("comma: \"a,b\"\ncolon: \"a:b\"\nbracket: \"a[b]\""),
    SpecOptions::default()
)]
#[case(
    json!({"value": "a,b"}),
    Some("value: a,b"),
    SpecOptions::default().with_delimiter('|')
)]
#[case(json!({"items": [1, 2, 3]}), Some("items[3]: 1,2,3"), SpecOptions::default())]
#[case(json!({"b": 1, "a": 2}), Some("b: 1\na: 2"), SpecOptions::default())]
#[case(json!({"n": 1e6}), Some("n: 1000000"), SpecOptions::default())]
#[case(json!({"n": -0.0}), Some("n: 0"), SpecOptions::default())]
#[case(
    json!({"a": {"b": 1, "c": 2}}),
    Some("a:\n  b: 1\n  c: 2"),
    SpecOptions::default()
)]
#[case(
    json!({"a": {"b": {"c": 1}}}),
    Some("a.b.c: 1"),
    SpecOptions::default().with_key_folding_safe()
)]
#[case(
    json!({"a": {"b": 1}, "a.b": 2}),
    Some("a:\n  b: 1\na.b: 2"),
    SpecOptions::default().with_key_folding_safe()
)]
#[case(
    json!({"a": {"b": {"c": 1}}}),
    Some("a.b:\n  c: 1"),
    SpecOptions::default().with_key_folding_safe().with_flatten_depth(2)
)]
fn spec13_encoder_conformance(
    #[case] input: Value,
    #[case] expected: Option<&'static str>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec13Adapter::encode(&input, &options)
                .unwrap_or_else(|err| panic!("encode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec13Adapter::encode(&input, &options).is_err());
        }
    }
}

#[rstest]
#[case("items[2]: 1,2", Some(json!({"items": [1, 2]})), SpecOptions::default())]
#[case("items[2|]: a|b", Some(json!({"items": ["a", "b"]})), SpecOptions::default())]
#[case(
    "items[2]{a,b}:\n  - 1,2\n  - 3,4",
    Some(json!({"items": [{"a": 1, "b": 2}, {"a": 3, "b": 4}]})),
    SpecOptions::default()
)]
#[case(
    "items[2|]: a,b|c",
    Some(json!({"items": ["a,b", "c"]})),
    SpecOptions::default()
)]
#[case(
    "value: \"a\\nb\\rc\\td\\\"e\\\\f\"",
    Some(json!({"value": "a\nb\rc\td\"e\\f"})),
    SpecOptions::default()
)]
#[case("value: \"\\q\"", None, SpecOptions::default())]
#[case(
    "a: true\nb: false\nc: null\nd: 42\ne: 3.14\nf: hello",
    Some(json!({
        "a": true,
        "b": false,
        "c": null,
        "d": 42,
        "e": "3.14".parse::<f64>().unwrap(),
        "f": "hello"
    })),
    SpecOptions::default()
)]
#[case("items[2]: 1", None, SpecOptions::default().with_strict(true))]
#[case("b: 1\na: 2", Some(json!({"b": 1, "a": 2})), SpecOptions::default())]
#[case(
    "a.b: 1\na.c: 2",
    Some(json!({"a": {"b": 1, "c": 2}})),
    SpecOptions::default().with_expand_paths_safe()
)]
#[case(
    "a.b: 1\na: 2",
    None,
    SpecOptions::default().with_expand_paths_safe().with_strict(true)
)]
#[case(
    "a.b: 1\na: 2",
    Some(json!({"a": 2})),
    SpecOptions::default().with_expand_paths_safe().with_strict(false)
)]
fn spec13_decoder_conformance(
    #[case] input: &str,
    #[case] expected: Option<Value>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec13Adapter::decode(input, &options)
                .unwrap_or_else(|err| panic!("decode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec13Adapter::decode(input, &options).is_err());
        }
    }
}

#[rstest]
#[case("a: 1", true)]
#[case("a 1", false)]
#[case("a: 1 ", false)]
#[case("a: 1\n", false)]
#[case("items[2]: 1", false)]
#[case("items[1]{a,b}:\n  - 1", false)]
#[case("a:\n\tb: 1", false)]
#[case("items[2]:\n  - 1\n\n  - 2", false)]
#[case("items[2|]{a,b}:\n  - 1,2\n  - 3,4", false)]
#[case("value: \"\\q\"", false)]
fn spec13_validator_conformance(#[case] input: &str, #[case] valid: bool) {
    let result = Spec13Adapter::validate(input);
    if valid {
        assert!(result.is_ok());
    } else {
        assert!(result.is_err());
    }
}
