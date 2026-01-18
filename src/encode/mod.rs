mod folding;
mod writer;
use indexmap::IndexMap;
use std::collections::HashSet;

use crate::{
    constants::MAX_DEPTH,
    types::{EncodeOptions, JsonValue as Value, KeyFoldingMode, Number, ToonError, ToonResult},
    utils::{validation::validate_depth, QuotingContext},
};

/// Encode any serializable value to TOON format.
///
/// This function accepts any type implementing `serde::Serialize`, including:
/// - Custom structs with `#[derive(Serialize)]`
/// - `serde_json::Value`
/// - Built-in types (Vec, HashMap, etc.)
///
/// # Examples
///
/// **With custom structs:**
/// ```
/// use serde::Serialize;
/// use serde_toon::{
///     encode,
///     EncodeOptions,
/// };
///
/// #[derive(Serialize)]
/// struct User {
///     name: String,
///     age: u32,
/// }
///
/// let user = User {
///     name: "Alice".to_string(),
///     age: 30,
/// };
/// let toon = encode(&user, &EncodeOptions::default())?;
/// assert!(toon.contains("name: Alice"));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
///
/// **With JSON values:**
/// ```
/// use serde_toon::{encode, EncodeOptions, Delimiter};
/// use serde_json::json;
///
/// let data = json!({"tags": ["a", "b", "c"]});
/// let options = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
/// let toon = encode(&data, &options)?;
/// assert!(toon.contains("|"));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn encode<T: serde::Serialize>(value: &T, options: &EncodeOptions) -> ToonResult<String> {
    let json_value = crate::serde::to_value(value)?;
    encode_impl(&json_value, options)
}

fn encode_impl(value: &Value, options: &EncodeOptions) -> ToonResult<String> {
    let mut writer = writer::Writer::new(options.clone());

    write_root(&mut writer, value)?;

    Ok(writer.finish())
}

pub(crate) fn encode_to_vec<T: serde::Serialize>(
    value: &T,
    options: &EncodeOptions,
) -> ToonResult<Vec<u8>> {
    let json_value = crate::serde::to_value(value)?;
    encode_impl_to_vec(&json_value, options)
}

fn encode_impl_to_vec(value: &Value, options: &EncodeOptions) -> ToonResult<Vec<u8>> {
    let mut writer = writer::Writer::new(options.clone());

    write_root(&mut writer, value)?;

    Ok(writer.finish_bytes())
}

/// Encode a `serde_json::Value` directly to TOON with options.
pub fn encode_value(value: &serde_json::Value, options: &EncodeOptions) -> ToonResult<String> {
    let internal = Value::from(value);
    encode_impl(&internal, options)
}

/// Encode a `serde_json::Value` directly to TOON with default options.
pub fn encode_value_default(value: &serde_json::Value) -> ToonResult<String> {
    encode_value(value, &EncodeOptions::default())
}

pub(crate) fn encode_value_to_vec(
    value: &serde_json::Value,
    options: &EncodeOptions,
) -> ToonResult<Vec<u8>> {
    let internal = Value::from(value);
    encode_impl_to_vec(&internal, options)
}

fn write_root(writer: &mut writer::Writer, value: &Value) -> ToonResult<()> {
    match value {
        Value::Array(arr) => write_array(writer, None, arr, 0),
        Value::Object(obj) => write_object(writer, obj, 0),
        _ => write_primitive_value(writer, value, QuotingContext::ObjectValue),
    }
}

/// Encode with default options (2-space indent, comma delimiter).
///
/// Works with any type implementing `serde::Serialize`.
///
/// # Examples
///
/// **With structs:**
/// ```
/// use serde::Serialize;
/// use serde_toon::encode_default;
///
/// #[derive(Serialize)]
/// struct Person {
///     name: String,
///     age: u32,
/// }
///
/// let person = Person {
///     name: "Alice".to_string(),
///     age: 30,
/// };
/// let toon = encode_default(&person)?;
/// assert!(toon.contains("name: Alice"));
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
///
/// **With JSON values:**
/// ```
/// use serde_toon::encode_default;
/// use serde_json::json;
///
/// let data = json!({"tags": ["reading", "gaming", "coding"]});
/// let toon = encode_default(&data)?;
/// assert_eq!(toon, "tags[3]: reading,gaming,coding");
/// # Ok::<(), serde_toon::ToonError>(())
/// ```
pub fn encode_default<T: serde::Serialize>(value: &T) -> ToonResult<String> {
    encode(value, &EncodeOptions::default())
}

