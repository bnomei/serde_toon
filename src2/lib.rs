//! Experimental canonical fast path (src2).

pub mod canonical;
pub mod parallel;

use serde::de::DeserializeOwned;

#[derive(Debug)]
pub enum ToonError {
    DeserializationError(String),
    SerializationError(String),
    InvalidInput(String),
}

impl std::fmt::Display for ToonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToonError::DeserializationError(message) => write!(f, "{}", message),
            ToonError::SerializationError(message) => write!(f, "{}", message),
            ToonError::InvalidInput(message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for ToonError {}

pub type ToonResult<T> = Result<T, ToonError>;

pub use canonical::arena::{ArenaView, NodeKind};
pub use canonical::decode::{
    decode_and_parse_view, decode_canonical, decode_to_value, validate_canonical,
    validate_passthrough,
};
pub use canonical::encode::encode_canonical;
pub use canonical::profile::CanonicalProfile;
pub use canonical::serde::decode_from_arena_view;

pub fn to_string<T: serde::Serialize>(value: &T) -> ToonResult<String> {
    encode_canonical(value, CanonicalProfile::default())
}

pub fn from_str<T: DeserializeOwned>(input: &str) -> ToonResult<T> {
    let arena = decode_and_parse_view(input).map_err(|err| ToonError::InvalidInput(err.message))?;
    decode_from_arena_view(&arena)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct User {
        name: String,
        age: u32,
    }

    #[rstest::rstest]
    fn test_round_trip_string_api() {
        let user = User {
            name: "Ada".to_string(),
            age: 37,
        };
        let toon = to_string(&user).unwrap();
        assert_eq!(toon, "age: 37\nname: Ada");
        let round_trip: User = from_str(&toon).unwrap();
        assert_eq!(user, round_trip);
    }

    #[rstest::rstest]
    fn test_untyped_value() {
        let value: serde_json::Value = from_str("age: 32\nname: Margaret").unwrap();
        assert_eq!(value, serde_json::json!({"age": 32, "name": "Margaret"}));
    }

    #[rstest::rstest]
    fn test_inline_array_header() {
        let value: serde_json::Value = from_str("items[3]: 1, 2, 3").unwrap();
        assert_eq!(value, serde_json::json!({"items": [1, 2, 3]}));
    }

    #[rstest::rstest]
    fn test_team_round_trip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Team {
            id: u64,
            name: String,
            users: Vec<User>,
        }

        let team = Team {
            id: 42,
            name: "ops".to_string(),
            users: vec![
                User {
                    name: "Ada".to_string(),
                    age: 37,
                },
                User {
                    name: "Grace".to_string(),
                    age: 60,
                },
            ],
        };

        let toon = to_string(&team).unwrap();
        let round_trip: Team = from_str(&toon).unwrap();
        assert_eq!(team, round_trip);
    }

    #[rstest::rstest]
    fn test_root_array_round_trip() {
        let users = vec![
            User {
                name: "Ada".to_string(),
                age: 37,
            },
            User {
                name: "Grace".to_string(),
                age: 60,
            },
        ];
        let toon = to_string(&users).unwrap();
        let round_trip: Vec<User> = from_str(&toon).unwrap();
        assert_eq!(users, round_trip);
    }

    #[rstest::rstest]
    fn test_root_array_inline_primitives() {
        let values: Vec<String> = from_str("[3]: a, b, c").unwrap();
        assert_eq!(
            values,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[rstest::rstest]
    fn test_root_array_tabular() {
        let value: serde_json::Value = from_str("[2]{id}:\n  1\n  2").unwrap();
        assert_eq!(value, serde_json::json!([{"id": 1}, {"id": 2}]));
    }

    #[rstest::rstest]
    fn test_root_array_requires_header() {
        let err = validate_canonical("- a\n- b").unwrap_err();
        assert!(err.message.contains("root array"));
    }

    #[rstest::rstest]
    fn test_tabular_array_encode_decode() {
        let value = serde_json::json!({
            "rows": [
                {"id": 1, "name": "Ada"},
                {"id": 2, "name": "Bob"}
            ]
        });
        let toon = to_string(&value).unwrap();
        assert!(toon.contains("rows[2]{id,name}:"));
        let decoded: serde_json::Value = from_str(&toon).unwrap();
        assert_eq!(decoded, value);
    }

    #[rstest::rstest]
    fn test_delimiter_parsing_tab_and_pipe() {
        let value: serde_json::Value = from_str("tags[3\t]: reading\tgaming\tcoding").unwrap();
        assert_eq!(
            value,
            serde_json::json!({"tags": ["reading", "gaming", "coding"]})
        );

        let value: serde_json::Value = from_str("tags[3|]: reading|gaming|coding").unwrap();
        assert_eq!(
            value,
            serde_json::json!({"tags": ["reading", "gaming", "coding"]})
        );
    }

    #[rstest::rstest]
    fn test_tabular_header_with_quoted_field() {
        let value: serde_json::Value = from_str("items[2|]{\"a|b\"}:\n  1\n  2").unwrap();
        assert_eq!(
            value,
            serde_json::json!({"items": [{"a|b": 1}, {"a|b": 2}]})
        );
    }

    #[rstest::rstest]
    fn test_list_item_tabular_array_canonical() {
        let input =
            "items[1]:\n  - users[2]{id,name}:\n      1, Ada\n      2, Bob\n    zstatus: active";
        let value: serde_json::Value = from_str(input).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "items": [
                    {
                        "users": [
                            {"id": 1, "name": "Ada"},
                            {"id": 2, "name": "Bob"}
                        ],
                        "zstatus": "active"
                    }
                ]
            })
        );
    }

    #[rstest::rstest]
    fn test_list_item_tabular_array_requires_hyphen_header() {
        let input = "items[1]:\n  -\n    users[2]{id,name}:\n      1, Ada\n      2, Bob\n    zstatus: active";
        let err = validate_canonical(input).unwrap_err();
        assert!(err.message.contains("hyphen line"));
    }

    #[rstest::rstest]
    fn test_encode_list_item_tabular_array() {
        let value = serde_json::json!({
            "items": [
                {
                    "users": [
                        {"id": 1, "name": "Ada"},
                        {"id": 2, "name": "Bob"},
                        {"id": 3, "name": "Cora"},
                        {"id": 4, "name": "Dana"}
                    ],
                    "zstatus": "active"
                }
            ]
        });
        let toon = to_string(&value).unwrap();
        assert!(toon.contains("  - users[4]{id,name}:"));
        assert!(toon.contains("      1, Ada"));
        assert!(toon.contains("    zstatus: active"));
    }

    #[rstest::rstest]
    fn test_encode_tabular_header_unquoted_when_canonical() {
        let value = serde_json::json!({
            "rows": [
                {"id": 1},
                {"id": 2},
                {"id": 3},
                {"id": 4}
            ]
        });
        let toon = to_string(&value).unwrap();
        assert!(toon.contains("rows[4]{id}:"));
        assert!(!toon.contains("{\"id\"}"));
    }

    #[rstest::rstest]
    fn test_array_inline_encode() {
        let value = serde_json::json!({"items": [1, 2, 3]});
        let toon = to_string(&value).unwrap();
        assert_eq!(toon, "items[3]: 1, 2, 3");
    }

    #[rstest::rstest]
    fn test_tabular_not_used_for_mixed_fields() {
        let value = serde_json::json!({
            "rows": [
                {"id": 1, "name": "Ada"},
                {"id": 2, "name": "Bob", "tag": "x"}
            ]
        });
        let toon = to_string(&value).unwrap();
        assert!(!toon.contains("{id,name}"));
        let decoded: serde_json::Value = from_str(&toon).unwrap();
        assert_eq!(decoded, value);
    }

    #[rstest::rstest]
    fn test_tabular_not_used_for_non_scalar_fields() {
        let value = serde_json::json!({
            "rows": [
                {"id": 1, "tags": ["a"]},
                {"id": 2, "tags": ["b"]}
            ]
        });
        let toon = to_string(&value).unwrap();
        assert!(!toon.contains("{id,tags}"));
        let decoded: serde_json::Value = from_str(&toon).unwrap();
        assert_eq!(decoded, value);
    }

    #[rstest::rstest]
    fn test_encode_delimiter_marker() {
        let value = serde_json::json!({"items": [1, 2]});
        let profile = CanonicalProfile {
            indent_spaces: 2,
            delimiter: crate::canonical::profile::CanonicalDelimiter::Pipe,
        };
        let toon = encode_canonical(&value, profile).unwrap();
        assert_eq!(toon, "items[2|]: 1|2");
    }

    #[rstest::rstest]
    fn test_validate_rejects_unsorted_keys() {
        let err = validate_canonical("b: 1\na: 2").unwrap_err();
        assert!(err.message.contains("sorted") || err.message.contains("increasing"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_trailing_newline() {
        let err = validate_canonical("name: Ada\n").unwrap_err();
        assert!(err.message.contains("trailing newline"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_unquoted_string_needing_quotes() {
        let err = validate_canonical("name: hello,world").unwrap_err();
        assert!(err.message.contains("quoted"));
        let err = validate_canonical("bad-key: ok").unwrap_err();
        assert!(err.message.contains("key"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_unnecessary_quotes() {
        let err = validate_canonical("name: \"Ada\"").unwrap_err();
        assert!(err.message.contains("unquoted"));
        let err = validate_canonical("\"name\": Ada").unwrap_err();
        assert!(err.message.contains("quoted"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_unnecessary_tabular_header_quotes() {
        let err = validate_canonical("items[1]{\"id\"}:\n  1").unwrap_err();
        assert!(err.message.contains("quoted"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_invalid_escape() {
        let err = validate_canonical("name: \"bad\\q\"").unwrap_err();
        assert!(err.message.contains("escape"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_noncanonical_spacing() {
        let err = validate_canonical("name:Ada").unwrap_err();
        assert!(err.message.contains("space"));
        let err = validate_canonical("name:  Ada").unwrap_err();
        assert!(err.message.contains("space"));
        let err = validate_canonical("items[3]: 1,2,3").unwrap_err();
        assert!(err.message.contains("delimiter") || err.message.contains("space"));
        let err = validate_canonical("items[2]{id, name}: 1, 2").unwrap_err();
        assert!(err.message.contains("tabular"));
    }

    #[rstest::rstest]
    fn test_validate_rejects_noncanonical_numbers() {
        let cases = [
            "num: 01",
            "num: 1.0",
            "num: -0",
            "num: 1e3",
            "num: 0.0",
            "num: 10.00",
            "num: 01.2",
        ];
        for case in cases {
            let err = validate_canonical(case).unwrap_err();
            assert!(err.message.contains("number"));
        }
    }

    #[rstest::rstest]
    fn test_canonical_numbers_round_trip() {
        let value: serde_json::Value = from_str("num: 0.01").unwrap();
        assert_eq!(value, serde_json::json!({"num": 0.01}));
        let value: serde_json::Value = from_str("num: -0.1").unwrap();
        assert_eq!(value, serde_json::json!({"num": -0.1}));
    }

    #[rstest::rstest]
    fn test_parallel_large_array_round_trip() {
        let mut items = Vec::new();
        for i in 0..300 {
            items.push(serde_json::json!({
                "id": i,
                "tags": [format!("{:03}-{}", i, "x".repeat(64))]
            }));
        }
        let value = serde_json::Value::Array(items.clone());
        let toon = to_string(&value).unwrap();
        let round_trip: serde_json::Value = from_str(&toon).unwrap();
        assert_eq!(round_trip, serde_json::Value::Array(items));
    }

    #[rstest::rstest]
    fn test_parallel_large_object_key_order() {
        let mut map = serde_json::Map::new();
        for i in 0..200 {
            map.insert(
                format!("key{:03}", i),
                serde_json::Value::String("x".repeat(64)),
            );
        }
        let value = serde_json::Value::Object(map);
        let toon = to_string(&value).unwrap();
        let keys: Vec<&str> = toon
            .lines()
            .map(|line| line.split(':').next().unwrap_or(""))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }
}
