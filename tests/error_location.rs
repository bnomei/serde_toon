use serde_json::Value;

fn expected_line_col(input: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (idx, byte) in input.as_bytes().iter().enumerate() {
        if idx == offset {
            break;
        }
        if *byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn assert_location(input: &str, offset: usize, err: serde_toon::Error) {
    let loc = err.location.expect("location");
    assert_eq!(loc.offset, offset);
    let (line, column) = expected_line_col(input, offset);
    assert_eq!(loc.line, line);
    assert_eq!(loc.column, column);
}

fn assert_streaming_location(input: &str, offset: usize) {
    let cursor = std::io::Cursor::new(input.as_bytes());
    let err = serde_toon::from_reader_streaming_with_options::<serde_json::Value, _>(
        cursor,
        &serde_toon::DecodeOptions::new(),
    )
    .unwrap_err();
    assert_location(input, offset, err);
}

#[test]
fn error_locations_value_decoder() {
    let input = "name: \"unterminated";
    let offset = input.find('"').expect("quote");
    let err = serde_toon::decode_to_value(input).unwrap_err();
    assert_location(input, offset, err);
    assert_streaming_location(input, offset);

    let input = "a:\n   b: 1";
    let offset = input.find('\n').expect("newline") + 1;
    let err = serde_toon::decode_to_value(input).unwrap_err();
    assert_location(input, offset, err);
    assert_streaming_location(input, offset);

    let input = "[1]: 1\nextra: 2";
    let offset = input.find("extra").expect("extra");
    let err = serde_toon::decode_to_value(input).unwrap_err();
    assert_location(input, offset, err);

    let input = "items[1]:\n  - name: \"bad\\q\"";
    let offset = input.find('"').expect("quote");
    let err = serde_toon::decode_to_value(input).unwrap_err();
    assert_location(input, offset, err);
}

#[test]
fn error_locations_arena_decoder() {
    let input = "name: \"unterminated";
    let offset = input.find('"').expect("quote");
    let err = serde_toon::from_str::<Value>(input).unwrap_err();
    assert_location(input, offset, err);

    let input = "a:\n   b: 1";
    let offset = input.find('\n').expect("newline") + 1;
    let err = serde_toon::from_str::<Value>(input).unwrap_err();
    assert_location(input, offset, err);

    let input = "[1]: 1\nextra: 2";
    let offset = input.find("extra").expect("extra");
    let err = serde_toon::from_str::<Value>(input).unwrap_err();
    assert_location(input, offset, err);
}

#[test]
fn error_locations_streaming_decoder() {
    let input = "name: \"unterminated";
    let offset = input.find('"').expect("quote");
    assert_streaming_location(input, offset);

    let input = "a:\n   b: 1";
    let offset = input.find('\n').expect("newline") + 1;
    assert_streaming_location(input, offset);

    let input = "[1]: 1\nextra: 2";
    let offset = input.find("extra").expect("extra");
    assert_streaming_location(input, offset);
}
