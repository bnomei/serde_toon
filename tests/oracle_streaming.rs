use std::io::{BufReader, Cursor};

use serde_json::{json, Value};

fn decode(input: &str, strict: bool) -> serde_toon::Result<Value> {
    let options = serde_toon::DecodeOptions {
        strict,
        ..Default::default()
    };
    let streamed = serde_toon::from_reader_streaming_with_options(
        BufReader::with_capacity(1, Cursor::new(input.as_bytes())),
        &options,
    );
    let arena = serde_toon::from_str_with_options::<Value>(input, &options);
    match (&streamed, &arena) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "API mismatch for {input:?}"),
        (Err(_), Err(_)) => {}
        _ => panic!("API mismatch for {input:?}: streaming {streamed:?}, arena {arena:?}"),
    }
    streamed
}

#[test]
fn numeric_tokens_use_the_exact_grammar() {
    assert_eq!(decode("1.0\t", true).unwrap(), json!("1.0\t"));
    for token in [".5", "1.", "+5", "1e", "1e+", "01", "-01"] {
        assert_eq!(decode(token, true).unwrap(), json!(token));
    }
    assert_eq!(decode("-1E+03", true).unwrap(), json!(-1000.0));
}

#[test]
fn strict_tabular_scope_does_not_synthetically_deindent() {
    assert!(decode("a[1]{x}:\n  1\n  b: 2", true).is_err());
}

#[test]
fn tabular_rows_preserve_hyphens_and_list_markers_are_exact() {
    assert_eq!(
        decode("[1]{x}:\n  - x", true).unwrap(),
        json!([{"x": "- x"}])
    );
    assert!(decode("[1]:\n  -1", false).is_err());
    assert_eq!(
        decode("[1]:\n  - \u{a0}x", true).unwrap(),
        json!(["\u{a0}x"])
    );
}

#[test]
fn keyed_first_fields_use_the_normative_depth_in_all_modes() {
    assert_eq!(
        decode("[2]:\n  - m[2:]{x}:\n      a: 1\n      b: 2\n  - 0", false).unwrap(),
        json!([{"m":{"a":{"x":1},"b":{"x":2}}}, 0])
    );
    assert_eq!(
        decode("[1]:\n  - m[0:]{x}:\n    s: 1", false).unwrap(),
        json!([{"m":{},"s":1}])
    );
}

#[test]
fn nonstrict_field_walk_is_recursive_and_validates_surplus_cells() {
    assert_eq!(
        decode("[1]{n{x},n{y}}:\n  1,2", false).unwrap(),
        json!([{"n":{"y":2}}])
    );
    assert_eq!(
        decode("[1]{a,n{x}}:\n  1", false).unwrap(),
        json!([{"a":1,"n":{}}])
    );
    assert!(decode("[1]{a}:\n  1,\"\\q\"", false).is_err());
    assert!(decode("[1]{a}:\n  1,\"ok\"junk", false).is_err());
}

#[test]
fn blank_lines_remain_visible_to_ancestor_header_scopes() {
    assert!(decode("[1]:\n  - a:\n      b: 1\n\n      c: 2", true).is_err());
    assert!(decode("[2]:\n  - [1]:\n    - 1\n\n  - 2", true).is_err());
    assert!(decode("[2]:\n  - a:\n      b: 1\n\n  - 2", true).is_err());
    assert_eq!(
        decode("[1]:\n  - a: 1\n\n    b: 2", false).unwrap(),
        json!([{"a":1,"b":2}])
    );
}

#[test]
fn root_scalars_and_invalid_multi_line_scalars() {
    assert_eq!(decode("hello world", true).unwrap(), json!("hello world"));
    assert!(decode("a: 1\nb", false).is_err());
    assert!(decode("hello\nworld", false).is_err());
    assert_eq!(decode("\u{a0}", true).unwrap(), json!("\u{a0}"));
}

#[test]
fn quoted_controls_and_header_spacing_are_rejected() {
    assert!(decode("\"\\u+041\"", false).is_err());
    assert!(decode("\"a\u{1}b\"", false).is_err());
    assert!(decode("[0]{x} :", true).is_err());
}

#[test]
fn huge_declared_lengths_fail_normally_and_tsv_empty_first_cell_works() {
    assert!(decode("[999999999999999999999999999999]:", true).is_err());
    assert!(decode("[18446744073709551615]:", true).is_err());
    assert!(decode("a:\n  \tb: 1", true).is_err());
    assert!(decode("[1]{x}:\n  \t1", true).is_err());
    assert_eq!(
        decode("[1\t]{a\tb}:\n  \t1", false).unwrap(),
        json!([{"a":"","b":1}])
    );
    assert_eq!(
        decode("[1\t]{a\tb}:\n  \t1", true).unwrap(),
        json!([{"a":"","b":1}])
    );
}

#[test]
fn list_payload_trims_spaces_before_classification() {
    for strict in [false, true] {
        assert_eq!(
            decode("[3]:\n  -  1\n  -  []\n  -  [1]: 2", strict).unwrap(),
            json!([1, [], [2]])
        );
        assert!(decode("[1]:\n  -  \"\\q\"", strict).is_err());
        assert_eq!(decode("[1]:\n  -  \t1", strict).unwrap(), json!(["\t1"]));
    }
}

#[test]
fn leading_blanks_in_nested_headers_belong_to_ancestors() {
    for input in [
        "[1]:\n  - [1]:\n\n    - 1",
        "[1]:\n  - a[0]{x}:\n\n    b: 1",
        "[1]:\n  - a[1:]{x}:\n\n      k: 1",
    ] {
        assert!(decode(input, true).is_err(), "{input:?}");
        assert!(decode(input, false).is_ok(), "{input:?}");
    }
    assert_eq!(decode("[1]:\n\n  - 1", true).unwrap(), json!([1]));
    assert_eq!(
        decode("a[1]:\n  - b[0]{x}:\n\nz: 2", true).unwrap(),
        json!({"a":[{"b":[]}],"z":2})
    );
}

#[test]
fn empty_tsv_rows_are_payload_not_blank_lines() {
    for strict in [false, true] {
        assert_eq!(
            decode("[1\t]{a\tb}:\n  \t", strict).unwrap(),
            json!([{"a":"","b":""}])
        );
        assert!(decode("a:\n  \t", strict).is_err());
        assert!(decode("  \t1", strict).is_err());
    }
    assert!(decode("[0\t]{a\tb}:\n  \t", true).is_err());
    assert_eq!(
        decode("[0\t]{a\tb}:\n  \t", false).unwrap(),
        json!([{"a":"","b":""}])
    );
}

#[test]
fn missing_header_colons_error_in_both_modes() {
    for input in ["a[1]", "[1]", "a[0]{x}", "[0:]{x}"] {
        for strict in [false, true] {
            assert!(decode(input, strict).is_err(), "{input:?}");
        }
    }
    assert_eq!(decode("\t#x", false).unwrap(), json!("#x"));
    assert!(decode("\t#x", true).is_err());
    for (input, expected) in [
        ("a[0]junk: 1", json!({"a[0]junk":1})),
        ("a[0]{x} : 1", json!({"a[0]{x}":1})),
    ] {
        assert_eq!(decode(input, false).unwrap(), expected);
        assert!(decode(input, true).is_err());
    }
}
