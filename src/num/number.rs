use serde_json::Number;

pub fn format_json_number(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        let mut buffer = itoa::Buffer::new();
        return buffer.format(value).to_string();
    }
    if let Some(value) = number.as_u64() {
        let mut buffer = itoa::Buffer::new();
        return buffer.format(value).to_string();
    }
    if let Some(value) = number.as_f64() {
        return format_f64(value);
    }
    "null".to_string()
}

fn format_f64(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format(value);
    normalize_number_str(raw)
}

fn normalize_number_str(raw: &str) -> String {
    if raw.contains('e') || raw.contains('E') {
        return expand_exponent(raw);
    }
    trim_number(raw.to_string())
}

fn expand_exponent(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if bytes.get(i) == Some(&b'-') {
        negative = true;
        i += 1;
    }

    let mut digits = String::new();
    let mut dot_pos = None;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                digits.push(bytes[i] as char);
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
    let mut out = String::new();
    if negative {
        out.push('-');
    }

    if new_pos <= 0 {
        out.push('0');
        out.push('.');
        out.extend(std::iter::repeat_n('0', (-new_pos) as usize));
        out.push_str(&digits);
        return trim_number(out);
    }

    if new_pos as usize >= digits.len() {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', new_pos as usize - digits.len()));
        return trim_number(out);
    }

    let pos = new_pos as usize;
    out.push_str(&digits[..pos]);
    out.push('.');
    out.push_str(&digits[pos..]);
    trim_number(out)
}

fn trim_number(mut value: String) -> String {
    if let Some(dot) = value.find('.') {
        let mut end = value.len();
        while end > dot + 1 && value.as_bytes()[end - 1] == b'0' {
            end -= 1;
        }
        value.truncate(end);
        if value.ends_with('.') {
            value.pop();
        }
    }
    let digits = value
        .trim_start_matches('-')
        .chars()
        .filter(|ch| *ch != '.')
        .collect::<String>();
    if digits.chars().all(|ch| ch == '0') {
        return "0".to_string();
    }
    value
}