#[cfg(test)]
pub(crate) fn encode_array(value: Value, options: &EncodeOptions) -> ToonResult<String> {
    match &value {
        Value::Array(_) => encode_impl(&value, options),
        other => Err(ToonError::type_mismatch("array", other.type_name())),
    }
}

#[cfg(test)]
pub(crate) fn encode_object(value: Value, options: &EncodeOptions) -> ToonResult<String> {
    match &value {
        Value::Object(_) => encode_impl(&value, options),
        other => Err(ToonError::type_mismatch("object", other.type_name())),
    }
}

fn write_object(
    writer: &mut writer::Writer,
    obj: &IndexMap<String, Value>,
    depth: usize,
) -> ToonResult<()> {
    write_object_impl(writer, obj, depth, false)
}

fn write_object_impl(
    writer: &mut writer::Writer,
    obj: &IndexMap<String, Value>,
    depth: usize,
    disable_folding: bool,
) -> ToonResult<()> {
    validate_depth(depth, MAX_DEPTH)?;

    let allow_folding = !disable_folding && writer.options.key_folding == KeyFoldingMode::Safe;
    let (key_set, prefix_conflicts) = if allow_folding {
        let mut key_set: HashSet<&str> = HashSet::with_capacity(obj.len());
        let mut prefix_conflicts: HashSet<&str> = HashSet::new();

        for key in obj.keys() {
            let key_str = key.as_str();
            key_set.insert(key_str);
            if key_str.contains('.') {
                let mut start = 0;
                while let Some(pos) = key_str[start..].find('.') {
                    let end = start + pos;
                    if end > 0 {
                        prefix_conflicts.insert(&key_str[..end]);
                    }
                    start = end + 1;
                }
            }
        }

        (key_set, prefix_conflicts)
    } else {
        (HashSet::new(), HashSet::new())
    };

    for (i, (key, value)) in obj.iter().enumerate() {
        if i > 0 {
            writer.write_newline()?;
        }

        let key_str = key.as_str();

        // Check if this key-value pair can be folded (v1.5 feature)
        // Don't fold if any sibling key is a dotted path starting with this key
        // (e.g., don't fold inside "data" if "data.meta.items" exists as a sibling)
        let has_conflicting_sibling =
            allow_folding && (key_str.contains('.') || prefix_conflicts.contains(key_str));

        let folded = if allow_folding && !has_conflicting_sibling {
            folding::analyze_foldable_chain(key_str, value, writer.options.flatten_depth, &key_set)
        } else {
            None
        };

        if let Some(chain) = folded {
            // Write folded key-value pair
            if depth > 0 {
                writer.write_indent(depth)?;
            }

            // Write the leaf value
            match chain.leaf_value {
                Value::Array(arr) => {
                    // For arrays, pass the folded key to write_array so it generates the header
                    // correctly
                    write_array(writer, Some(&chain.folded_key), arr, 0)?;
                }
                Value::Object(nested_obj) => {
                    // Write the folded key (e.g., "a.b.c")
                    writer.write_key(&chain.folded_key)?;
                    writer.write_char(':')?;
                    if !nested_obj.is_empty() {
                        writer.write_newline()?;
                        // After folding a chain, disable folding for the leaf object
                        // This respects flattenDepth and prevents over-folding
                        write_object_impl(writer, nested_obj, depth + 1, true)?;
                    }
                }
                _ => {
                    // Write the folded key (e.g., "a.b.c")
                    writer.write_key(&chain.folded_key)?;
                    writer.write_char(':')?;
                    writer.write_char(' ')?;
                    write_primitive_value(writer, chain.leaf_value, QuotingContext::ObjectValue)?;
                }
            }
        } else {
            // Standard (non-folded) encoding
            match value {
                Value::Array(arr) => {
                    write_array(writer, Some(key_str), arr, depth)?;
                }
                Value::Object(nested_obj) => {
                    if depth > 0 {
                        writer.write_indent(depth)?;
                    }
                    writer.write_key(key_str)?;
                    writer.write_char(':')?;
                    if !nested_obj.is_empty() {
                        writer.write_newline()?;
                        // If this key has a conflicting sibling, disable folding for its nested
                        // objects
                        let nested_disable_folding = disable_folding || has_conflicting_sibling;
                        write_object_impl(writer, nested_obj, depth + 1, nested_disable_folding)?;
                    }
                }
                _ => {
                    if depth > 0 {
                        writer.write_indent(depth)?;
                    }
                    writer.write_key(key_str)?;
                    writer.write_char(':')?;
                    writer.write_char(' ')?;
                    write_primitive_value(writer, value, QuotingContext::ObjectValue)?;
                }
            }
        }
    }

    Ok(())
}

