use serde_json::json;
use serde_toon::{EncodeOptions, ErrorKind, Indent};

#[test]
fn encode_rejects_zero_indent() {
    let opts = EncodeOptions::new().with_indent(Indent::spaces(0));
    let err = serde_toon::to_string_with_options(&json!({"a": 1}), &opts).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Encode);
    assert_eq!(err.message, "indent size must be greater than zero");
}
