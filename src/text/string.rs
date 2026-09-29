use memchr::{memchr, memchr2, memchr3};
use smallvec::SmallVec;

pub trait ByteSink {
    fn push_byte(&mut self, byte: u8);
    fn extend_bytes(&mut self, bytes: &[u8]);
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn truncate(&mut self, len: usize);
    fn last_byte(&self) -> Option<u8>;
    fn pop_byte(&mut self) -> Option<u8>;
    fn as_slice(&self) -> &[u8];
}

impl ByteSink for Vec<u8> {
    fn push_byte(&mut self, byte: u8) {
        self.push(byte);
    }

    fn extend_bytes(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn truncate(&mut self, len: usize) {
        Vec::truncate(self, len);
    }

    fn last_byte(&self) -> Option<u8> {
        self.last().copied()
    }

    fn pop_byte(&mut self) -> Option<u8> {
        self.pop()
    }

    fn as_slice(&self) -> &[u8] {
        self.as_slice()
    }
}

macro_rules! impl_bytesink_for_smallvec {
    ($len:expr) => {
        impl ByteSink for SmallVec<[u8; $len]> {
            fn push_byte(&mut self, byte: u8) {
                self.push(byte);
            }

            fn extend_bytes(&mut self, bytes: &[u8]) {
                self.extend_from_slice(bytes);
            }

            fn len(&self) -> usize {
                SmallVec::len(self)
            }

            fn truncate(&mut self, len: usize) {
                SmallVec::truncate(self, len);
            }

            fn last_byte(&self) -> Option<u8> {
                self.last().copied()
            }

            fn pop_byte(&mut self) -> Option<u8> {
                self.pop()
            }

            fn as_slice(&self) -> &[u8] {
                self.as_slice()
            }
        }
    };
}

impl_bytesink_for_smallvec!(32);
impl_bytesink_for_smallvec!(256);

pub fn analyze_string(value: &str, delimiter: char) -> (bool, bool) {
    if value.is_empty() {
        return (true, false);
    }
    if is_literal_like(value) {
        return (true, false);
    }
    if !value.is_ascii() {
        return analyze_string_unicode(value, delimiter);
    }

    let bytes = value.as_bytes();
    let first = bytes[0];
    let mut needs_quote = false;
    let mut needs_escape = false;

    if matches!(first, b' ' | b'\t' | b'-' | b'#') || first < 0x20 {
        needs_quote = true;
    }
    if is_structural_byte(first, delimiter)
        || first == b'\\'
        || first == b'"'
        || first == delimiter as u8
    {
        needs_quote = true;
    }
    if matches!(first, b'\\' | b'"') || first < 0x20 {
        needs_escape = true;
    }
    if first == b'0' && bytes.len() > 1 && bytes[1].is_ascii_digit() {
        needs_quote = true;
    }
    if bytes.len() == 1 {
        return (needs_quote, needs_escape);
    }

    if let Some(&last) = bytes.last() {
        if matches!(last, b' ' | b'\t') {
            needs_quote = true;
        }
    }

    let tail = &bytes[1..];
    let has_escape =
        memchr3(b'\\', b'"', b'\n', tail).is_some() || memchr2(b'\r', b'\t', tail).is_some();
    if has_escape {
        needs_quote = true;
        needs_escape = true;
    }

    let has_structural = memchr(b'[', tail).is_some()
        || memchr(b']', tail).is_some()
        || memchr(b'{', tail).is_some()
        || memchr(b'}', tail).is_some()
        || memchr(b':', tail).is_some()
        || (delimiter == ',' && memchr(b',', tail).is_some())
        || (delimiter != ',' && memchr(delimiter as u8, tail).is_some());
    if has_structural {
        needs_quote = true;
    }
    if bytes.iter().any(|byte| *byte < 0x20) {
        needs_quote = true;
        needs_escape = true;
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
            0x00..=0x1f => {
                if start < idx {
                    out.push_str(&value[start..idx]);
                }
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{byte:04x}");
                start = idx + 1;
                continue;
            }
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

pub fn escape_string_into_bytes<B: ByteSink>(out: &mut B, value: &str) {
    let bytes = value.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let slice = &bytes[start..];
        let idx_a = memchr3(b'\\', b'"', b'\n', slice);
        let idx_b = memchr2(b'\r', b'\t', slice);
        let idx_control = slice.iter().position(|byte| *byte < 0x20);
        let idx = match (idx_a, idx_b, idx_control) {
            (Some(a), Some(b), Some(c)) => Some(a.min(b).min(c)),
            (Some(a), Some(b), None) => Some(a.min(b)),
            (Some(a), None, Some(c)) => Some(a.min(c)),
            (None, Some(b), Some(c)) => Some(b.min(c)),
            (Some(a), None, None) => Some(a),
            (None, Some(b), None) => Some(b),
            (None, None, Some(c)) => Some(c),
            (None, None, None) => None,
        };

        let Some(rel_idx) = idx else {
            out.extend_bytes(slice);
            break;
        };
        let idx = start + rel_idx;
        if start < idx {
            out.extend_bytes(&bytes[start..idx]);
        }
        let escaped: &[u8] = match bytes[idx] {
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            b'\t' => b"\\t",
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            byte @ 0x00..=0x1f => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let escaped = [
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ];
                out.extend_bytes(&escaped);
                start = idx + 1;
                continue;
            }
            _ => unreachable!(),
        };
        out.extend_bytes(escaped);
        start = idx + 1;
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
    if matches!(bytes[0], b'-' | b'+') {
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

    if matches!(first, ' ' | '\t' | '-' | '#' | '\u{feff}') || first < '\u{20}' {
        needs_quote = true;
    }

    if is_structural_char(first, delimiter) || first == '\\' || first == '"' || first == delimiter {
        needs_quote = true;
    }

    if matches!(first, '\\' | '"') || first < '\u{20}' {
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
            || ch < '\u{20}'
        {
            needs_quote = true;
        }
        if matches!(ch, '\\' | '"') || ch < '\u{20}' {
            needs_escape = true;
        }
        last = ch;
    }

    if matches!(last, ' ' | '\t') {
        needs_quote = true;
    }

    (needs_quote, needs_escape)
}
