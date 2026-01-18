//! Canonical validators for sorted keys, quoting, numbers, and whitespace.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationKind {
    SortedKeys,
    CanonicalNumbers,
    CanonicalWhitespace,
    CanonicalQuoting,
}

pub fn is_canonical_unquoted_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
}

pub fn is_canonical_unquoted_string(value: &str, delimiter: char) -> bool {
    !needs_quoting(value, delimiter)
}

pub fn needs_quoting(value: &str, delimiter: char) -> bool {
    analyze_string(value, delimiter).0
}

pub fn analyze_string(value: &str, delimiter: char) -> (bool, bool) {
    if value.is_empty() {
        return (true, false);
    }
    if is_literal_like(value) {
        return (true, false);
    }

    let bytes = value.as_bytes();
    let first = bytes[0];
    if first & 0x80 != 0 {
        return analyze_string_unicode(value, delimiter);
    }

    let mut needs_quote = false;
    let mut needs_escape = false;

    if first.is_ascii_whitespace() || first == b'-' {
        needs_quote = true;
    }
    if is_structural_byte(first, delimiter)
        || first == b'\\'
        || first == b'"'
        || first == delimiter as u8
    {
        needs_quote = true;
    }
    if matches!(first, b'\\' | b'"' | b'\n' | b'\r' | b'\t') {
        needs_escape = true;
    }

    if first == b'0' && bytes.len() > 1 && bytes[1].is_ascii_digit() {
        needs_quote = true;
    }

    let mut last = first;
    for &byte in &bytes[1..] {
        if byte & 0x80 != 0 {
            return analyze_string_unicode(value, delimiter);
        }
        if is_structural_byte(byte, delimiter)
            || byte == b'\\'
            || byte == b'"'
            || byte == delimiter as u8
            || byte == b'\n'
            || byte == b'\r'
            || byte == b'\t'
        {
            needs_quote = true;
        }
        if matches!(byte, b'\\' | b'"' | b'\n' | b'\r' | b'\t') {
            needs_escape = true;
        }
        last = byte;
    }

    if last.is_ascii_whitespace() {
        needs_quote = true;
    }

    (needs_quote, needs_escape)
}

pub fn is_canonical_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let mut i = 0;
    let negative = bytes[0] == b'-';
    if negative {
        i = 1;
        if i >= bytes.len() {
            return false;
        }
    }

    let mut dot = None;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {}
            b'.' => {
                if dot.is_some() {
                    return false;
                }
                dot = Some(i);
            }
            b'e' | b'E' | b'+' => return false,
            _ => return false,
        }
        i += 1;
    }

    let int_start = if negative { 1 } else { 0 };
    let int_end = dot.unwrap_or(bytes.len());
    if int_start >= int_end {
        return false;
    }

    if int_end - int_start > 1 && bytes[int_start] == b'0' {
        return false;
    }

    if dot.is_none() {
        if negative && bytes[int_start] == b'0' && int_end - int_start == 1 {
            return false;
        }
        return true;
    }

    let frac_start = dot.unwrap() + 1;
    if frac_start >= bytes.len() {
        return false;
    }
    if bytes[bytes.len() - 1] == b'0' {
        return false;
    }
    true
}

fn is_literal_like(value: &str) -> bool {
    is_keyword(value) || is_numeric_like(value)
}

fn is_keyword(value: &str) -> bool {
    matches!(value, "true" | "false" | "null")
}

pub(crate) fn is_numeric_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    if bytes[0] == b'-' {
        i = 1;
    }
    if i >= bytes.len() {
        return false;
    }
    let first = bytes[i];
    if !first.is_ascii_digit() {
        return false;
    }
    bytes[i..].iter().all(|b| {
        b.is_ascii_digit() || *b == b'.' || *b == b'e' || *b == b'E' || *b == b'+' || *b == b'-'
    })
}

fn is_structural_char(ch: char, delimiter: char) -> bool {
    matches!(ch, '[' | ']' | '{' | '}' | ':') || (delimiter == ',' && ch == ',')
}

fn is_structural_byte(byte: u8, delimiter: char) -> bool {
    matches!(byte, b'[' | b']' | b'{' | b'}' | b':') || (delimiter == ',' && byte == b',')
}

fn analyze_string_unicode(value: &str, delimiter: char) -> (bool, bool) {
    let mut chars = value.chars();
    let first = match chars.next() {
        Some(ch) => ch,
        None => return (true, false),
    };

    let mut needs_quote = false;
    let mut needs_escape = false;

    if first.is_whitespace() || first == '-' {
        needs_quote = true;
    }

    if is_structural_char(first, delimiter) || first == '\\' || first == '"' || first == delimiter {
        needs_quote = true;
    }

    if matches!(first, '\\' | '"' | '\n' | '\r' | '\t') {
        needs_escape = true;
    }

    if first == '0' && chars.clone().next().is_some_and(|c| c.is_ascii_digit()) {
        needs_quote = true;
    }

    let mut last = first;
    for ch in chars {
        if is_structural_char(ch, delimiter)
            || ch == '\\'
            || ch == '"'
            || ch == delimiter
            || ch == '\n'
            || ch == '\r'
            || ch == '\t'
        {
            needs_quote = true;
        }
        if matches!(ch, '\\' | '"' | '\n' | '\r' | '\t') {
            needs_escape = true;
        }
        last = ch;
    }

    if last.is_whitespace() {
        needs_quote = true;
    }

    (needs_quote, needs_escape)
}
