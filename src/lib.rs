//! Serde-compatible TOON v3.0 encoder/decoder with optional v1.5 features.
//!
//! # Examples
//!
//! Quick encode/decode:
//!
//! ```rust
//! use serde::Serialize;
//! use serde_toon::toon;
//!
//! #[derive(Serialize)]
//! struct User {
//!     name: String,
//!     age: u32,
//! }
//!
//! let user = User {
//!     name: "Ada Lovelace".to_string(),
//!     age: 37,
//! };
//! let toon = toon!(encode: user)?;
//! let toon_from_json = toon!(encode_json: r#"{"name":"Grace Hopper"}"#)?;
//! let value = toon!("name: Ada Lovelace")?;
//! assert_eq!(toon, "name: Ada Lovelace\nage: 37");
//! assert_eq!(toon_from_json, "name: Grace Hopper");
//! assert_eq!(value, serde_json::json!({"name": "Ada Lovelace"}));
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! Encode to TOON:
//!
//! ```rust
//! use serde::{Deserialize, Serialize};
//! use serde_toon::to_string;
//!
//! #[derive(Debug, Serialize, Deserialize, PartialEq)]
//! struct User {
//!     name: String,
//!     age: u32,
//! }
//!
//! let user = User {
//!     name: "Ada Lovelace".to_string(),
//!     age: 37,
//! };
//!
//! let toon = to_string(&user)?;
//! assert_eq!(toon, "name: Ada Lovelace\nage: 37");
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! Decode back:
//!
//! ```rust
//! use serde::Deserialize;
//! use serde_toon::from_str;
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! struct User {
//!     name: String,
//!     age: u32,
//! }
//!
//! let toon = "name: Ada Lovelace\nage: 37";
//! let round_trip: User = from_str(toon)?;
//! assert_eq!(
//!     round_trip,
//!     User {
//!         name: "Ada Lovelace".to_string(),
//!         age: 37
//!     }
//! );
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! JSON string round-trip:
//!
//! ```rust
//! use serde_toon::{from_str, to_string_from_json_str};
//!
//! let json = r#"{"name":"Grace Hopper","field":"computer science","year":1952}"#;
//! let toon = to_string_from_json_str(json)?;
//! assert_eq!(
//!     toon,
//!     "name: Grace Hopper\nfield: computer science\nyear: 1952"
//! );
//!
//! let back_to_json = serde_json::to_string(&from_str::<serde_json::Value>(&toon)?)?;
//! assert_eq!(back_to_json, json);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Untyped values:
//!
//! ```rust
//! use serde_toon::Value;
//!
//! let value: Value = serde_toon::from_str("name: Margaret Hamilton\nage: 32")?;
//! assert_eq!(value, serde_json::json!({"name": "Margaret Hamilton", "age": 32}));
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! Custom options:
//!
//! ```rust
//! use serde_toon::{Delimiter, EncodeOptions, Indent, KeyFolding};
//!
//! let opts = EncodeOptions::new()
//!     .with_indent(Indent::spaces(4))
//!     .with_delimiter(Delimiter::Pipe)
//!     .with_key_folding(KeyFolding::Safe)
//!     .with_flatten_depth(Some(2));
//! let toon = serde_toon::to_string_with_options(&serde_json::json!({"items": ["a", "b"]}), &opts)?;
//! assert_eq!(toon, "items[2|]: a|b");
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! ```rust
//! use serde_toon::{DecodeOptions, ExpandPaths, Indent};
//!
//! let opts = DecodeOptions::new()
//!     .with_indent(Indent::spaces(4))
//!     .with_strict(false)
//!     .with_expand_paths(ExpandPaths::Safe);
//! let value: serde_json::Value = serde_toon::from_str_with_options("a.b: 1", &opts)?;
//! assert_eq!(value, serde_json::json!({"a": {"b": 1}}));
//! # Ok::<(), serde_toon::Error>(())
//! ```
//!
//! # Removed APIs
//!
//! ```compile_fail
//! use serde_toon::tabular;
//! ```

pub mod arena;
pub mod canonical;
pub mod decode;
pub mod encode;
pub mod error;
pub mod num;
pub mod options;
pub mod text;

use std::io::{BufRead, Read, Write};

pub use crate::error::{Error, ErrorKind, ErrorStage, Location};
pub use crate::options::{
    DecodeOptions, Delimiter, EncodeOptions, ExpandPaths, Indent, KeyFolding,
};
pub use canonical::{encode_canonical, CanonicalProfile};
use serde::de::DeserializeOwned;
use serde::Serialize;
pub use serde_json::Value;

pub type Result<T> = std::result::Result<T, Error>;

pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
    to_string_with_options(value, &EncodeOptions::default())
}