fn write_array(
    writer: &mut writer::Writer,
    key: Option<&str>,
    arr: &[Value],
    depth: usize,
) -> ToonResult<()> {
    validate_depth(depth, MAX_DEPTH)?;

    if arr.is_empty() {
        writer.write_empty_array_with_key(key, depth)?;
        return Ok(());
    }

    // Select format based on array content: tabular (uniform objects) > inline
    // primitives > nested list
    match classify_array(arr) {
        ArrayKind::Tabular(keys) => encode_tabular_array(writer, key, arr, &keys, depth)?,
        ArrayKind::Primitive => encode_primitive_array(writer, key, arr, depth)?,
        ArrayKind::Nested => encode_nested_array(writer, key, arr, depth)?,
    }

    Ok(())
}

enum ArrayKind<'a> {
    Tabular(Vec<&'a str>),
    Primitive,
    Nested,
}

fn classify_array<'a>(arr: &'a [Value]) -> ArrayKind<'a> {
    let first = match arr.first() {
        Some(value) => value,
        None => return ArrayKind::Primitive,
    };

    if let Value::Object(first_obj) = first {
        if !first_obj.values().all(is_primitive) {
            return ArrayKind::Nested;
        }

        let keys: Vec<&str> = first_obj.keys().map(|key| key.as_str()).collect();

        for val in arr.iter().skip(1) {
            let obj = match val.as_object() {
                Some(obj) => obj,
                None => return ArrayKind::Nested,
            };

            if obj.len() != keys.len() {
                return ArrayKind::Nested;
            }

            for key in &keys {
                if !obj.contains_key(*key) {
                    return ArrayKind::Nested;
                }
            }

            if !obj.values().all(is_primitive) {
                return ArrayKind::Nested;
            }
        }

        return ArrayKind::Tabular(keys);
    }

    if arr.iter().all(is_primitive) {
        ArrayKind::Primitive
    } else {
        ArrayKind::Nested
    }
}

fn is_primitive(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn encode_primitive_array(
    writer: &mut writer::Writer,
    key: Option<&str>,
    arr: &[Value],
    depth: usize,
) -> ToonResult<()> {
    writer.write_array_header(key, arr.len(), None, depth)?;
    writer.write_char(' ')?;
    // Set delimiter context for array values (affects quoting decisions)
    writer.push_active_delimiter(writer.options.delimiter);

    for (i, val) in arr.iter().enumerate() {
        if i > 0 {
            writer.write_delimiter()?;
        }
        write_primitive_value(writer, val, QuotingContext::ArrayValue)?;
    }
    writer.pop_active_delimiter();

    Ok(())
}

#[cfg(test)]
mod extra_tests {
    use indexmap::IndexMap;
    use serde_json::json;

    use super::*;
    use crate::types::{JsonValue, Number};

    #[rstest::rstest]
    fn test_encode_array_and_object_with_json() {
        let array = json!(["a", "b"]);
        let encoded = encode_array(JsonValue::from(array), &EncodeOptions::default()).unwrap();
        assert!(encoded.starts_with("[2]:"));

        let object = json!({"a": 1});
        let encoded = encode_object(JsonValue::from(object), &EncodeOptions::default()).unwrap();
        assert!(encoded.contains("a: 1"));
    }

    #[rstest::rstest]
    fn test_encode_array_object_type_mismatch() {
        let err = encode_array(
            JsonValue::from(json!({"a": 1})),
            &EncodeOptions::default(),
        )
        .unwrap_err();
        match err {
            ToonError::TypeMismatch { expected, found } => {
                assert_eq!(expected, "array");
                assert_eq!(found, "object");
            }
            _ => panic!("Expected TypeMismatch for encode_array"),
        }

        let err = encode_object(
            JsonValue::from(json!(["a", "b"])),
            &EncodeOptions::default(),
        )
        .unwrap_err();
        match err {
            ToonError::TypeMismatch { expected, found } => {
                assert_eq!(expected, "object");
                assert_eq!(found, "array");
            }
            _ => panic!("Expected TypeMismatch for encode_object"),
        }
    }

    #[rstest::rstest]
    fn test_encode_array_object_with_json_value() {
        let value = JsonValue::Array(vec![JsonValue::Number(Number::from(1))]);
        let encoded = encode_array(value, &EncodeOptions::default()).unwrap();
        assert!(encoded.contains("1"));

        let mut obj = IndexMap::new();
        obj.insert("key".to_string(), JsonValue::Bool(true));
        let value = JsonValue::Object(obj);
        let encoded = encode_object(value, &EncodeOptions::default()).unwrap();
        assert!(encoded.contains("key: true"));
    }
}

fn write_primitive_value(
    writer: &mut writer::Writer,
    value: &Value,
    context: QuotingContext,
) -> ToonResult<()> {
    match value {
        Value::Null => writer.write_str("null"),
        Value::Bool(b) => writer.write_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            write_normalized_number(writer, n)
        }
        Value::String(s) => {
            if writer.needs_quoting(s, context) {
                writer.write_quoted_string(s)
            } else {
                writer.write_str(s)
            }
        }
        _ => Err(ToonError::InvalidInput(
            "Expected primitive value".to_string(),
        )),
    }
}

