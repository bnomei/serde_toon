use serde_json::json;

#[test]
fn to_writer_matches_to_string_output() {
    let value = json!({
        "user": {
            "name": "Ada",
            "tags": ["math", "code"],
            "meta": {"active": true, "score": 9.5}
        },
        "items": [
            {"id": 1, "qty": 2},
            {"id": 2, "qty": 3}
        ]
    });
    let expected =
        serde_toon::to_string_with_options(&value, &serde_toon::EncodeOptions::default())
            .expect("to_string_with_options");

    let mut out = Vec::new();
    serde_toon::to_writer_with_options(&mut out, &value, &serde_toon::EncodeOptions::default())
        .expect("to_writer_with_options");
    let actual = String::from_utf8(out).expect("utf-8");
    assert_eq!(actual, expected);
}
