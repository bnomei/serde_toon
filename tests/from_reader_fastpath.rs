use std::io::Cursor;

#[test]
fn from_reader_matches_streaming_for_small_input() {
    let input = "name: Ada\nage: 37";
    let cursor = Cursor::new(input.as_bytes());
    let from_reader: serde_json::Value = serde_toon::from_reader(cursor).expect("from_reader");
    let cursor = Cursor::new(input.as_bytes());
    let from_streaming: serde_json::Value =
        serde_toon::from_reader_streaming(cursor).expect("from_reader_streaming");
    assert_eq!(from_reader, from_streaming);
}

#[test]
fn from_reader_matches_streaming_for_large_input() {
    let large = "a".repeat(512 * 1024);
    let input = format!("name: \"{large}\"");
    let cursor = Cursor::new(input.as_bytes());
    let from_reader: serde_json::Value = serde_toon::from_reader(cursor).expect("from_reader");
    let cursor = Cursor::new(input.as_bytes());
    let from_streaming: serde_json::Value =
        serde_toon::from_reader_streaming(cursor).expect("from_reader_streaming");
    assert_eq!(from_reader, from_streaming);
}
