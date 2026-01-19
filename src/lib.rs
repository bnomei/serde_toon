pub mod arena;
pub mod canonical;
pub mod decode;
pub mod encode;
pub mod error;
pub mod num;
pub mod options;
pub mod tabular;
pub mod text;

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

pub use crate::error::{Error, ErrorKind, ErrorStage, Location};
pub use crate::options::{
    DecodeOptions, Delimiter, EncodeOptions, ExpandPaths, Indent, KeyFolding,
};
pub use canonical::{encode_canonical, CanonicalProfile};

pub type Result<T> = std::result::Result<T, Error>;

pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
    to_string_with_options(value, &EncodeOptions::default())
}

pub fn to_string_with_options<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<String> {
    encode::to_string(value, options)
}

pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_vec_with_options(value, &EncodeOptions::default())
}

pub fn to_vec_with_options<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<Vec<u8>> {
    encode::to_vec(value, options)
}

pub fn to_writer<T: Serialize, W: Write>(writer: W, value: &T) -> Result<()> {
    to_writer_with_options(writer, value, &EncodeOptions::default())
}

pub fn to_writer_with_options<T: Serialize, W: Write>(
    writer: W,
    value: &T,
    options: &EncodeOptions,
) -> Result<()> {
    encode::to_writer(writer, value, options)
}

pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
    from_str_with_options(input, &DecodeOptions::default())
}

pub fn from_str_with_options<T: DeserializeOwned>(
    input: &str,
    options: &DecodeOptions,
) -> Result<T> {
    decode::from_str(input, options)
}

pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
    from_slice_with_options(input, &DecodeOptions::default())
}

pub fn from_slice_with_options<T: DeserializeOwned>(
    input: &[u8],
    options: &DecodeOptions,
) -> Result<T> {
    decode::from_slice(input, options)
}

pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T> {
    from_reader_with_options(reader, &DecodeOptions::default())
}

pub fn from_reader_with_options<T: DeserializeOwned, R: Read>(
    reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    decode::from_reader(reader, options)
}

pub fn decode_to_value(input: &str) -> Result<Value> {
    decode_to_value_with_options(input, &DecodeOptions::default())
}

pub fn decode_to_value_with_options(input: &str, options: &DecodeOptions) -> Result<Value> {
    let value = decode::from_str(input, options)?;
    Ok(canonicalize_numbers(value))
}

fn canonicalize_numbers(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_numbers).collect()),
        Value::Object(map) => {
            let mapped = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize_numbers(value)))
                .collect();
            Value::Object(mapped)
        }
        Value::Number(number) => {
            let canonical = crate::num::number::format_json_number(&number);
            match serde_json::from_str::<Value>(&canonical) {
                Ok(Value::Number(number)) => Value::Number(number),
                _ => Value::Number(number),
            }
        }
        other => other,
    }
}

pub fn validate_str(input: &str) -> Result<()> {
    validate_str_with_options(input, &DecodeOptions::default())
}

pub fn validate_str_with_options(input: &str, options: &DecodeOptions) -> Result<()> {
    decode::validate_str(input, options)
}
