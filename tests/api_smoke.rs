use serde_json::json;

#[test]
fn encode_helpers_write_expected_toon() {
    let value = json!({"items": [1, 2]});
    let expected = "items[2]: 1,2";

    let bytes = serde_toon::to_vec(&value).expect("to_vec");
    assert_eq!(std::str::from_utf8(&bytes).expect("utf-8"), expected);

    let mut buffer = Vec::new();
    serde_toon::to_writer(&mut buffer, &value).expect("to_writer");
    assert_eq!(std::str::from_utf8(&buffer).expect("utf-8"), expected);

    let mut out = String::new();
    serde_toon::to_string_into(&value, &mut out).expect("to_string_into");
    assert_eq!(out, expected);
}

#[test]
fn decode_helpers_read_expected_value() {
    let input = "items[2]: 1,2";
    let expected = json!({"items": [1, 2]});

    let from_slice: serde_json::Value =
        serde_toon::from_slice(input.as_bytes()).expect("from_slice");
    assert_eq!(from_slice, expected);

    let cursor = std::io::Cursor::new(input.as_bytes());
    let from_reader: serde_json::Value = serde_toon::from_reader(cursor).expect("from_reader");
    assert_eq!(from_reader, expected);
}

#[test]
fn decode_to_value_auto_matches_json_and_toon() {
    let json_input = r#"{"items":[1,2]}"#;
    let json_expected = serde_json::from_str::<serde_json::Value>(json_input).expect("json parse");
    let auto_json = serde_toon::decode_to_value_auto(json_input).expect("auto json");
    assert_eq!(auto_json, json_expected);

    let toon_input = "items[2]: 1,2";
    let auto_toon = serde_toon::decode_to_value_auto(toon_input).expect("auto toon");
    let direct_toon = serde_toon::decode_to_value(toon_input).expect("decode toon");
    assert_eq!(auto_toon, direct_toon);
}
