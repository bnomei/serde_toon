use serde_json::json;
use serde_toon::{Delimiter, EncodeOptions};

#[test]
fn encode_scalar_cache_is_stable() {
    let value = json!({
        "name": "Ada",
        "score": 42,
        "flags": [true, false],
        "tags": ["alpha", "beta"],
    });
    let options = EncodeOptions::new();
    let first = serde_toon::to_string_with_options(&value, &options).unwrap();
    let second = serde_toon::to_string_with_options(&value, &options).unwrap();
    assert_eq!(first, second);
}

#[test]
fn encode_scalar_cache_respects_delimiter() {
    let value = json!(["a,b"]);
    let comma = EncodeOptions::new();
    let comma_out = serde_toon::to_string_with_options(&value, &comma).unwrap();
    assert_eq!(comma_out, "[1]: \"a,b\"");

    let pipe = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
    let pipe_out = serde_toon::to_string_with_options(&value, &pipe).unwrap();
    assert_eq!(pipe_out, "[1|]: a,b");
}
