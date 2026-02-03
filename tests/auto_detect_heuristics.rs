use serde_json::json;

#[test]
fn auto_detect_prefers_json_for_json_input() {
    let input = r#"{"name":"Ada","age":37}"#;
    let value =
        serde_toon::decode_to_value_auto_with_options(input, &serde_toon::DecodeOptions::default())
            .expect("decode_to_value_auto_with_options");
    assert_eq!(value, json!({"name": "Ada", "age": 37}));
}

#[test]
fn auto_detect_prefers_toon_for_toon_input() {
    let input = "name: Ada\nage: 37";
    let value =
        serde_toon::decode_to_value_auto_with_options(input, &serde_toon::DecodeOptions::default())
            .expect("decode_to_value_auto_with_options");
    assert_eq!(value, json!({"name": "Ada", "age": 37}));
}

#[test]
fn auto_detect_falls_back_on_ambiguous_input() {
    let input = "\"";
    let err =
        serde_toon::decode_to_value_auto_with_options(input, &serde_toon::DecodeOptions::default())
            .expect_err("expected error");
    let message = err.to_string();
    assert!(message.contains("input is neither valid JSON nor TOON"));
    assert!(message.contains("json error"));
    assert!(message.contains("toon error"));
}
