use serde_json::Value;

#[test]
fn missing_array_payload_reports_message_in_value_decoder() {
    let err = serde_toon::decode_to_value("[2]:").unwrap_err();
    assert_eq!(err.message, "array payload required");
}

#[test]
fn missing_array_payload_reports_message_in_arena_decoder() {
    let err = serde_toon::from_str::<Value>("[2]:").unwrap_err();
    assert_eq!(err.message, "array payload required");
}
