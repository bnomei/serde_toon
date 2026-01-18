pub(crate) mod literal;
pub(crate) mod number;
pub(crate) mod string;
pub(crate) mod text;
pub(crate) mod validation;

use indexmap::IndexMap;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::types::{JsonValue as Value, Number};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotingContext {
    ObjectValue,
    ArrayValue,
}

#[cfg(feature = "parallel")]
const PARALLEL_THRESHOLD: usize = 256;

#[allow(dead_code)]
pub fn normalize(value: Value) -> Value {
    match value {
        Value::Number(n) => {
            // Handle NegInt(0) case - convert to PosInt(0)
            if let Number::NegInt(0) = n {
                Value::Number(Number::from(0u64))
            } else if let Some(f) = n.as_f64() {
                if f.is_nan() || f.is_infinite() {
                    Value::Null
                } else if f == 0.0 && f.is_sign_negative() {
                    Value::Number(Number::from(0u64))
                } else {
                    Value::Number(n)
                }
            } else {
                Value::Number(n)
            }
        }
        Value::Object(obj) => {
            #[cfg(feature = "parallel")]
            {
                if obj.len() >= PARALLEL_THRESHOLD {
                    let entries: Vec<(String, Value)> = obj.into_iter().collect();
                    let normalized_entries: Vec<(String, Value)> = entries
                        .into_par_iter()
                        .map(|(k, v)| (k, normalize(v)))
                        .collect();
                    return Value::Object(IndexMap::from_iter(normalized_entries));
                }
            }

            let normalized: IndexMap<String, Value> =
                obj.into_iter().map(|(k, v)| (k, normalize(v))).collect();
            Value::Object(normalized)
        }
        Value::Array(arr) => {
            #[cfg(feature = "parallel")]
            {
                if arr.len() >= PARALLEL_THRESHOLD {
                    let normalized: Vec<Value> = arr.into_par_iter().map(normalize).collect();
                    return Value::Array(normalized);
                }
            }

            let normalized: Vec<Value> = arr.into_iter().map(normalize).collect();
            Value::Array(normalized)
        }
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use core::f64;

    use serde_json::json;

    use super::*;

    #[rstest::rstest]
    fn test_normalize_nan() {
        let value = Value::from(json!(f64::NAN));
        let normalized = normalize(value);
        assert_eq!(normalized, Value::from(json!(null)));
    }

    #[rstest::rstest]
    fn test_normalize_infinity() {
        let value = Value::from(json!(f64::INFINITY));
        let normalized = normalize(value);
        assert_eq!(normalized, Value::from(json!(null)));

        let value = Value::from(json!(f64::NEG_INFINITY));
        let normalized = normalize(value);
        assert_eq!(normalized, Value::from(json!(null)));
    }

    #[rstest::rstest]
    fn test_normalize_negative_zero() {
        let value = Value::from(json!(-0.0));
        let normalized = normalize(value);
        assert_eq!(normalized, Value::from(json!(0)));
    }

    #[rstest::rstest]
    fn test_normalize_nested() {
        let value = Value::from(json!({
            "a": f64::NAN,
            "b": {
                "c": f64::INFINITY
            },
            "d": [1, f64::NAN, 3]
        }));

        let normalized = normalize(value);
        assert_eq!(
            normalized,
            Value::from(json!({
                "a": null,
                "b": {
                    "c": null
                },
                "d": [1, null, 3]
            }))
        );
    }

    #[rstest::rstest]
    fn test_normalize_normal_values() {
        let value = Value::from(json!({
            "name": "Alice",
            "age": 30,
            "score": f64::consts::PI
        }));

        let normalized = normalize(value.clone());
        assert_eq!(normalized, value);
    }
}
