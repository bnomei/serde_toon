use rstest::rstest;
use serde_json::{json, Number, Value};
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
#[derive(Clone, Debug)]
enum SpecInput {
    Json(Value),
    HostNumber(HostNumber),
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum HostNumber {
    Finite(f64),
    NegZero,
    NaN,
    PosInf,
    NegInf,
}

#[allow(dead_code)]
struct Spec03Adapter;

impl Spec03Adapter {
    fn encode(_input: &SpecInput, _options: &SpecOptions) -> Result<String, String> {
        let options = map_encode_options(_options);
        let value = match _input {
            SpecInput::Json(value) => value.clone(),
            SpecInput::HostNumber(host) => match host {
                HostNumber::Finite(value) => Number::from_f64(*value)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                HostNumber::NegZero => Number::from_f64(-0.0)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                HostNumber::NaN | HostNumber::PosInf | HostNumber::NegInf => Value::Null,
            },
        };
        serde_toon::to_string_with_options(&value, &options).map_err(|err| err.to_string())
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
#[case(SpecInput::Json(json!(1e6)), Some("1000000"), SpecOptions::default())]
#[case(
    SpecInput::HostNumber(HostNumber::NegZero),
    Some("0"),
    SpecOptions::default()
)]
#[case(
    SpecInput::HostNumber(HostNumber::NaN),
    Some("null"),
    SpecOptions::default()
)]
#[case(
    SpecInput::HostNumber(HostNumber::PosInf),
    Some("null"),
    SpecOptions::default()
)]
#[case(
    SpecInput::HostNumber(HostNumber::NegInf),
    Some("null"),
    SpecOptions::default()
)]
fn spec03_encoding_normalization_encode(
    #[case] input: SpecInput,
    #[case] expected: Option<&'static str>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec03Adapter::encode(&input, &options)
                .unwrap_or_else(|err| panic!("encode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec03Adapter::encode(&input, &options).is_err());
        }
    }
}

#[rstest]
#[case("null", Some(json!(null)), SpecOptions::default())]
fn spec03_encoding_normalization_decode(
    #[case] input: &str,
    #[case] expected: Option<Value>,
    #[case] options: SpecOptions,
) {
    match expected {
        Some(expected) => {
            let actual = Spec03Adapter::decode(input, &options)
                .unwrap_or_else(|err| panic!("decode failed: {err}"));
            assert_eq!(actual, expected);
        }
        None => {
            assert!(Spec03Adapter::decode(input, &options).is_err());
        }
    }
}

#[rstest]
#[case("null", true)]
#[case("NaN", false)]
fn spec03_encoding_normalization_validate(#[case] input: &str, #[case] valid: bool) {
    let result = Spec03Adapter::validate(input);
    if valid {
        assert!(result.is_ok());
    } else {
        assert!(result.is_err());
    }
}
