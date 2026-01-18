use crate::utils::{literal, text::TextBuffer};

/// Escape special characters in a string for quoted output.
///
/// # Examples
/// ```
/// use serde_toon::escape_string;
///
/// assert_eq!(escape_string("hello\nworld"), "hello\\nworld");
/// ```
pub fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());

    escape_string_into(&mut result, s);

    result
}

pub fn escape_string_into<B: TextBuffer>(out: &mut B, s: &str) {
    for ch in s.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push_char(ch),
        }
    }
}


fn is_valid_unquoted_key_internal(key: &str, allow_hyphen: bool) -> bool {
    let bytes = key.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let first = bytes[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }

    bytes[1..].iter().all(|b| {
        b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.' || (allow_hyphen && *b == b'-')
    })
}

/// Check if a key can be written without quotes (alphanumeric, underscore, dot).
///
/// # Examples
/// ```
/// use serde_toon::is_valid_unquoted_key;
///
/// assert!(is_valid_unquoted_key("user_name"));
/// assert!(!is_valid_unquoted_key("1bad"));
/// ```
pub fn is_valid_unquoted_key(key: &str) -> bool {
    is_valid_unquoted_key_internal(key, false)
}

/// Determine if a string needs quoting based on content and delimiter.
///
/// # Examples
/// ```
/// use serde_toon::needs_quoting;
///
/// assert!(needs_quoting("true", ','));
/// assert!(!needs_quoting("hello", ','));
/// ```
pub fn needs_quoting(s: &str, delimiter: char) -> bool {
    if s.is_empty() {
        return true;
    }

    if literal::is_literal_like(s) {
        return true;
    }

    let mut chars = s.chars();
    let first = match chars.next() {
        Some(ch) => ch,
        None => return true,
    };

    if first.is_whitespace() || first == '-' {
        return true;
    }

    if first == '\\'
        || first == '"'
        || first == delimiter
        || first == '\n'
        || first == '\r'
        || first == '\t'
        || literal::is_structural_char(first)
    {
        return true;
    }

    if first == '0' && chars.clone().next().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }

    let mut last = first;
    for ch in chars {
        if literal::is_structural_char(ch)
            || ch == '\\'
            || ch == '"'
            || ch == delimiter
            || ch == '\n'
            || ch == '\r'
            || ch == '\t'
        {
            return true;
        }
        last = ch;
    }

    if last.is_whitespace() {
        return true;
    }

    false
}


#[cfg(test)]
mod tests {
    use crate::Delimiter;

    use super::*;

    #[rstest::rstest]
    fn test_escape_string() {
        assert_eq!(escape_string("hello"), "hello");
        assert_eq!(escape_string("hello\nworld"), "hello\\nworld");
        assert_eq!(escape_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_string("back\\slash"), "back\\\\slash");
    }


    #[rstest::rstest]
    fn test_needs_quoting() {
        let comma = Delimiter::Comma.as_char();

        assert!(needs_quoting("", comma));

        assert!(needs_quoting("true", comma));
        assert!(needs_quoting("false", comma));
        assert!(needs_quoting("null", comma));
        assert!(needs_quoting("123", comma));

        assert!(needs_quoting("hello[world]", comma));
        assert!(needs_quoting("key:value", comma));

        assert!(needs_quoting("a,b", comma));
        assert!(!needs_quoting("a,b", Delimiter::Pipe.as_char()));

        assert!(!needs_quoting("hello world", comma));
        assert!(needs_quoting(" hello", comma));
        assert!(needs_quoting("hello ", comma));

        assert!(!needs_quoting("hello", comma));
        assert!(!needs_quoting("world", comma));
        assert!(!needs_quoting("helloworld", comma));
    }


    #[rstest::rstest]
    fn test_is_valid_unquoted_key() {
        // Valid keys (should return true)
        assert!(is_valid_unquoted_key("normal_key"));
        assert!(is_valid_unquoted_key("key123"));
        assert!(is_valid_unquoted_key("key.value"));
        assert!(is_valid_unquoted_key("_private"));
        assert!(is_valid_unquoted_key("KeyName"));
        assert!(is_valid_unquoted_key("key_name"));
        assert!(is_valid_unquoted_key("key.name.sub"));
        assert!(is_valid_unquoted_key("a"));
        assert!(is_valid_unquoted_key("_"));
        assert!(is_valid_unquoted_key("key_123.value"));

        assert!(!is_valid_unquoted_key(""));
        assert!(!is_valid_unquoted_key("123"));
        assert!(!is_valid_unquoted_key("key:value"));
        assert!(!is_valid_unquoted_key("key-value"));
        assert!(!is_valid_unquoted_key("key value"));
        assert!(!is_valid_unquoted_key(".key"));
        assert!(is_valid_unquoted_key("key.value.sub."));
        assert!(is_valid_unquoted_key("key."));
        assert!(!is_valid_unquoted_key("key[value]"));
        assert!(!is_valid_unquoted_key("key{value}"));
    }
}
