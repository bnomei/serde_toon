use serde_json::json;
use std::io::{BufReader, Cursor};

#[test]
fn from_reader_matches_from_str_output() {
    let items: Vec<_> = (0..512)
        .map(|idx| {
            json!({
                "id": idx,
                "label": format!("Item {idx}"),
                "active": idx % 2 == 0,
                "score": idx as f64 / 10.0,
            })
        })
        .collect();
    let value = json!({
        "items": items,
        "meta": {
            "count": 512,
            "source": "streaming",
        }
    });
    let toon = serde_toon::to_string_with_options(&value, &serde_toon::EncodeOptions::default())
        .expect("to_string_with_options");
    let reader = BufReader::new(Cursor::new(toon.as_bytes()));
    let from_reader: serde_json::Value = serde_toon::from_reader_streaming_with_options(
        reader,
        &serde_toon::DecodeOptions::default(),
    )
    .expect("from_reader_streaming_with_options");
    let from_str: serde_json::Value =
        serde_toon::from_str_with_options(&toon, &serde_toon::DecodeOptions::default())
            .expect("from_str_with_options");
    assert_eq!(from_reader, from_str);
}