pub fn to_string_with_options<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<String> {
    encode::to_string(value, options)
}

pub fn to_string_into<T: Serialize>(value: &T, out: &mut String) -> Result<()> {
    to_string_into_with_options(value, &EncodeOptions::default(), out)
}

pub fn to_string_into_with_options<T: Serialize>(
    value: &T,
    options: &EncodeOptions,
    out: &mut String,
) -> Result<()> {
    encode::to_string_into(value, options, out)
}

pub fn to_string_from_json_str(input: &str) -> Result<String> {
    to_string_from_json_str_with_options(input, &EncodeOptions::default())
}

pub fn to_string_from_json_str_with_options(
    input: &str,
    options: &EncodeOptions,
) -> Result<String> {
    encode::to_string_from_json_str(input, options)
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

#[cfg(feature = "parallel")]
pub fn from_str_parallel<T: DeserializeOwned + Send>(input: &str) -> Result<Vec<T>> {
    from_str_parallel_with_options(input, &DecodeOptions::default())
}

#[cfg(feature = "parallel")]
pub fn from_str_parallel_with_options<T: DeserializeOwned + Send>(
    input: &str,
    options: &DecodeOptions,
) -> Result<Vec<T>> {
    decode::from_str_parallel(input, options)
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

/// Decode a value from a reader by buffering the entire input into memory.
///
/// `from_reader` reads all bytes from `reader` into a `Vec<u8>` before decoding, so
/// large or untrusted inputs can exhaust memory. For incremental, bounded-memory
/// processing, prefer [`from_reader_streaming`] or
/// [`from_reader_streaming_with_options`]. See [`DecodeOptions`] for alternate
/// decoding behavior.
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T> {
    from_reader_with_options(reader, &DecodeOptions::default())
}

/// Decode a value from a reader with explicit [`DecodeOptions`].
///
/// This buffers the entire input into memory, so large or untrusted inputs can
/// exhaust memory. If you need incremental, bounded-memory processing, prefer
/// [`from_reader_streaming_with_options`] (or [`from_reader_streaming`] with
/// defaults).
pub fn from_reader_with_options<T: DeserializeOwned, R: Read>(
    reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    decode::from_reader(reader, options)
}

pub fn from_reader_streaming<T: DeserializeOwned, R: BufRead>(reader: R) -> Result<T> {
    from_reader_streaming_with_options(reader, &DecodeOptions::default())
}

pub fn from_reader_streaming_with_options<T: DeserializeOwned, R: BufRead>(
    reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    decode::from_reader_streaming(reader, options)
}

pub fn decode_to_value(input: &str) -> Result<Value> {
    decode_to_value_with_options(input, &DecodeOptions::default())
}

pub fn decode_to_value_with_options(input: &str, options: &DecodeOptions) -> Result<Value> {
    let value = decode::from_str_value(input, options)?;
    Ok(canonicalize_numbers(value))
}

pub fn decode_to_value_auto<S: AsRef<str>>(input: S) -> Result<Value> {
    decode_to_value_auto_with_options(input, &DecodeOptions::default())
}

pub fn decode_to_value_auto_with_options<S: AsRef<str>>(
    input: S,
    options: &DecodeOptions,
) -> Result<Value> {
    let input = input.as_ref();
    match detect_auto_kind(input) {
        AutoDetectKind::Json | AutoDetectKind::Uncertain => {
            match serde_json::from_str::<Value>(input) {
                Ok(value) => Ok(canonicalize_numbers(value)),
                Err(json_err) => match decode_to_value_with_options(input, options) {
                    Ok(value) => Ok(value),
                    Err(toon_err) => Err(auto_detect_error(json_err, toon_err)),
                },
            }
        }
        AutoDetectKind::Toon => match decode_to_value_with_options(input, options) {
            Ok(value) => Ok(value),
            Err(toon_err) => match serde_json::from_str::<Value>(input) {
                Ok(value) => Ok(canonicalize_numbers(value)),
                Err(json_err) => Err(auto_detect_error(json_err, toon_err)),
            },
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoDetectKind {
    Json,
    Toon,
    Uncertain,
}

fn detect_auto_kind(input: &str) -> AutoDetectKind {
    let first_non_ws = input.chars().find(|ch| !ch.is_whitespace());
    let toon_key_pos = find_toon_key_token(input);
    if let Some(ch) = first_non_ws {
        if ch == '{' || ch == '[' {
            if let Some(json_key_pos) = find_json_quoted_key(input) {
                if toon_key_pos.is_none() || Some(json_key_pos) < toon_key_pos {
                    return AutoDetectKind::Json;
                }
            }
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            return AutoDetectKind::Toon;
        }
    } else {
        return AutoDetectKind::Uncertain;
    }
    if toon_key_pos.is_some() {
        return AutoDetectKind::Toon;
    }
    AutoDetectKind::Uncertain
}

fn find_json_quoted_key(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'"' {
            let start = idx;
            idx += 1;
            let mut escape = false;
            while idx < bytes.len() {
                let byte = bytes[idx];
                if escape {
                    escape = false;
                    idx += 1;
                    continue;
                }
                if byte == b'\\' {
                    escape = true;
                    idx += 1;
                    continue;
                }
                if byte == b'"' {
                    idx += 1;
                    break;
                }
                idx += 1;
            }
            if idx >= bytes.len() {
                break;
            }
            let mut lookahead = idx;
            while lookahead < bytes.len()
                && matches!(bytes[lookahead], b' ' | b'\t' | b'\r' | b'\n')
            {
                lookahead += 1;
            }
            if lookahead < bytes.len() && bytes[lookahead] == b':' {
                return Some(start);
            }
            idx = lookahead;
            continue;
        }
        idx += 1;
    }
    None
}

fn find_toon_key_token(input: &str) -> Option<usize> {
    let mut offset = 0;
    for line in input.split_terminator('\n') {
        let bytes = line.as_bytes();
        let mut idx = 0;
        while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t' | b'\r') {
            idx += 1;
        }
        if idx < bytes.len() && is_ident_start_byte(bytes[idx]) {
            let start = offset + idx;
            idx += 1;
            while idx < bytes.len() && is_ident_continue_byte(bytes[idx]) {
                idx += 1;
            }
            while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t') {
                idx += 1;
            }
            if idx < bytes.len() && bytes[idx] == b':' {
                return Some(start);
            }
        }
        offset += line.len() + 1;
    }
    None
}

fn is_ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

fn auto_detect_error(json_err: serde_json::Error, toon_err: Error) -> Error {
    Error::decode(format!(
        "input is neither valid JSON nor TOON: json error: {json_err}; toon error: {toon_err}"
    ))
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

#[macro_export]
/// Parse a JSON or TOON string into a `serde_json::Value`, or encode values into TOON.
///
/// This macro calls `decode_to_value_auto`, returning a `Result<Value>`.
///
/// # Examples
///
/// ```rust
/// use serde_toon::toon;
///
/// let value = toon!("name: \"Snoopy\"\nage: 5")?;
/// assert_eq!(value, serde_json::json!({"name": "Snoopy", "age": 5}));
/// # Ok::<(), serde_toon::Error>(())
/// ```
///
/// ```rust
/// use serde_toon::toon;
///
/// let toon = toon!(encode_json: r#"{"name":"Grace Hopper"}"#)?;
/// assert_eq!(toon, "name: Grace Hopper");
/// # Ok::<(), serde_toon::Error>(())
/// ```
macro_rules! toon {
    (encode: $input:expr) => {
        $crate::to_string(&$input)
    };
    (encode: $input:expr, $options:expr) => {
        $crate::to_string_with_options(&$input, $options)
    };
    (encode_json: $input:expr) => {
        $crate::to_string_from_json_str($input)
    };
    (encode_json: $input:expr, $options:expr) => {
        $crate::to_string_from_json_str_with_options($input, $options)
    };
    ($input:expr) => {
        $crate::decode_to_value_auto($input)
    };
    ($input:expr, $options:expr) => {
        $crate::decode_to_value_auto_with_options($input, $options)
    };
}
