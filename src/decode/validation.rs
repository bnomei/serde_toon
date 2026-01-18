use crate::types::{ToonError, ToonResult};
use std::collections::HashSet;

pub fn validate_array_length(expected: usize, actual: usize) -> ToonResult<()> {
    if expected != actual {
        return Err(ToonError::length_mismatch(expected, actual));
    }
    Ok(())
}

pub fn validate_array_length_non_negative(length: i64) -> ToonResult<()> {
    if length < 0 {
        return Err(ToonError::InvalidInput(
            "Array length must be non-negative".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_field_list(fields: &[String]) -> ToonResult<()> {
    if fields.is_empty() {
        return Err(ToonError::InvalidInput(
            "Field list cannot be empty for tabular arrays".to_string(),
        ));
    }

    let mut seen = HashSet::with_capacity(fields.len());
    for field in fields {
        if field.is_empty() {
            return Err(ToonError::InvalidInput(
                "Field name cannot be empty".to_string(),
            ));
        }
        if !seen.insert(field.as_str()) {
            return Err(ToonError::InvalidInput(format!(
                "Duplicate field name: '{field}'"
            )));
        }
    }

    Ok(())
}

pub fn validate_delimiter_consistency(
    detected: Option<crate::types::Delimiter>,
    expected: Option<crate::types::Delimiter>,
) -> ToonResult<()> {
    if let (Some(detected), Some(expected)) = (detected, expected) {
        if detected != expected {
            return Err(ToonError::InvalidDelimiter(format!(
                "Detected delimiter {detected} but expected {expected}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_validate_array_length() {
        assert!(validate_array_length(5, 3).is_err());
        assert!(validate_array_length(3, 5).is_err());
        assert!(validate_array_length(5, 5).is_ok());
    }

    #[rstest::rstest]
    fn test_validate_array_length_non_negative() {
        assert!(validate_array_length_non_negative(0).is_ok());
        assert!(validate_array_length_non_negative(5).is_ok());
        assert!(validate_array_length_non_negative(-1).is_err());
    }

    #[rstest::rstest]
    fn test_validate_field_list() {
        assert!(validate_field_list(&["id".to_string(), "name".to_string()]).is_ok());
        assert!(validate_field_list(&["field1".to_string()]).is_ok());

        assert!(validate_field_list(&[]).is_err());

        assert!(
            validate_field_list(&["id".to_string(), "name".to_string(), "id".to_string()]).is_err()
        );

        assert!(
            validate_field_list(&["id".to_string(), "".to_string(), "name".to_string()]).is_err()
        );
    }

    #[rstest::rstest]
    fn test_validate_delimiter_consistency() {
        use crate::types::Delimiter;

        assert!(
            validate_delimiter_consistency(Some(Delimiter::Comma), Some(Delimiter::Comma)).is_ok()
        );

        assert!(
            validate_delimiter_consistency(Some(Delimiter::Comma), Some(Delimiter::Pipe)).is_err()
        );

        assert!(validate_delimiter_consistency(None, Some(Delimiter::Comma)).is_ok());
        assert!(validate_delimiter_consistency(Some(Delimiter::Comma), None).is_ok());
        assert!(validate_delimiter_consistency(None, None).is_ok());
    }
}