pub(super) fn write_normalized_number(writer: &mut writer::Writer, n: &Number) -> ToonResult<()> {
    match n {
        Number::Float(f) if f.is_nan() || f.is_infinite() => writer.write_str("null"),
        Number::Float(f) if *f == 0.0 && f.is_sign_negative() => writer.write_str("0"),
        Number::NegInt(0) => writer.write_str("0"),
        _ => writer.write_canonical_number(n),
    }
}

fn encode_tabular_array(
    writer: &mut writer::Writer,
    key: Option<&str>,
    arr: &[Value],
    keys: &[&str],
    depth: usize,
) -> ToonResult<()> {
    writer.write_array_header(key, arr.len(), Some(keys), depth)?;
    writer.write_newline()?;

    writer.push_active_delimiter(writer.options.delimiter);

    // Write each row with values separated by delimiters
    for (row_index, obj_val) in arr.iter().enumerate() {
        if let Some(obj) = obj_val.as_object() {
            writer.write_indent(depth + 1)?;

            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    writer.write_delimiter()?;
                }

                // Missing fields become null
                if let Some(val) = obj.get(*key) {
                    write_primitive_value(writer, val, QuotingContext::ArrayValue)?;
                } else {
                    writer.write_str("null")?;
                }
            }

            if row_index < arr.len() - 1 {
                writer.write_newline()?;
            }
        }
    }

    Ok(())
}

fn encode_list_item_tabular_array(
    writer: &mut writer::Writer,
    arr: &[Value],
    keys: &[&str],
    depth: usize,
) -> ToonResult<()> {
    // Write array header without key (key already written on hyphen line)
    writer.write_char('[')?;
    writer.write_usize(arr.len())?;

    if writer.options.delimiter != crate::types::Delimiter::Comma {
        writer.write_char(writer.options.delimiter.as_char())?;
    }

    writer.write_char(']')?;

    // Write field list for tabular arrays: {field1,field2}
    writer.write_char('{')?;
    for (i, field) in keys.iter().enumerate() {
        if i > 0 {
            writer.write_char(writer.options.delimiter.as_char())?;
        }
        writer.write_key(field)?;
    }
    writer.write_char('}')?;
    writer.write_char(':')?;
    writer.write_newline()?;

    writer.push_active_delimiter(writer.options.delimiter);

    // Write rows at depth + 2 (relative to hyphen line)
    // The hyphen line is at depth, so rows appear at depth + 2
    for (row_index, obj_val) in arr.iter().enumerate() {
        if let Some(obj) = obj_val.as_object() {
            writer.write_indent(depth + 2)?;

            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    writer.write_delimiter()?;
                }

                // Missing fields become null
                if let Some(val) = obj.get(*key) {
                    write_primitive_value(writer, val, QuotingContext::ArrayValue)?;
                } else {
                    writer.write_str("null")?;
                }
            }

            if row_index < arr.len() - 1 {
                writer.write_newline()?;
            }
        }
    }

    writer.pop_active_delimiter();

    Ok(())
}

