//! Canonical-only decode entry point.

use super::{
    parse::{parse, parse_value_view, parse_view, ParseError},
    profile::CanonicalProfile,
    scan::{preflight_scan, ScanError, ScanResult},
};

#[derive(Debug)]
pub struct CanonicalViolation {
    pub message: String,
}

pub fn decode_canonical(_input: &str) -> Result<ScanResult, CanonicalViolation> {
    let profile = CanonicalProfile::default();
    preflight_scan(_input, profile).map_err(CanonicalViolation::from_scan)
}

pub fn validate_canonical(input: &str) -> Result<(), CanonicalViolation> {
    let profile = CanonicalProfile::default();
    let scan = preflight_scan(input, profile).map_err(CanonicalViolation::from_scan)?;
    parse_view(input, &scan)
        .map(|_| ())
        .map_err(CanonicalViolation::from_parse)
}

pub fn validate_passthrough<'a>(input: &'a str) -> Result<&'a str, CanonicalViolation> {
    validate_canonical(input)?;
    Ok(input)
}

pub fn decode_and_parse(_input: &str) -> Result<super::arena::Arena, CanonicalViolation> {
    let scan = decode_canonical(_input)?;
    Ok(parse(&scan))
}

pub fn decode_and_parse_view<'a>(
    input: &'a str,
) -> Result<super::arena::ArenaView<'a>, CanonicalViolation> {
    let scan = decode_canonical(input)?;
    parse_view(input, &scan).map_err(CanonicalViolation::from_parse)
}

pub fn decode_to_value(input: &str) -> Result<serde_json::Value, CanonicalViolation> {
    let scan = decode_canonical(input)?;
    parse_value_view(input, &scan).map_err(CanonicalViolation::from_parse)
}

impl CanonicalViolation {
    fn from_scan(err: ScanError) -> Self {
        Self {
            message: format!("line {} col {}: {}", err.line, err.column, err.message),
        }
    }

    fn from_parse(err: ParseError) -> Self {
        Self {
            message: format!("line {} col {}: {}", err.line, err.column, err.message),
        }
    }
}
