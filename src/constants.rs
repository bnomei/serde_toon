pub const KEYWORDS: &[&str] = &["null", "true", "false"];

pub const DEFAULT_INDENT: usize = 2;

pub const MAX_DEPTH: usize = 256;

pub(crate) const QUOTED_KEY_MARKER: char = '\x00';

#[inline]
pub fn is_structural_char(ch: char) -> bool {
    matches!(ch, '[' | ']' | '{' | '}' | ':' | '-')
}

#[inline]
pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::{decode, encode, DecodeOptions, EncodeOptions};

    #[rstest::rstest]
    fn test_is_structural_char() {
        assert!(is_structural_char('['));
        assert!(is_structural_char(']'));
        assert!(is_structural_char('{'));
        assert!(is_structural_char('}'));
        assert!(is_structural_char(':'));
        assert!(is_structural_char('-'));
        assert!(!is_structural_char('a'));
        assert!(!is_structural_char(','));
    }

    #[rstest::rstest]
    fn test_is_keyword() {
        assert!(is_keyword("null"));
        assert!(is_keyword("true"));
        assert!(is_keyword("false"));
        assert!(!is_keyword("hello"));
        assert!(!is_keyword("TRUE"));
    }

    #[rstest::rstest]
    fn test_max_depth_boundary() {
        let mut nested = json!(null);
        for _ in 0..=MAX_DEPTH {
            nested = json!({"a": nested});
        }

        let encoded = encode(&nested, &EncodeOptions::default());
        assert!(encoded.is_ok());

        let too_deep = json!({"a": nested});
        let result = encode(&too_deep, &EncodeOptions::default());
        assert!(result.is_err());
    }

    #[rstest::rstest]
    fn test_large_array() {
        let data: Vec<u32> = (0..10_000).collect();
        let large = json!(data);
        let encoded = encode(&large, &EncodeOptions::default()).unwrap();
        let decoded: serde_json::Value = decode(&encoded, &DecodeOptions::default()).unwrap();
        assert_eq!(large, decoded);
    }

    #[rstest::rstest]
    fn test_very_long_string() {
        let long_string = "x".repeat(100_000);
        let value = json!({"data": long_string});
        let encoded = encode(&value, &EncodeOptions::default()).unwrap();
        let decoded: serde_json::Value = decode(&encoded, &DecodeOptions::default()).unwrap();
        assert_eq!(value, decoded);
    }

    #[rstest::rstest]
    fn test_empty_structures() {
        let empty_obj = json!({});
        let empty_arr = json!([]);

        let encoded_obj = encode(&empty_obj, &EncodeOptions::default()).unwrap();
        let encoded_arr = encode(&empty_arr, &EncodeOptions::default()).unwrap();

        let decoded_obj: serde_json::Value =
            decode(&encoded_obj, &DecodeOptions::default()).unwrap();
        let decoded_arr: serde_json::Value =
            decode(&encoded_arr, &DecodeOptions::default()).unwrap();

        assert_eq!(empty_obj, decoded_obj);
        assert_eq!(empty_arr, decoded_arr);
    }
}