fn encode_nested_array(
    writer: &mut writer::Writer,
    key: Option<&str>,
    arr: &[Value],
    depth: usize,
) -> ToonResult<()> {
    writer.write_array_header(key, arr.len(), None, depth)?;
    writer.write_newline()?;
    writer.push_active_delimiter(writer.options.delimiter);

    for (i, val) in arr.iter().enumerate() {
        writer.write_indent(depth + 1)?;
        writer.write_char('-')?;

        match val {
            Value::Array(inner_arr) => {
                writer.write_char(' ')?;
                write_array(writer, None, inner_arr, depth + 1)?;
            }
            Value::Object(obj) => {
                // Objects in list items: first field on same line as "- ", rest indented
                // For empty objects, write only the hyphen (no space)
                let mut iter = obj.iter();
                if let Some((first_key, first_val)) = iter.next() {
                    writer.write_char(' ')?;

                    match first_val {
                        Value::Array(arr) => {
                            // Arrays as first field of list items require special indentation
                            // (depth +2 relative to hyphen) for their nested content
                            // (rows for tabular, items for non-uniform)
                            writer.write_key(first_key)?;
                            if arr.is_empty() {
                                writer.write_empty_array_with_key(None, depth + 2)?;
                            } else {
                                match classify_array(arr) {
                                    ArrayKind::Tabular(keys) => {
                                        // Tabular array: write inline with correct indentation
                                        encode_list_item_tabular_array(
                                            writer,
                                            arr,
                                            &keys,
                                            depth + 1,
                                        )?;
                                    }
                                    ArrayKind::Primitive => {
                                        // Non-tabular array: write with depth offset
                                        // (items at depth +2 instead of depth +1)
                                        encode_primitive_array(writer, None, arr, depth + 2)?;
                                    }
                                    ArrayKind::Nested => {
                                        // Non-tabular array: write with depth offset
                                        // (items at depth +2 instead of depth +1)
                                        encode_nested_array(writer, None, arr, depth + 2)?;
                                    }
                                }
                            }
                        }
                        Value::Object(nested_obj) => {
                            writer.write_key(first_key)?;
                            writer.write_char(':')?;
                            if !nested_obj.is_empty() {
                                writer.write_newline()?;
                                write_object(writer, nested_obj, depth + 3)?;
                            }
                        }
                        _ => {
                            writer.write_key(first_key)?;
                            writer.write_char(':')?;
                            writer.write_char(' ')?;
                            write_primitive_value(writer, first_val, QuotingContext::ObjectValue)?;
                        }
                    }

                    // Remaining fields on separate lines with proper indentation
                    for (key, value) in iter {
                        writer.write_newline()?;
                        writer.write_indent(depth + 2)?;

                        match value {
                            Value::Array(arr) => {
                                writer.write_key(key)?;
                                write_array(writer, None, arr, depth + 2)?;
                            }
                            Value::Object(nested_obj) => {
                                writer.write_key(key)?;
                                writer.write_char(':')?;
                                if !nested_obj.is_empty() {
                                    writer.write_newline()?;
                                    write_object(writer, nested_obj, depth + 3)?;
                                }
                            }
                            _ => {
                                writer.write_key(key)?;
                                writer.write_char(':')?;
                                writer.write_char(' ')?;
                                write_primitive_value(writer, value, QuotingContext::ObjectValue)?;
                            }
                        }
                    }
                }
            }
            _ => {
                writer.write_char(' ')?;
                write_primitive_value(writer, val, QuotingContext::ArrayValue)?;
            }
        }

        if i < arr.len() - 1 {
            writer.write_newline()?;
        }
    }
    writer.pop_active_delimiter();

    Ok(())
}

#[cfg(test)]
mod tests {
    use core::f64;

    use serde_json::json;

    use super::*;

    #[rstest::rstest]
    fn test_encode_null() {
        let value = json!(null);
        assert_eq!(encode_default(&value).unwrap(), "null");
    }

    #[rstest::rstest]
    fn test_encode_bool() {
        assert_eq!(encode_default(&json!(true)).unwrap(), "true");
        assert_eq!(encode_default(&json!(false)).unwrap(), "false");
    }

    #[rstest::rstest]
    fn test_encode_number() {
        assert_eq!(encode_default(&json!(42)).unwrap(), "42");
        assert_eq!(
            encode_default(&json!(f64::consts::PI)).unwrap(),
            "3.141592653589793"
        );
        assert_eq!(encode_default(&json!(-5)).unwrap(), "-5");
    }

    #[rstest::rstest]
    fn test_encode_string() {
        assert_eq!(encode_default(&json!("hello")).unwrap(), "hello");
        assert_eq!(
            encode_default(&json!("hello world")).unwrap(),
            "hello world"
        );
    }

