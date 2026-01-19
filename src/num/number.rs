use serde_json::Number;
use smallvec::SmallVec;

use crate::text::string::ByteSink;

pub fn format_json_number(number: &Number) -> String {
    let mut out = Vec::with_capacity(32);
    append_json_number(&mut out, number);
    String::from_utf8(out).unwrap_or_else(|_| "null".to_string())
}

pub fn append_json_number(out: &mut Vec<u8>, number: &Number) {
    append_json_number_bytes(out, number);
}

pub fn append_json_number_bytes<B: ByteSink>(out: &mut B, number: &Number) {
    if let Some(value) = number.as_i64() {
        let mut buffer = itoa::Buffer::new();
        out.extend_bytes(buffer.format(value).as_bytes());
        return;
    }
    if let Some(value) = number.as_u64() {
        let mut buffer = itoa::Buffer::new();
        out.extend_bytes(buffer.format(value).as_bytes());
        return;
    }
    if let Some(value) = number.as_f64() {
        append_f64(out, value);
        return;
    }
    out.extend_bytes(b"null");
}

fn append_f64<B: ByteSink>(out: &mut B, value: f64) {
    if !value.is_finite() {
        out.extend_bytes(b"null");
        return;
    }
    if value == 0.0 {
        out.push_byte(b'0');
        return;
    }
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format(value);
    let start = out.len();
    if raw
        .as_bytes()
        .iter()
        .any(|byte| *byte == b'e' || *byte == b'E')
    {
        expand_exponent_into(out, raw);
    } else {
        out.extend_bytes(raw.as_bytes());
    }
    trim_number_bytes_in_place(out, start);
}

fn expand_exponent_into<B: ByteSink>(out: &mut B, raw: &str) {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if bytes.get(i) == Some(&b'-') {
        negative = true;
        i += 1;
    }

    let mut digits: SmallVec<[u8; 32]> = SmallVec::new();
    let mut dot_pos = None;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                digits.push(bytes[i]);
                i += 1;
            }
            b'.' => {
                dot_pos = Some(digits.len());
                i += 1;
            }
            b'e' | b'E' => {
                i += 1;
                break;
            }
            _ => {
                i += 1;
            }
        }
    }

    let mut exp_sign = 1i32;
    if i < bytes.len() {
        if bytes[i] == b'-' {
            exp_sign = -1;
            i += 1;
        } else if bytes[i] == b'+' {
            i += 1;
        }
    }

    let mut exp: i32 = 0;
    while i < bytes.len() {
        if let b'0'..=b'9' = bytes[i] {
            exp = exp
                .saturating_mul(10)
                .saturating_add((bytes[i] - b'0') as i32);
        }
        i += 1;
    }
    exp *= exp_sign;

    let dot_pos = dot_pos.unwrap_or(digits.len());
    let new_pos = dot_pos as i32 + exp;
    if negative {
        out.push_byte(b'-');
    }

    if new_pos <= 0 {
        out.push_byte(b'0');
        out.push_byte(b'.');
        for _ in 0..(-new_pos) {
            out.push_byte(b'0');
        }
        out.extend_bytes(&digits);
        return;
    }

    let new_pos = new_pos as usize;
    if new_pos >= digits.len() {
        out.extend_bytes(&digits);
        for _ in 0..(new_pos - digits.len()) {
            out.push_byte(b'0');
        }
        return;
    }

    out.extend_bytes(&digits[..new_pos]);
    out.push_byte(b'.');
    out.extend_bytes(&digits[new_pos..]);
}

fn trim_number_bytes_in_place<B: ByteSink>(out: &mut B, start: usize) {
    let mut dot = None;
    for (idx, byte) in out.as_slice()[start..].iter().enumerate() {
        if *byte == b'.' {
            dot = Some(start + idx);
            break;
        }
    }
    if let Some(dot_pos) = dot {
        let mut end = out.len();
        while end > dot_pos + 1 && out.as_slice()[end - 1] == b'0' {
            end -= 1;
        }
        if end != out.len() {
            out.truncate(end);
        }
        if out.last_byte() == Some(b'.') {
            out.pop_byte();
        }
    }

    let mut all_zero = true;
    for &byte in &out.as_slice()[start..] {
        if byte == b'-' || byte == b'.' {
            continue;
        }
        if byte != b'0' {
            all_zero = false;
            break;
        }
    }
    if all_zero {
        out.truncate(start);
        out.push_byte(b'0');
    }
}
