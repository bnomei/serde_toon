pub mod profile;

use serde_json::Value;

use crate::{encode, Delimiter, EncodeOptions, Indent, Result};

pub use profile::{CanonicalDelimiter, CanonicalProfile};

pub fn encode_canonical(value: &Value, profile: CanonicalProfile) -> Result<String> {
    let options = EncodeOptions {
        indent: Indent::Spaces(profile.indent_spaces),
        delimiter: match profile.delimiter {
            CanonicalDelimiter::Comma => Delimiter::Comma,
            CanonicalDelimiter::Tab => Delimiter::Tab,
            CanonicalDelimiter::Pipe => Delimiter::Pipe,
        },
        ..EncodeOptions::default()
    };
    encode::to_string(value, &options)
}
