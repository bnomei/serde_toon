use std::{
    fmt,
    ops::{Index, IndexMut},
};

use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Number {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

impl Number {
    pub fn from_f64(f: f64) -> Option<Self> {
        if f.is_finite() {
            Some(Number::Float(f))
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn is_i64(&self) -> bool {
        match self {
            Number::NegInt(_) => true,
            Number::PosInt(u) => *u <= i64::MAX as u64,
            Number::Float(f) => {
                let i = *f as i64;
                i as f64 == *f && i != i64::MAX
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_u64(&self) -> bool {
        match self {
            Number::PosInt(_) => true,
            Number::NegInt(_) => false,
            Number::Float(f) => {
                let u = *f as u64;
                u as f64 == *f
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_f64(&self) -> bool {
        matches!(self, Number::Float(_))
    }

    #[allow(dead_code)]
    pub fn is_integer(&self) -> bool {
        match self {
            Number::PosInt(_) | Number::NegInt(_) => true,
            Number::Float(f) => f.fract() == 0.0,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Number::PosInt(u) => {
                if *u <= i64::MAX as u64 {
                    Some(*u as i64)
                } else {
                    None
                }
            }
            Number::NegInt(i) => Some(*i),
            Number::Float(f) => {
                let i = *f as i64;
                if i as f64 == *f {
                    Some(i)
                } else {
                    None
                }
            }
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Number::PosInt(u) => Some(*u),
            Number::NegInt(_) => None,
            Number::Float(f) => {
                if *f >= 0.0 {
                    let u = *f as u64;
                    if u as f64 == *f {
                        Some(u)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Number::PosInt(u) => Some(*u as f64),
            Number::NegInt(i) => Some(*i as f64),
            Number::Float(f) => Some(*f),
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s_json_num = match self {
            Number::PosInt(u) => serde_json::Number::from(*u),
            Number::NegInt(i) => serde_json::Number::from(*i),
            Number::Float(fl) => {
                serde_json::Number::from_f64(*fl).unwrap_or_else(|| serde_json::Number::from(0))
            }
        };
        write!(f, "{s_json_num}")
    }
}

impl From<i8> for Number {
    fn from(n: i8) -> Self {
        Number::NegInt(n as i64)
    }
}

impl From<i16> for Number {
    fn from(n: i16) -> Self {
        Number::NegInt(n as i64)
    }
}

impl From<i32> for Number {
    fn from(n: i32) -> Self {
        Number::NegInt(n as i64)
    }
}

impl From<i64> for Number {
    fn from(n: i64) -> Self {
        if n >= 0 {
            Number::PosInt(n as u64)
        } else {
            Number::NegInt(n)
        }
    }
}

impl From<isize> for Number {
    fn from(n: isize) -> Self {
        Number::from(n as i64)
    }
}

impl From<u8> for Number {
    fn from(n: u8) -> Self {
        Number::PosInt(n as u64)
    }
}

impl From<u16> for Number {
    fn from(n: u16) -> Self {
        Number::PosInt(n as u64)
    }
}

impl From<u32> for Number {
    fn from(n: u32) -> Self {
        Number::PosInt(n as u64)
    }
}

impl From<u64> for Number {
    fn from(n: u64) -> Self {
        Number::PosInt(n)
    }
}

impl From<usize> for Number {
    fn from(n: usize) -> Self {
        Number::PosInt(n as u64)
    }
}

impl From<f32> for Number {
    fn from(n: f32) -> Self {
        Number::Float(n as f64)
    }
}

impl From<f64> for Number {
    fn from(n: f64) -> Self {
        Number::Float(n)
    }
}

pub(crate) type Object = IndexMap<String, JsonValue>;

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum JsonValue {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<JsonValue>),
    Object(Object),
}

impl JsonValue {
    #[allow(dead_code)]
    pub const fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    #[allow(dead_code)]
    pub const fn is_bool(&self) -> bool {
        matches!(self, JsonValue::Bool(_))
    }

    #[allow(dead_code)]
    pub const fn is_number(&self) -> bool {
        matches!(self, JsonValue::Number(_))
    }

    #[allow(dead_code)]
    pub const fn is_string(&self) -> bool {
        matches!(self, JsonValue::String(_))
    }

    #[allow(dead_code)]
    pub const fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }

    #[allow(dead_code)]
    pub const fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    #[allow(dead_code)]
    pub fn is_i64(&self) -> bool {
        match self {
            JsonValue::Number(n) => n.is_i64(),
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_u64(&self) -> bool {
        match self {
            JsonValue::Number(n) => n.is_u64(),
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_f64(&self) -> bool {
        match self {
            JsonValue::Number(n) => n.is_f64(),
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Number(n) => n.as_i64(),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) => n.as_u64(),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            JsonValue::Object(obj) => Some(obj),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            JsonValue::Object(obj) => Some(obj),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(obj) => obj.get(key),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn get_index(&self, index: usize) -> Option<&JsonValue> {
        match self {
            JsonValue::Array(arr) => arr.get(index),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn take(&mut self) -> JsonValue {
        std::mem::replace(self, JsonValue::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "array",
            JsonValue::Object(_) => "object",
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{b}"),
            JsonValue::Number(n) => write!(f, "{n}"),
            JsonValue::String(s) => write!(f, "\"{s}\""),
            JsonValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            JsonValue::Object(obj) => {
                write!(f, "{{")?;
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{k}\": {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Index<usize> for JsonValue {
    type Output = JsonValue;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            JsonValue::Array(arr) => arr.get(index).unwrap_or_else(|| {
                panic!(
                    "index {index} out of bounds for array of length {}",
                    arr.len()
                )
            }),
            _ => panic!(
                "cannot index into non-array value of type {}",
                self.type_name()
            ),
        }
    }
}

impl IndexMut<usize> for JsonValue {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self {
            JsonValue::Array(arr) => {
                let len = arr.len();
                arr.get_mut(index).unwrap_or_else(|| {
                    panic!("index {index} out of bounds for array of length {len}")
                })
            }
            _ => panic!(
                "cannot index into non-array value of type {}",
                self.type_name()
            ),
        }
    }
}

impl Index<&str> for JsonValue {
    type Output = JsonValue;

    fn index(&self, key: &str) -> &Self::Output {
        match self {
            JsonValue::Object(obj) => obj.get(key).unwrap_or_else(|| {
                panic!("key '{key}' not found in object with {} entries", obj.len())
            }),
            _ => panic!(
                "cannot index into non-object value of type {}",
                self.type_name()
            ),
        }
    }
}

impl IndexMut<&str> for JsonValue {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        match self {
            JsonValue::Object(obj) => {
                let len = obj.len();
                obj.get_mut(key)
                    .unwrap_or_else(|| panic!("key '{key}' not found in object with {len} entries"))
            }
            _ => panic!(
                "cannot index into non-object value of type {}",
                self.type_name()
            ),
        }
    }
}

impl Index<String> for JsonValue {
    type Output = JsonValue;

    fn index(&self, key: String) -> &Self::Output {
        self.index(key.as_str())
    }
}

impl IndexMut<String> for JsonValue {
    fn index_mut(&mut self, key: String) -> &mut Self::Output {
        self.index_mut(key.as_str())
    }
}

impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsonValue::Null,
            serde_json::Value::Bool(b) => JsonValue::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    JsonValue::Number(Number::from(i))
                } else if let Some(u) = n.as_u64() {
                    JsonValue::Number(Number::from(u))
                } else if let Some(f) = n.as_f64() {
                    JsonValue::Number(Number::from(f))
                } else {
                    JsonValue::Null
                }
            }
            serde_json::Value::String(s) => JsonValue::String(s),
            serde_json::Value::Array(arr) => {
                JsonValue::Array(arr.into_iter().map(JsonValue::from).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut new_obj = Object::new();
                for (k, v) in obj {
                    new_obj.insert(k, JsonValue::from(v));
                }
                JsonValue::Object(new_obj)
            }
        }
    }
}

impl From<&serde_json::Value> for JsonValue {
    fn from(value: &serde_json::Value) -> Self {
        value.clone().into()
    }
}

impl From<JsonValue> for serde_json::Value {
    fn from(value: JsonValue) -> Self {
        match value {
            JsonValue::Null => serde_json::Value::Null,
            JsonValue::Bool(b) => serde_json::Value::Bool(b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    serde_json::Value::Number(i.into())
                } else if let Some(u) = n.as_u64() {
                    serde_json::Value::Number(u.into())
                } else if let Some(f) = n.as_f64() {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            JsonValue::String(s) => serde_json::Value::String(s),
            JsonValue::Array(arr) => {
                serde_json::Value::Array(arr.into_iter().map(Into::into).collect())
            }
            JsonValue::Object(obj) => {
                let mut new_obj = serde_json::Map::new();
                for (k, v) in obj {
                    new_obj.insert(k, v.into());
                }
                serde_json::Value::Object(new_obj)
            }
        }
    }
}

impl From<&JsonValue> for serde_json::Value {
    fn from(value: &JsonValue) -> Self {
        value.clone().into()
    }
}

#[cfg(test)]
trait IntoJsonValue {
    fn into_json_value(self) -> JsonValue;
}

#[cfg(test)]
impl IntoJsonValue for &JsonValue {
    fn into_json_value(self) -> JsonValue {
        self.clone()
    }
}

#[cfg(test)]
impl IntoJsonValue for JsonValue {
    fn into_json_value(self) -> JsonValue {
        self
    }
}

#[cfg(test)]
impl IntoJsonValue for &serde_json::Value {
    fn into_json_value(self) -> JsonValue {
        self.into()
    }
}

#[cfg(test)]
impl IntoJsonValue for serde_json::Value {
    fn into_json_value(self) -> JsonValue {
        (&self).into()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use indexmap::IndexMap;
    use serde_json::json;

    use super::{IntoJsonValue, JsonValue, Number};

    #[rstest::rstest]
    fn test_number_from_f64_rejects_non_finite() {
        assert!(Number::from_f64(f64::NAN).is_none());
        assert!(Number::from_f64(f64::INFINITY).is_none());
        assert!(Number::from_f64(f64::NEG_INFINITY).is_none());
        assert!(Number::from_f64(1.5).is_some());
    }

    #[rstest::rstest]
    fn test_number_integer_checks() {
        let float_int = Number::Float(42.0);
        assert!(float_int.is_i64());
        assert!(float_int.is_u64());

        let float_frac = Number::Float(42.5);
        assert!(!float_frac.is_i64());
        assert!(!float_frac.is_u64());

        let float_max = Number::Float(i64::MAX as f64);
        assert!(!float_max.is_i64());
        assert_eq!(float_max.as_i64(), Some(i64::MAX));

        let float_neg = Number::Float(-1.0);
        assert!(!float_neg.is_u64());
    }

    #[rstest::rstest]
    fn test_number_as_conversions() {
        let too_large = Number::PosInt(i64::MAX as u64 + 1);
        assert_eq!(too_large.as_i64(), None);

        let neg = Number::NegInt(-5);
        assert_eq!(neg.as_u64(), None);

        let float_exact = Number::Float(7.0);
        assert_eq!(float_exact.as_i64(), Some(7));
        assert_eq!(float_exact.as_u64(), Some(7));

        let float_frac = Number::Float(7.25);
        assert_eq!(float_frac.as_i64(), None);
        assert_eq!(float_frac.as_u64(), None);

        let float_nan = Number::Float(f64::NAN);
        assert!(!float_nan.is_integer());
    }

    #[rstest::rstest]
    fn test_number_display_nan() {
        let value = Number::from(f64::NAN);
        assert_eq!(format!("{value}"), "0");
    }

    #[rstest::rstest]
    fn test_json_value_accessors_and_take() {
        let mut obj = IndexMap::new();
        obj.insert("a".to_string(), JsonValue::Number(Number::from(1)));

        let mut value = JsonValue::Object(obj);
        assert!(value.is_object());
        assert_eq!(value.type_name(), "object");
        assert_eq!(value.get("a").and_then(JsonValue::as_i64), Some(1));

        value
            .as_object_mut()
            .unwrap()
            .insert("b".to_string(), JsonValue::String("hi".to_string()));
        assert_eq!(value.get("b").and_then(JsonValue::as_str), Some("hi"));

        let mut arr = JsonValue::Array(vec![JsonValue::Bool(true)]);
        assert!(arr.is_array());
        arr.as_array_mut().unwrap().push(JsonValue::Null);
        assert_eq!(arr.as_array().unwrap().len(), 2);

        let mut taken = JsonValue::String("take".to_string());
        let prior = taken.take();
        assert!(matches!(taken, JsonValue::Null));
        assert_eq!(prior.as_str(), Some("take"));
    }

    #[rstest::rstest]
    fn test_json_value_indexing_success() {
        let mut arr = JsonValue::Array(vec![JsonValue::Number(Number::from(1)), JsonValue::Null]);
        assert_eq!(arr[0].as_i64(), Some(1));
        arr[1] = JsonValue::Bool(true);
        assert_eq!(arr[1].as_bool(), Some(true));

        let mut obj = IndexMap::new();
        obj.insert("key".to_string(), JsonValue::Bool(false));
        let mut value = JsonValue::Object(obj);

        assert_eq!(value["key"].as_bool(), Some(false));
        value["key"] = JsonValue::Bool(true);
        assert_eq!(value["key"].as_bool(), Some(true));

        let owned_key = "key".to_string();
        assert_eq!(value[owned_key].as_bool(), Some(true));
    }

    #[rstest::rstest]
    fn test_json_value_indexing_panics() {
        let value = JsonValue::Null;
        let err = catch_unwind(AssertUnwindSafe(|| {
            let _ = &value["missing"];
        }));
        assert!(err.is_err());

        let empty_array = JsonValue::Array(Vec::new());
        let err = catch_unwind(AssertUnwindSafe(|| {
            let _ = &empty_array[1];
        }));
        assert!(err.is_err());

        let mut not_array = JsonValue::Null;
        let err = catch_unwind(AssertUnwindSafe(|| {
            not_array[0] = JsonValue::Null;
        }));
        assert!(err.is_err());

        let empty_object = JsonValue::Object(IndexMap::new());
        let err = catch_unwind(AssertUnwindSafe(|| {
            let _ = &empty_object["absent"];
        }));
        assert!(err.is_err());
    }

    #[rstest::rstest]
    fn test_json_value_conversions() {
        let json_value = json!({"a": [1, 2], "b": {"c": true}});
        let value = JsonValue::from(json_value.clone());
        let roundtrip: serde_json::Value = value.clone().into();
        assert_eq!(roundtrip, json_value);

        let nan_value = JsonValue::Number(Number::Float(f64::NAN));
        let json_nan: serde_json::Value = nan_value.into();
        assert_eq!(json_nan, json!(null));
    }

    #[rstest::rstest]
    fn test_into_json_value_trait() {
        let json_value = json!({"a": 1});
        let owned = json_value.into_json_value();
        assert!(owned.is_object());

        let json_value = json!({"b": true});
        let borrowed = (&json_value).into_json_value();
        assert!(borrowed.is_object());

        let value = JsonValue::Bool(false);
        let cloned = value.into_json_value();
        assert!(matches!(cloned, JsonValue::Bool(false)));

        let value = JsonValue::Bool(true);
        let borrowed = (&value).into_json_value();
        assert!(matches!(borrowed, JsonValue::Bool(true)));
    }

    #[rstest::rstest]
    fn test_get_missing_key_returns_none() {
        let obj = JsonValue::Object(Default::default());
        assert!(obj.get("nonexistent").is_none());
    }

    #[rstest::rstest]
    fn test_get_on_non_object_returns_none() {
        let arr = JsonValue::Array(vec![]);
        assert!(arr.get("key").is_none());
    }

    #[rstest::rstest]
    fn test_get_index_out_of_bounds_returns_none() {
        let arr = JsonValue::Array(vec![]);
        assert!(arr.get_index(0).is_none());
    }

    #[rstest::rstest]
    fn test_get_index_on_non_array_returns_none() {
        let obj = JsonValue::Object(Default::default());
        assert!(obj.get_index(0).is_none());
    }
}
