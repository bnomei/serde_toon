use std::io::Cursor;

use serde_json::json;

#[test]
fn from_reader_fastpath_matches_from_str() {
    let input = "name: Ada\nage: 37";
    let cursor = Cursor::new(input.as_bytes());
    let from_reader: serde_json::Value = serde_toon::from_reader(cursor).expect("from_reader");
    let from_str: serde_json::Value = serde_toon::from_str(input).expect("from_str");
    assert_eq!(from_reader, from_str);
    assert_eq!(from_reader, json!({"name": "Ada", "age": 37}));
}

#[test]
fn from_reader_streaming_matches_from_str_for_large_input() {
    let large = "a".repeat(512 * 1024);
    let input = format!("name: \"{large}\"");
    let cursor = Cursor::new(input.as_bytes());
    let from_reader: serde_json::Value = serde_toon::from_reader(cursor).expect("from_reader");
    let from_str: serde_json::Value = serde_toon::from_str(&input).expect("from_str");
    assert_eq!(from_reader, from_str);
}
