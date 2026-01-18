use itoa::Buffer as ItoaBuffer;
use ryu::Buffer as RyuBuffer;

use crate::types::Number;
use crate::utils::text::TextBuffer;

pub(crate) fn write_canonical_number_into<B: TextBuffer>(n: &Number, out: &mut B) {
    match n {
        Number::PosInt(u) => write_u64(out, *u),
        Number::NegInt(i) => write_i64(out, *i),
        Number::Float(f) => write_f64_canonical_into(*f, out),
    }
}

fn write_u64<B: TextBuffer>(out: &mut B, value: u64) {
    let mut buf = ItoaBuffer::new();
    out.push_str(buf.format(value));
}

fn write_i64<B: TextBuffer>(out: &mut B, value: i64) {
    let mut buf = ItoaBuffer::new();
    out.push_str(buf.format(value));
}

fn write_f64_canonical_into<B: TextBuffer>(f: f64, out: &mut B) {
    // Normalize integer-valued floats to integers
    if f.is_finite() && f.fract() == 0.0 && f.abs() <= i64::MAX as f64 {
        write_i64(out, f as i64);
        return;
    }

    if !f.is_finite() {
        out.push_char('0');
        return;
    }

    let mut buf = RyuBuffer::new();
    let formatted = buf.format(f);

    // Handle cases where Rust would use exponential notation
    if formatted.contains('e') || formatted.contains('E') {
        write_without_exponent(f, out);
    } else {
        push_trimmed_decimal(formatted, out);
    }
}

fn write_without_exponent<B: TextBuffer>(f: f64, out: &mut B) {
    if !f.is_finite() {
        out.push_char('0');
        return;
    }

    if f.abs() >= 1.0 {
        let abs_f = f.abs();
        let int_part = abs_f.trunc();
        let frac_part = abs_f.fract();

        if frac_part == 0.0 {
            if abs_f <= i64::MAX as f64 {
                if f < 0.0 {
                    out.push_char('-');
                }
                write_i64(out, int_part as i64);
            } else {
                let result = format!("{f:.0}");
                push_trimmed_decimal(&result, out);
            }
        } else {
            // High precision to avoid exponent, then trim trailing zeros
            let result = format!("{f:.17}");
            push_trimmed_decimal(&result, out);
        }
    } else if f == 0.0 {
        out.push_char('0');
    } else {
        // Small numbers: use high precision to avoid exponent
        let result = format!("{f:.17}");
        push_trimmed_decimal(&result, out);
    }
}

#[cfg(test)]
fn remove_trailing_zeros(s: &str) -> String {
    if let Some((int_part, frac_part)) = s.split_once('.') {
        let trimmed = frac_part.trim_end_matches('0');
        if trimmed.is_empty() {
            int_part.to_string()
        } else {
            let mut out = String::with_capacity(int_part.len() + 1 + trimmed.len());
            out.push_str(int_part);
            out.push('.');
            out.push_str(trimmed);
            out
        }
    } else {
        // No decimal point, return as-is
        s.to_string()
    }
}

fn push_trimmed_decimal<B: TextBuffer>(s: &str, out: &mut B) {
    if let Some((int_part, frac_part)) = s.split_once('.') {
        let trimmed = frac_part.trim_end_matches('0');
        if trimmed.is_empty() {
            out.push_str(int_part);
        } else {
            out.push_str(int_part);
            out.push_char('.');
            out.push_str(trimmed);
        }
    } else {
        out.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use std::f64;

    use serde_json::json;

    use super::*;

    fn format_number(n: &Number) -> String {
        let mut out = String::new();
        write_canonical_number_into(n, &mut out);
        out
    }

    #[rstest::rstest]
    fn test_format_canonical_integers() {
        let n = Number::from(42i64);
        assert_eq!(format_number(&n), "42");

        let n = Number::from(-123i64);
        assert_eq!(format_number(&n), "-123");

        let n = Number::from(0i64);
        assert_eq!(format_number(&n), "0");
    }

    #[rstest::rstest]
    fn test_format_canonical_floats() {
        // Integer-valued floats
        let n = Number::from(1.0);
        assert_eq!(format_number(&n), "1");

        let n = Number::from(42.0);
        assert_eq!(format_number(&n), "42");

        // Non-integer floats
        let n = Number::from(1.5);
        assert_eq!(format_number(&n), "1.5");

        let n = Number::from(f64::consts::PI);
        let result = format_number(&n);
        assert!(result.starts_with("3.141592653589793"));
        assert!(!result.contains('e'));
        assert!(!result.contains('E'));
    }

    #[rstest::rstest]
    fn test_remove_trailing_zeros() {
        assert_eq!(remove_trailing_zeros("1.5000"), "1.5");
        assert_eq!(remove_trailing_zeros("1.0"), "1");
        assert_eq!(remove_trailing_zeros("1.500"), "1.5");
        assert_eq!(remove_trailing_zeros("42"), "42");
        assert_eq!(remove_trailing_zeros("0.0"), "0");
        assert_eq!(remove_trailing_zeros("1.23"), "1.23");
    }

    #[rstest::rstest]
    fn test_large_numbers_no_exponent() {
        // 1e6 should become 1000000
        let n = Number::from(1_000_000.0);
        let result = format_number(&n);
        assert_eq!(result, "1000000");
        assert!(!result.contains('e'));

        // 1e9
        let n = Number::from(1_000_000_000.0);
        let result = format_number(&n);
        assert_eq!(result, "1000000000");
        assert!(!result.contains('e'));
    }

    #[rstest::rstest]
    fn test_small_numbers_no_exponent() {
        // 1e-6 should become 0.000001
        let n = Number::from(0.000001);
        let result = format_number(&n);
        assert!(result.starts_with("0.000001"));
        assert!(!result.contains('e'));
        assert!(!result.contains('E'));

        // 1e-3
        let n = Number::from(0.001);
        let result = format_number(&n);
        assert_eq!(result, "0.001");
    }

    #[rstest::rstest]
    fn test_consistency_with_json() {
        let n = Number::from(1.234);
        let mut out = String::new();
        write_canonical_number_into(&n, &mut out);
        let json_value = json!(1.234);
        assert_eq!(out, json_value.to_string());
    }
}