    #[rstest::rstest]
    fn test_encode_simple_object() {
        let obj = json!({"name": "Alice", "age": 30});
        let result = encode_default(&obj).unwrap();
        assert!(result.contains("name: Alice"));
        assert!(result.contains("age: 30"));
    }

    #[rstest::rstest]
    fn test_encode_primitive_array() {
        let obj = json!({"tags": ["reading", "gaming", "coding"]});
        let result = encode_default(&obj).unwrap();
        assert_eq!(result, "tags[3]: reading,gaming,coding");
    }

    #[rstest::rstest]
    fn test_encode_tabular_array() {
        let obj = json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        });
        let result = encode_default(&obj).unwrap();
        assert!(result.contains("users[2]{id,name}:"));
        assert!(result.contains("1,Alice"));
        assert!(result.contains("2,Bob"));
    }

    #[rstest::rstest]
    fn test_encode_empty_array() {
        let obj = json!({"items": []});
        let result = encode_default(&obj).unwrap();
        assert_eq!(result, "items[0]:");
    }

    #[rstest::rstest]
    fn test_encode_nested_object() {
        let obj = json!({
            "user": {
                "name": "Alice",
                "age": 30
            }
        });
        let result = encode_default(&obj).unwrap();
        assert!(result.contains("user:"));
        assert!(result.contains("name: Alice"));
        assert!(result.contains("age: 30"));
    }

    #[rstest::rstest]
    fn test_encode_list_item_tabular_array_v3() {
        let obj = json!({
            "items": [
                {
                    "users": [
                        {"id": 1, "name": "Ada"},
                        {"id": 2, "name": "Bob"}
                    ],
                    "status": "active"
                }
            ]
        });

        let result = encode_default(&obj).unwrap();

        assert!(
            result.contains("  - users[2]{id,name}:"),
            "Header should be on hyphen line"
        );

        assert!(
            result.contains("      1,Ada"),
            "First row should be at 6 spaces (depth +2 from hyphen). Got:\n{}",
            result
        );
        assert!(
            result.contains("      2,Bob"),
            "Second row should be at 6 spaces (depth +2 from hyphen). Got:\n{}",
            result
        );

        assert!(
            result.contains("    status: active"),
            "Sibling field should be at 4 spaces (depth +1 from hyphen). Got:\n{}",
            result
        );
    }

    #[rstest::rstest]
    fn test_encode_list_item_tabular_array_multiple_items() {
        let obj = json!({
            "data": [
                {
                    "records": [
                        {"id": 1, "val": "x"}
                    ],
                    "count": 1
                },
                {
                    "records": [
                        {"id": 2, "val": "y"}
                    ],
                    "count": 1
                }
            ]
        });

        let result = encode_default(&obj).unwrap();

        let lines: Vec<&str> = result.lines().collect();

        let row_lines: Vec<&str> = lines
            .iter()
            .filter(|line| line.trim().starts_with(char::is_numeric))
            .copied()
            .collect();

        for row in row_lines {
            let spaces = row.len() - row.trim_start().len();
            assert_eq!(
                spaces, 6,
                "Tabular rows should be at 6 spaces. Found {} spaces in: {}",
                spaces, row
            );
        }
    }

    #[rstest::rstest]
    fn test_encode_list_item_non_tabular_array_unchanged() {
        let obj = json!({
            "items": [
                {
                    "tags": ["a", "b", "c"],
                    "name": "test"
                }
            ]
        });

        let result = encode_default(&obj).unwrap();

        assert!(
            result.contains("  - tags[3]: a,b,c"),
            "Inline array should be on hyphen line. Got:\n{}",
            result
        );

        assert!(
            result.contains("    name: test"),
            "Sibling field should be at 4 spaces. Got:\n{}",
            result
        );
    }

    #[rstest::rstest]
    fn test_encode_list_item_tabular_array_with_nested_fields() {
        let obj = json!({
            "entries": [
                {
                    "people": [
                        {"name": "Alice", "age": 30},
                        {"name": "Bob", "age": 25}
                    ],
                    "total": 2,
                    "category": "staff"
                }
            ]
        });

        let result = encode_default(&obj).unwrap();

        assert!(result.contains("  - people[2]{name,age}:"));

        assert!(result.contains("      Alice,30"));
        assert!(result.contains("      Bob,25"));

        assert!(result.contains("    total: 2"));
        assert!(result.contains("    category: staff"));
    }
}
