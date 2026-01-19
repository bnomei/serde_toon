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

pub fn escape_string_into(out: &mut String, value: &str) {
    let bytes = value.as_bytes();
    let mut start = 0;
    for (idx, byte) in bytes.iter().enumerate() {
        let escaped = match byte {
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            b'"' => "\\\"",
            b'\\' => "\\\\",
            _ => continue,
        };
        if start < idx {
            out.push_str(&value[start..idx]);
        }
        out.push_str(escaped);
        start = idx + 1;
    }
    if start < value.len() {
        out.push_str(&value[start..]);
    }
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

pub fn is_identifier_segment(key: &str) -> bool {
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
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn is_literal_like(value: &str) -> bool {
    is_keyword(value) || is_numeric_like(value)
}

fn is_keyword(value: &str) -> bool {
    matches!(value, "true" | "false" | "null")
}

fn is_numeric_like(value: &str) -> bool {
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

    if first == '0' && chars.clone().next().is_some_and(|ch| ch.is_ascii_digit()) {
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
