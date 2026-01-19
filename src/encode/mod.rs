use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;

use serde::Serialize;
use serde_json::Value;
#[cfg(feature = "parallel")]
use smallvec::SmallVec;
use smol_str::SmolStr;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::num::number::append_json_number_bytes;
use crate::text::string::{
    analyze_string, escape_string_into, escape_string_into_bytes, is_canonical_unquoted_key,
    is_identifier_segment, ByteSink,
};
use crate::{EncodeOptions, Error, Indent, Result};

const STRING_CACHE_MAX_LEN: usize = 64;
const STRING_CACHE_MAX_ITEMS: usize = 1024;
const KEY_CACHE_MAX_LEN: usize = 64;
const KEY_CACHE_MAX_ITEMS: usize = 1024;
const LARGE_CONTAINER_THRESHOLD: usize = 64;
const MAX_RESERVE_DEPTH: usize = 2;
const RESERVE_SAMPLE_ITEMS: usize = 4;
const PRECOMPUTE_SAMPLE_ITEMS: usize = 4;
const PRECOMPUTE_MAX_ROWS: usize = 128;
const PRECOMPUTE_MAX_STRINGS: usize = 2048;
const TABULAR_STRING_CACHE_MAX_ITEMS: usize = 512;
const NUMBER_CACHE_MAX_LEN: usize = 32;
const TABULAR_NUMBER_CACHE_MAX_ITEMS: usize = 512;
const TABULAR_PREFIXED_CACHE_MAX_ITEMS: usize = 512;
#[cfg(feature = "parallel")]
const PARALLEL_TABULAR_MIN_ROWS: usize = 256;
#[cfg(feature = "parallel")]
const PARALLEL_TABULAR_MIN_CELLS: usize = 2048;

#[cfg(feature = "parallel")]
type RowBuf = SmallVec<[u8; 256]>;

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
enum NumberKey {
    I64(i64),
    U64(u64),
    F64(u64),
}

#[derive(Default)]
struct TabularLastCache {
    string_key: Option<SmolStr>,
    string_bytes: Vec<u8>,
    number_key: Option<NumberKey>,
    number_bytes: Vec<u8>,
}

impl TabularLastCache {
    fn clear(&mut self) {
        self.string_key = None;
        self.string_bytes.clear();
        self.number_key = None;
        self.number_bytes.clear();
    }
}

fn number_cache_key(number: &serde_json::Number) -> Option<NumberKey> {
    if let Some(value) = number.as_i64() {
        return Some(NumberKey::I64(value));
    }
    if let Some(value) = number.as_u64() {
        return Some(NumberKey::U64(value));
    }
    number.as_f64().map(|value| NumberKey::F64(value.to_bits()))
}

thread_local! {
    static ENCODER_POOL: RefCell<Encoder> = RefCell::new(Encoder::new(&EncodeOptions::default()));
}

pub fn to_string<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|err| Error::serialize_with_source(format!("serialize failed: {err}"), err))?;
    let bytes = encode_value(&value, options)?;
    bytes_to_string(bytes)
}

pub fn to_string_into<T: Serialize>(
    value: &T,
    options: &EncodeOptions,
    out: &mut String,
) -> Result<()> {
    let value = serde_json::to_value(value)
        .map_err(|err| Error::serialize_with_source(format!("serialize failed: {err}"), err))?;
    let bytes = encode_value(&value, options)?;
    let encoded = unsafe { std::str::from_utf8_unchecked(&bytes) };
    out.clear();
    out.reserve(encoded.len());
    out.push_str(encoded);
    Ok(())
}

pub fn to_string_from_json_str(input: &str, options: &EncodeOptions) -> Result<String> {
    let value: Value = serde_json::from_str(input)
        .map_err(|err| Error::serialize_with_source(format!("invalid json: {err}"), err))?;
    let bytes = encode_value(&value, options)?;
    bytes_to_string(bytes)
}

pub fn to_vec<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)
        .map_err(|err| Error::serialize_with_source(format!("serialize failed: {err}"), err))?;
    encode_value(&value, options)
}

pub fn to_writer<T: Serialize, W: Write>(
    mut writer: W,
    value: &T,
    options: &EncodeOptions,
) -> Result<()> {
    let bytes = to_vec(value, options)?;
    writer
        .write_all(&bytes)
        .map_err(|err| Error::encode_with_source(format!("write failed: {err}"), err))?;
    Ok(())
}

fn bytes_to_string(bytes: Vec<u8>) -> Result<String> {
    debug_assert!(
        std::str::from_utf8(&bytes).is_ok(),
        "encoder emitted non-utf8 bytes"
    );
    Ok(unsafe { String::from_utf8_unchecked(bytes) })
}

fn encode_value(value: &Value, options: &EncodeOptions) -> Result<Vec<u8>> {
    ENCODER_POOL.with(|pool| {
        let mut encoder = pool.borrow_mut();
        encoder.reset(options);
        encoder.reserve_for_value(value);
        encoder.precompute_string_flags(value);
        encoder.encode_root(value)?;
        Ok(encoder.take_bytes())
    })
}

struct Encoder {
    document_delimiter: char,
    key_folding: bool,
    flatten_depth: usize,
    indent_unit: Vec<u8>,
    indent_cache: Vec<Vec<u8>>,
    delimiter_stack: Vec<char>,
    string_cache: HashMap<char, HashMap<SmolStr, (bool, bool)>>,
    key_cache: HashMap<SmolStr, String>,
    tabular_string_cache: HashMap<char, HashMap<SmolStr, Vec<u8>>>,
    tabular_prefixed_string_cache: HashMap<char, HashMap<SmolStr, Vec<u8>>>,
    tabular_number_cache: HashMap<NumberKey, Vec<u8>>,
    tabular_prefixed_number_cache: HashMap<char, HashMap<NumberKey, Vec<u8>>>,
    tabular_last_values: Vec<TabularLastCache>,
    key_intern: HashMap<String, usize>,
    interned_keys: Vec<String>,
    line_buf: Vec<u8>,
    out: Vec<u8>,
}

impl Encoder {
    fn new(options: &EncodeOptions) -> Self {
        let Indent::Spaces(indent_size) = options.indent;
        let indent_unit = vec![b' '; indent_size];
        Self {
            document_delimiter: options.delimiter.as_char(),
            key_folding: matches!(options.key_folding, crate::options::KeyFolding::Safe),
            flatten_depth: options.flatten_depth.unwrap_or(usize::MAX),
            indent_unit,
            indent_cache: vec![Vec::new()],
            delimiter_stack: Vec::new(),
            string_cache: HashMap::with_capacity(4),
            key_cache: HashMap::with_capacity(KEY_CACHE_MAX_ITEMS),
            tabular_string_cache: HashMap::with_capacity(4),
            tabular_prefixed_string_cache: HashMap::with_capacity(4),
            tabular_number_cache: HashMap::with_capacity(TABULAR_NUMBER_CACHE_MAX_ITEMS),
            tabular_prefixed_number_cache: HashMap::with_capacity(4),
            tabular_last_values: Vec::new(),
            key_intern: HashMap::new(),
            interned_keys: Vec::new(),
            line_buf: Vec::with_capacity(128),
            out: Vec::with_capacity(128),
        }
    }

    fn reset(&mut self, options: &EncodeOptions) {
        let Indent::Spaces(indent_size) = options.indent;
        self.document_delimiter = options.delimiter.as_char();
        self.key_folding = matches!(options.key_folding, crate::options::KeyFolding::Safe);
        self.flatten_depth = options.flatten_depth.unwrap_or(usize::MAX);
        if self.indent_unit.len() != indent_size {
            self.indent_unit.clear();
            self.indent_unit.resize(indent_size, b' ');
            self.indent_cache.clear();
            self.indent_cache.push(Vec::new());
        }
        self.delimiter_stack.clear();
        self.key_intern.clear();
        self.interned_keys.clear();
        self.line_buf.clear();
        self.out.clear();
        self.tabular_last_values.clear();
    }

    fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    fn active_delimiter(&self) -> char {
        self.delimiter_stack
            .last()
            .copied()
            .unwrap_or(self.document_delimiter)
    }

    fn with_array_delimiter<F, R>(&mut self, delimiter: char, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.delimiter_stack.push(delimiter);
        let result = f(self);
        self.delimiter_stack.pop();
        result
    }

    fn reserve_for_value(&mut self, value: &Value) {
        self.reserve_for_value_inner(value, 0);
    }

    fn reserve_for_value_inner(&mut self, value: &Value, depth: usize) {
        if depth > MAX_RESERVE_DEPTH {
            return;
        }
        match value {
            Value::Array(array) => {
                let len = array.len();
                if len >= LARGE_CONTAINER_THRESHOLD {
                    self.out.reserve(len.saturating_mul(8));
                    for item in array.iter().take(RESERVE_SAMPLE_ITEMS) {
                        self.reserve_for_value_inner(item, depth + 1);
                    }
                }
            }
            Value::Object(map) => {
                let len = map.len();
                if len >= LARGE_CONTAINER_THRESHOLD {
                    self.out.reserve(len.saturating_mul(12));
                    for (_, value) in map.iter().take(RESERVE_SAMPLE_ITEMS) {
                        self.reserve_for_value_inner(value, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn precompute_string_flags(&mut self, value: &Value) {
        self.precompute_string_flags_inner(value);
    }

    fn precompute_string_flags_inner(&mut self, value: &Value) {
        match value {
            Value::String(value) => self.precompute_small_string(value),
            Value::Array(array) => self.precompute_array_strings(array),
            Value::Object(map) => {
                for value in map.values() {
                    match value {
                        Value::String(value) => self.precompute_small_string(value),
                        Value::Array(array) => self.precompute_array_strings(array),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn precompute_small_string(&mut self, value: &str) {
        if value.len() <= STRING_CACHE_MAX_LEN {
            let _ = self.analyze_string_cached(value, self.document_delimiter);
        }
    }

    fn precompute_array_strings(&mut self, array: &[Value]) {
        if array.is_empty() {
            return;
        }
        let mut strings_seen = 0usize;
        if array.len() <= PRECOMPUTE_SAMPLE_ITEMS && array.iter().all(is_scalar) {
            for item in array {
                if let Value::String(value) = item {
                    if strings_seen >= PRECOMPUTE_MAX_STRINGS {
                        return;
                    }
                    self.precompute_small_string(value);
                    strings_seen += 1;
                }
            }
            return;
        }

        let Some(fields) = self.sample_tabular_fields(array) else {
            return;
        };

        for item in array.iter().take(PRECOMPUTE_MAX_ROWS) {
            let Some(row) = item.as_object() else {
                return;
            };
            if row.len() != fields.len() {
                return;
            }
            for key in &fields {
                let Some(value) = row.get(*key) else {
                    return;
                };
                if !is_scalar(value) {
                    return;
                }
                if let Value::String(value) = value {
                    if strings_seen >= PRECOMPUTE_MAX_STRINGS {
                        return;
                    }
                    self.precompute_small_string(value);
                    strings_seen += 1;
                }
            }
        }
    }

    fn sample_tabular_fields<'a>(&self, array: &'a [Value]) -> Option<Vec<&'a str>> {
        let first = array.first()?.as_object()?;
        if first.is_empty() {
            return None;
        }
        let fields: Vec<&str> = first.keys().map(|key| key.as_str()).collect();
        let sample = if array.len() > PRECOMPUTE_MAX_ROWS {
            2
        } else {
            array.len().min(PRECOMPUTE_SAMPLE_ITEMS)
        };
        for item in array.iter().take(sample) {
            let row = item.as_object()?;
            if row.len() != fields.len() {
                return None;
            }
            for key in &fields {
                let value = row.get(*key)?;
                if !is_scalar(value) {
                    return None;
                }
            }
        }
        Some(fields)
    }

    fn encode_root(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::Object(map) => self.encode_object(map, 0),
            Value::Array(array) => self.encode_array_value(array, 0, None, b""),
            _ => self.with_line_buf(|encoder, line| -> Result<()> {
                line.clear();
                encoder.append_scalar_document(line, value)?;
                encoder.write_line_bytes(0, line);
                Ok(())
            }),
        }
    }

    fn encode_object(
        &mut self,
        map: &serde_json::Map<String, Value>,
        indent_level: usize,
    ) -> Result<()> {
        self.reserve_object_entries(map.len());
        let mut siblings = HashSet::with_capacity(map.len());
        for key in map.keys() {
            siblings.insert(key.as_str());
        }
        for (key, value) in map.iter() {
            if let Some((folded_key, folded_value)) = self.fold_key_value(key, value, &siblings) {
                self.encode_object_entry(&folded_key, folded_value, indent_level)?;
            } else {
                self.encode_object_entry(key, value, indent_level)?;
            }
        }
        Ok(())
    }

    fn encode_object_entry(&mut self, key: &str, value: &Value, indent_level: usize) -> Result<()> {
        match value {
            Value::Array(array) => self.encode_array_value(array, indent_level, Some(key), b""),
            Value::Object(map) => {
                self.with_line_buf(|encoder, line| {
                    line.clear();
                    encoder.append_encoded_key(line, key);
                    line.push(b':');
                    encoder.write_line_bytes(indent_level, line);
                });
                self.encode_object(map, indent_level + 1)
            }
            _ => self.with_line_buf(|encoder, line| -> Result<()> {
                line.clear();
                encoder.append_encoded_key(line, key);
                line.extend_from_slice(b": ");
                encoder.append_scalar_document(line, value)?;
                encoder.write_line_bytes(indent_level, line);
                Ok(())
            }),
        }
    }

    fn fold_key_value<'a>(
        &self,
        key: &str,
        value: &'a Value,
        siblings: &HashSet<&str>,
    ) -> Option<(String, &'a Value)> {
        if !self.key_folding || self.flatten_depth < 2 {
            return None;
        }

        let mut segments = vec![key];
        let mut cursor = value;
        while let Value::Object(map) = cursor {
            if map.len() != 1 {
                break;
            }
            let (next_key, next_val) = map.iter().next()?;
            segments.push(next_key.as_str());
            cursor = next_val;
        }

        let chain_len = segments.len();
        let depth = std::cmp::min(chain_len, self.flatten_depth);
        if depth < 2 {
            return None;
        }
        if !segments[..depth]
            .iter()
            .all(|segment| is_identifier_segment(segment))
        {
            return None;
        }

        let folded = segments[..depth].join(".");
        if siblings.contains(folded.as_str()) {
            return None;
        }

        let mut folded_value = value;
        for _ in 1..depth {
            if let Value::Object(map) = folded_value {
                let (_, next_val) = map.iter().next()?;
                folded_value = next_val;
            } else {
                return None;
            }
        }

        Some((folded, folded_value))
    }

    fn encode_array_value(
        &mut self,
        array: &[Value],
        indent_level: usize,
        key: Option<&str>,
        prefix: &[u8],
    ) -> Result<()> {
        let delimiter = self.active_delimiter();
        self.with_array_delimiter(delimiter, |encoder| {
            encoder.encode_array_value_inner(array, indent_level, key, prefix)
        })
    }

    fn encode_array_value_inner(
        &mut self,
        array: &[Value],
        indent_level: usize,
        key: Option<&str>,
        prefix: &[u8],
    ) -> Result<()> {
        if let Some(fields) = self.tabular_fields(array) {
            self.with_line_buf(|encoder, line| {
                line.clear();
                encoder.append_array_header(line, array.len(), key, Some(&fields));
                line.push(b':');
                encoder.write_line_with_prefix_bytes(indent_level, prefix, line);
            });
            self.reserve_tabular_rows(array.len(), fields.len());
            let mut row_indent = indent_level + 1;
            if prefix == b"- " && key.is_some() {
                row_indent += 1;
            }
            let delimiter_char = self.active_delimiter();
            #[cfg(feature = "parallel")]
            if self.should_parallel_tabular(array.len(), fields.len()) {
                let field_names: Vec<SmolStr> = fields
                    .iter()
                    .map(|field| SmolStr::new(self.interned_key(*field)))
                    .collect();
                let results: Vec<Result<RowBuf>> = array
                    .par_iter()
                    .map_init(
                        || RowEncoder::new(delimiter_char),
                        |encoder, item| encoder.encode_tabular_row(item, &field_names),
                    )
                    .collect();
                for result in results {
                    let row = result?;
                    self.write_line_bytes(row_indent, &row);
                }
                return Ok(());
            }

            let field_names: Vec<SmolStr> = fields
                .iter()
                .map(|field| SmolStr::new(self.interned_key(*field)))
                .collect();
            self.with_out_buf(|encoder, out| -> Result<()> {
                encoder.reset_tabular_last_values(field_names.len());
                for item in array {
                    let obj = item
                        .as_object()
                        .ok_or_else(|| Error::encode("tabular row is not an object"))?;
                    let mut iter = field_names.iter().enumerate();
                    let Some((_, first_field)) = iter.next() else {
                        continue;
                    };
                    encoder.begin_line_with_prefix_into(out, row_indent, b"");
                    let value = obj
                        .get(first_field.as_str())
                        .ok_or_else(|| Error::encode("tabular row missing field"))?;
                    encoder.append_scalar_tabular(out, value, delimiter_char)?;
                    for (idx, field) in iter {
                        let value = obj
                            .get(field.as_str())
                            .ok_or_else(|| Error::encode("tabular row missing field"))?;
                        encoder.append_scalar_tabular_prefixed(out, value, delimiter_char, idx)?;
                    }
                }
                Ok(())
            })?;
            return Ok(());
        }

        if array.iter().all(is_scalar) {
            self.reserve_inline_array(array.len());
            self.with_line_buf(|encoder, line| -> Result<()> {
                line.clear();
                encoder.append_array_header(line, array.len(), key, None);
                if array.is_empty() {
                    line.push(b':');
                } else {
                    line.extend_from_slice(b": ");
                    encoder.append_inline_scalars(line, array)?;
                }
                encoder.write_line_with_prefix_bytes(indent_level, prefix, line);
                Ok(())
            })?;
            return Ok(());
        }

        self.with_line_buf(|encoder, line| {
            line.clear();
            encoder.append_array_header(line, array.len(), key, None);
            line.push(b':');
            encoder.write_line_with_prefix_bytes(indent_level, prefix, line);
        });
        let mut item_indent = indent_level + 1;
        if prefix == b"- " && key.is_some() {
            item_indent += 1;
        }
        for item in array {
            self.encode_list_item(item, item_indent)?;
        }
        Ok(())
    }

    fn encode_list_item(&mut self, value: &Value, indent_level: usize) -> Result<()> {
        match value {
            Value::Array(array) => self.encode_array_value(array, indent_level, None, b"- "),
            Value::Object(map) => self.encode_object_item(map, indent_level),
            _ => self.with_line_buf(|encoder, line| -> Result<()> {
                line.clear();
                encoder.append_scalar_document(line, value)?;
                encoder.write_line_with_prefix_bytes(indent_level, b"- ", line);
                Ok(())
            }),
        }
    }

    fn encode_object_item(
        &mut self,
        map: &serde_json::Map<String, Value>,
        indent_level: usize,
    ) -> Result<()> {
        let mut iter = map.iter();
        let Some((first_key, first_value)) = iter.next() else {
            self.write_line_with_prefix_bytes(indent_level, b"-", b"");
            return Ok(());
        };

        match first_value {
            Value::Array(array) => {
                self.encode_array_value(array, indent_level, Some(first_key), b"- ")?;
            }
            Value::Object(nested) => {
                self.with_line_buf(|encoder, line| {
                    line.clear();
                    encoder.append_encoded_key(line, first_key);
                    line.push(b':');
                    encoder.write_line_with_prefix_bytes(indent_level, b"- ", line);
                });
                self.encode_object(nested, indent_level + 1)?;
            }
            _ => {
                self.with_line_buf(|encoder, line| -> Result<()> {
                    line.clear();
                    encoder.append_encoded_key(line, first_key);
                    line.extend_from_slice(b": ");
                    encoder.append_scalar_document(line, first_value)?;
                    encoder.write_line_with_prefix_bytes(indent_level, b"- ", line);
                    Ok(())
                })?;
            }
        }

        for (key, value) in iter {
            self.encode_object_entry(key, value, indent_level + 1)?;
        }
        Ok(())
    }

    fn append_scalar_with_delimiter<B: ByteSink>(
        &mut self,
        buf: &mut B,
        value: &Value,
        delimiter: char,
    ) -> Result<()> {
        match value {
            Value::Null => {
                buf.extend_bytes(b"null");
                Ok(())
            }
            Value::Bool(value) => {
                if *value {
                    buf.extend_bytes(b"true");
                } else {
                    buf.extend_bytes(b"false");
                }
                Ok(())
            }
            Value::Number(number) => {
                append_json_number_bytes(buf, number);
                Ok(())
            }
            Value::String(value) => {
                self.append_string(buf, value, delimiter);
                Ok(())
            }
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn append_scalar_document<B: ByteSink>(&mut self, buf: &mut B, value: &Value) -> Result<()> {
        self.append_scalar_with_delimiter(buf, value, self.document_delimiter)
    }

    fn append_scalar_active<B: ByteSink>(&mut self, buf: &mut B, value: &Value) -> Result<()> {
        let delimiter = self.active_delimiter();
        self.append_scalar_with_delimiter(buf, value, delimiter)
    }

    fn append_scalar_tabular<B: ByteSink>(
        &mut self,
        buf: &mut B,
        value: &Value,
        delimiter: char,
    ) -> Result<()> {
        match value {
            Value::Null => {
                buf.extend_bytes(b"null");
                Ok(())
            }
            Value::Bool(value) => {
                if *value {
                    buf.extend_bytes(b"true");
                } else {
                    buf.extend_bytes(b"false");
                }
                Ok(())
            }
            Value::Number(number) => {
                if let Some(key) = number_cache_key(number) {
                    if let Some(encoded) = self.tabular_number_cache.get(&key) {
                        buf.extend_bytes(encoded);
                        return Ok(());
                    }
                    let start = buf.len();
                    append_json_number_bytes(buf, number);
                    let len = buf.len() - start;
                    if len <= NUMBER_CACHE_MAX_LEN
                        && self.tabular_number_cache.len() < TABULAR_NUMBER_CACHE_MAX_ITEMS
                    {
                        self.tabular_number_cache
                            .insert(key, buf.as_slice()[start..].to_vec());
                    }
                    return Ok(());
                }
                append_json_number_bytes(buf, number);
                Ok(())
            }
            Value::String(value) => {
                if value.len() <= STRING_CACHE_MAX_LEN {
                    let cached = self
                        .tabular_string_cache
                        .get(&delimiter)
                        .and_then(|cache| cache.get(value.as_str()));
                    if let Some(encoded) = cached {
                        buf.extend_bytes(encoded);
                        return Ok(());
                    }
                    let start = buf.len();
                    self.append_string(buf, value, delimiter);
                    let cache = self
                        .tabular_string_cache
                        .entry(delimiter)
                        .or_insert_with(|| HashMap::with_capacity(TABULAR_STRING_CACHE_MAX_ITEMS));
                    if cache.len() < TABULAR_STRING_CACHE_MAX_ITEMS {
                        cache.insert(SmolStr::new(value), buf.as_slice()[start..].to_vec());
                    }
                    return Ok(());
                }
                self.append_string(buf, value, delimiter);
                Ok(())
            }
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn append_scalar_tabular_prefixed<B: ByteSink>(
        &mut self,
        buf: &mut B,
        value: &Value,
        delimiter: char,
        column: usize,
    ) -> Result<()> {
        let delimiter_byte = delimiter as u8;
        match value {
            Value::Null => {
                buf.push_byte(delimiter_byte);
                buf.extend_bytes(b"null");
                Ok(())
            }
            Value::Bool(value) => {
                buf.push_byte(delimiter_byte);
                if *value {
                    buf.extend_bytes(b"true");
                } else {
                    buf.extend_bytes(b"false");
                }
                Ok(())
            }
            Value::Number(number) => {
                let Some(key) = number_cache_key(number) else {
                    buf.push_byte(delimiter_byte);
                    append_json_number_bytes(buf, number);
                    return Ok(());
                };
                if let Some(last) = self.tabular_last_values.get(column) {
                    if last.number_key == Some(key) {
                        buf.extend_bytes(&last.number_bytes);
                        return Ok(());
                    }
                }
                let start = buf.len();
                if let Some(encoded) = self
                    .tabular_prefixed_number_cache
                    .get(&delimiter)
                    .and_then(|cache| cache.get(&key))
                {
                    buf.extend_bytes(encoded);
                } else if let Some(encoded) = self.tabular_number_cache.get(&key) {
                    buf.push_byte(delimiter_byte);
                    buf.extend_bytes(encoded);
                    let cache = self
                        .tabular_prefixed_number_cache
                        .entry(delimiter)
                        .or_insert_with(|| {
                            HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS)
                        });
                    if cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                        cache
                            .entry(key)
                            .or_insert_with(|| buf.as_slice()[start..].to_vec());
                    }
                } else {
                    buf.push_byte(delimiter_byte);
                    append_json_number_bytes(buf, number);
                    let len = buf.len() - start - 1;
                    let cache = self
                        .tabular_prefixed_number_cache
                        .entry(delimiter)
                        .or_insert_with(|| {
                            HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS)
                        });
                    if len <= NUMBER_CACHE_MAX_LEN && cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS
                    {
                        cache.insert(key, buf.as_slice()[start..].to_vec());
                    }
                }
                self.update_tabular_last_number(column, key, &buf.as_slice()[start..]);
                Ok(())
            }
            Value::String(value) => {
                if let Some(last) = self.tabular_last_values.get(column) {
                    if last.string_key.as_deref() == Some(value.as_str()) {
                        buf.extend_bytes(&last.string_bytes);
                        return Ok(());
                    }
                }
                let start = buf.len();
                if value.len() <= STRING_CACHE_MAX_LEN {
                    if let Some(encoded) = self
                        .tabular_prefixed_string_cache
                        .get(&delimiter)
                        .and_then(|cache| cache.get(value.as_str()))
                    {
                        buf.extend_bytes(encoded);
                        self.update_tabular_last_string(column, value, &buf.as_slice()[start..]);
                        return Ok(());
                    }
                    if let Some(encoded) = self
                        .tabular_string_cache
                        .get(&delimiter)
                        .and_then(|cache| cache.get(value.as_str()))
                    {
                        buf.push_byte(delimiter_byte);
                        buf.extend_bytes(encoded);
                        let cache = self
                            .tabular_prefixed_string_cache
                            .entry(delimiter)
                            .or_insert_with(|| {
                                HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS)
                            });
                        if cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                            cache
                                .entry(SmolStr::new(value))
                                .or_insert_with(|| buf.as_slice()[start..].to_vec());
                        }
                        self.update_tabular_last_string(column, value, &buf.as_slice()[start..]);
                        return Ok(());
                    }
                    buf.push_byte(delimiter_byte);
                    self.append_string(buf, value, delimiter);
                    let cache = self
                        .tabular_prefixed_string_cache
                        .entry(delimiter)
                        .or_insert_with(|| {
                            HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS)
                        });
                    if cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                        cache.insert(SmolStr::new(value), buf.as_slice()[start..].to_vec());
                    }
                    self.update_tabular_last_string(column, value, &buf.as_slice()[start..]);
                    return Ok(());
                }
                buf.push_byte(delimiter_byte);
                self.append_string(buf, value, delimiter);
                self.update_tabular_last_string(column, value, &buf.as_slice()[start..]);
                Ok(())
            }
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn reset_tabular_last_values(&mut self, columns: usize) {
        if self.tabular_last_values.len() < columns {
            self.tabular_last_values
                .resize_with(columns, TabularLastCache::default);
        } else {
            self.tabular_last_values.truncate(columns);
        }
        for cache in &mut self.tabular_last_values {
            cache.clear();
        }
    }

    fn update_tabular_last_string(&mut self, column: usize, value: &str, bytes: &[u8]) {
        if let Some(cache) = self.tabular_last_values.get_mut(column) {
            cache.string_key = Some(SmolStr::new(value));
            cache.string_bytes.clear();
            cache.string_bytes.extend_from_slice(bytes);
        }
    }

    fn update_tabular_last_number(&mut self, column: usize, key: NumberKey, bytes: &[u8]) {
        if let Some(cache) = self.tabular_last_values.get_mut(column) {
            cache.number_key = Some(key);
            cache.number_bytes.clear();
            cache.number_bytes.extend_from_slice(bytes);
        }
    }

    fn append_string<B: ByteSink>(&mut self, buf: &mut B, value: &str, delimiter: char) {
        if is_canonical_unquoted_key(value) && !matches!(value, "true" | "false" | "null") {
            buf.extend_bytes(value.as_bytes());
            return;
        }
        let (needs_quote, needs_escape) = self.analyze_string_cached(value, delimiter);
        if !needs_quote {
            buf.extend_bytes(value.as_bytes());
            return;
        }
        buf.push_byte(b'"');
        if needs_escape {
            escape_string_into_bytes(buf, value);
        } else {
            buf.extend_bytes(value.as_bytes());
        }
        buf.push_byte(b'"');
    }

    fn analyze_string_cached(&mut self, value: &str, delimiter: char) -> (bool, bool) {
        if value.len() > STRING_CACHE_MAX_LEN {
            return analyze_string(value, delimiter);
        }
        let cache = self
            .string_cache
            .entry(delimiter)
            .or_insert_with(|| HashMap::with_capacity(STRING_CACHE_MAX_ITEMS));
        if let Some(flags) = cache.get(value) {
            return *flags;
        }
        let flags = analyze_string(value, delimiter);
        if cache.len() < STRING_CACHE_MAX_ITEMS {
            cache.insert(SmolStr::new(value), flags);
        }
        flags
    }

    fn append_encoded_key(&mut self, buf: &mut Vec<u8>, key: &str) {
        if is_canonical_unquoted_key(key) {
            buf.extend_from_slice(key.as_bytes());
            return;
        }
        if let Some(encoded) = self.key_cache.get(key) {
            buf.extend_from_slice(encoded.as_bytes());
            return;
        }
        let mut encoded = String::with_capacity(key.len() + 2);
        encoded.push('"');
        escape_string_into(&mut encoded, key);
        encoded.push('"');
        if key.len() <= KEY_CACHE_MAX_LEN && self.key_cache.len() < KEY_CACHE_MAX_ITEMS {
            let entry = self.key_cache.entry(SmolStr::new(key)).or_insert(encoded);
            buf.extend_from_slice(entry.as_bytes());
            return;
        }
        buf.extend_from_slice(encoded.as_bytes());
    }

    fn append_encoded_key_no_cache(&self, buf: &mut Vec<u8>, key: &str) {
        if is_canonical_unquoted_key(key) {
            buf.extend_from_slice(key.as_bytes());
            return;
        }
        let mut encoded = String::with_capacity(key.len() + 2);
        encoded.push('"');
        escape_string_into(&mut encoded, key);
        encoded.push('"');
        buf.extend_from_slice(encoded.as_bytes());
    }

    fn intern_key_id(&mut self, key: &str) -> usize {
        if let Some(&id) = self.key_intern.get(key) {
            return id;
        }
        let id = self.interned_keys.len();
        let owned = key.to_string();
        self.interned_keys.push(owned.clone());
        self.key_intern.insert(owned, id);
        id
    }

    fn interned_key(&self, id: usize) -> &str {
        self.interned_keys
            .get(id)
            .expect("interned key id missing")
            .as_str()
    }

    fn append_array_header(
        &mut self,
        buf: &mut Vec<u8>,
        len: usize,
        key: Option<&str>,
        fields: Option<&[usize]>,
    ) {
        let delimiter = self.active_delimiter();
        if let Some(key) = key {
            self.append_encoded_key(buf, key);
        }
        buf.push(b'[');
        let mut num = itoa::Buffer::new();
        buf.extend_from_slice(num.format(len).as_bytes());
        if delimiter != ',' {
            buf.push(delimiter as u8);
        }
        buf.push(b']');
        if let Some(fields) = fields {
            buf.push(b'{');
            for (idx, field_id) in fields.iter().enumerate() {
                if idx > 0 {
                    buf.push(delimiter as u8);
                }
                let field = self.interned_key(*field_id);
                self.append_encoded_key_no_cache(buf, field);
            }
            buf.push(b'}');
        }
    }

    fn append_inline_scalars(&mut self, buf: &mut Vec<u8>, array: &[Value]) -> Result<()> {
        for (idx, value) in array.iter().enumerate() {
            if idx > 0 {
                buf.push(self.active_delimiter() as u8);
            }
            self.append_scalar_active(buf, value)?;
        }
        Ok(())
    }

    fn with_line_buf<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut Vec<u8>) -> R,
    {
        let mut buf = std::mem::take(&mut self.line_buf);
        let result = f(self, &mut buf);
        self.line_buf = buf;
        result
    }

    fn with_out_buf<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut Vec<u8>) -> R,
    {
        let mut buf = std::mem::take(&mut self.out);
        let result = f(self, &mut buf);
        self.out = buf;
        result
    }

    #[cfg(feature = "parallel")]
    fn should_parallel_tabular(&self, rows: usize, fields: usize) -> bool {
        rows >= PARALLEL_TABULAR_MIN_ROWS
            && rows.saturating_mul(fields) >= PARALLEL_TABULAR_MIN_CELLS
    }

    fn tabular_fields(&mut self, array: &[Value]) -> Option<Vec<usize>> {
        let first = array.first()?.as_object()?;
        if first.is_empty() {
            return None;
        }
        let fields: Vec<usize> = first.keys().map(|key| self.intern_key_id(key)).collect();
        for item in array {
            let row = item.as_object()?;
            if row.len() != fields.len() {
                return None;
            }
            for field in &fields {
                let key = self.interned_key(*field);
                let value = row.get(key)?;
                if !is_scalar(value) {
                    return None;
                }
            }
        }
        Some(fields)
    }

    fn write_line_bytes(&mut self, indent_level: usize, content: &[u8]) {
        self.write_line_with_prefix_bytes(indent_level, b"", content);
    }

    fn begin_line_with_prefix(&mut self, indent_level: usize, prefix: &[u8]) {
        if !self.out.is_empty() {
            self.out.push(b'\n');
        }
        if indent_level > 0 && !self.indent_unit.is_empty() {
            self.ensure_indent_cache(indent_level);
            let indent = &self.indent_cache[indent_level];
            Self::append_bytes(&mut self.out, indent);
        }
        Self::append_bytes(&mut self.out, prefix);
    }

    fn begin_line_with_prefix_into(
        &mut self,
        out: &mut Vec<u8>,
        indent_level: usize,
        prefix: &[u8],
    ) {
        if !out.is_empty() {
            out.push(b'\n');
        }
        if indent_level > 0 && !self.indent_unit.is_empty() {
            self.ensure_indent_cache(indent_level);
            let indent = &self.indent_cache[indent_level];
            Self::append_bytes(out, indent);
        }
        Self::append_bytes(out, prefix);
    }

    fn write_line_with_prefix_bytes(&mut self, indent_level: usize, prefix: &[u8], content: &[u8]) {
        self.begin_line_with_prefix(indent_level, prefix);
        Self::append_bytes(&mut self.out, content);
    }

    fn ensure_indent_cache(&mut self, level: usize) {
        if self.indent_unit.is_empty() || level == 0 {
            return;
        }
        while self.indent_cache.len() <= level {
            let mut next = self.indent_cache.last().unwrap().clone();
            next.extend_from_slice(&self.indent_unit);
            self.indent_cache.push(next);
        }
    }

    fn reserve_tabular_rows(&mut self, rows: usize, fields: usize) {
        if rows == 0 || fields == 0 {
            return;
        }
        let avg_cell = 6usize;
        let row_len = fields.saturating_mul(avg_cell) + fields.saturating_sub(1);
        let estimate = rows.saturating_mul(row_len + 1);
        self.out.reserve(estimate);
    }

    fn reserve_object_entries(&mut self, entries: usize) {
        if entries == 0 {
            return;
        }
        let avg_entry = 12usize;
        let estimate = entries.saturating_mul(avg_entry + 1);
        self.out.reserve(estimate);
    }

    fn reserve_inline_array(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let avg_cell = 6usize;
        let estimate = count.saturating_mul(avg_cell + 1);
        self.out.reserve(estimate);
    }

    fn append_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
        match bytes.len() {
            0 => {}
            1 => out.push(bytes[0]),
            2 => {
                out.push(bytes[0]);
                out.push(bytes[1]);
            }
            3 => {
                out.push(bytes[0]);
                out.push(bytes[1]);
                out.push(bytes[2]);
            }
            4 => {
                out.push(bytes[0]);
                out.push(bytes[1]);
                out.push(bytes[2]);
                out.push(bytes[3]);
            }
            _ => out.extend_from_slice(bytes),
        }
    }
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

#[cfg(feature = "parallel")]
struct RowEncoder {
    delimiter: char,
    string_cache: HashMap<SmolStr, (bool, bool)>,
    string_encoded_cache: HashMap<SmolStr, Vec<u8>>,
    string_prefixed_cache: HashMap<SmolStr, Vec<u8>>,
    number_encoded_cache: HashMap<NumberKey, Vec<u8>>,
    number_prefixed_cache: HashMap<NumberKey, Vec<u8>>,
}

#[cfg(feature = "parallel")]
impl RowEncoder {
    fn new(delimiter: char) -> Self {
        Self {
            delimiter,
            string_cache: HashMap::with_capacity(STRING_CACHE_MAX_ITEMS),
            string_encoded_cache: HashMap::with_capacity(TABULAR_STRING_CACHE_MAX_ITEMS),
            string_prefixed_cache: HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS),
            number_encoded_cache: HashMap::with_capacity(TABULAR_NUMBER_CACHE_MAX_ITEMS),
            number_prefixed_cache: HashMap::with_capacity(TABULAR_PREFIXED_CACHE_MAX_ITEMS),
        }
    }

    fn encode_tabular_row(&mut self, item: &Value, fields: &[SmolStr]) -> Result<RowBuf> {
        let obj = item
            .as_object()
            .ok_or_else(|| Error::encode("tabular row is not an object"))?;
        let mut row = RowBuf::new();
        let row_capacity = fields.len() * 4;
        row.reserve(row_capacity);
        let mut iter = fields.iter();
        let Some(first_field) = iter.next() else {
            return Ok(row);
        };
        let value = obj
            .get(first_field.as_str())
            .ok_or_else(|| Error::encode("tabular row missing field"))?;
        self.append_scalar(&mut row, value)?;
        for field in iter {
            let value = obj
                .get(field.as_str())
                .ok_or_else(|| Error::encode("tabular row missing field"))?;
            self.append_scalar_prefixed(&mut row, value)?;
        }
        Ok(row)
    }

    fn append_scalar(&mut self, buf: &mut RowBuf, value: &Value) -> Result<()> {
        match value {
            Value::Null => {
                buf.extend_from_slice(b"null");
                Ok(())
            }
            Value::Bool(value) => {
                if *value {
                    buf.extend_from_slice(b"true");
                } else {
                    buf.extend_from_slice(b"false");
                }
                Ok(())
            }
            Value::Number(number) => self.append_number(buf, number),
            Value::String(value) => {
                if value.len() <= STRING_CACHE_MAX_LEN {
                    if let Some(encoded) = self.string_encoded_cache.get(value.as_str()) {
                        buf.extend_from_slice(encoded);
                        return Ok(());
                    }
                    let start = buf.len();
                    self.append_string(buf, value);
                    if self.string_encoded_cache.len() < TABULAR_STRING_CACHE_MAX_ITEMS {
                        self.string_encoded_cache
                            .insert(SmolStr::new(value), buf[start..].to_vec());
                    }
                    return Ok(());
                }
                self.append_string(buf, value);
                Ok(())
            }
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn append_scalar_prefixed(&mut self, buf: &mut RowBuf, value: &Value) -> Result<()> {
        let delimiter_byte = self.delimiter as u8;
        match value {
            Value::Null => {
                buf.push(delimiter_byte);
                buf.extend_from_slice(b"null");
                Ok(())
            }
            Value::Bool(value) => {
                buf.push(delimiter_byte);
                if *value {
                    buf.extend_from_slice(b"true");
                } else {
                    buf.extend_from_slice(b"false");
                }
                Ok(())
            }
            Value::Number(number) => self.append_number_prefixed(buf, number),
            Value::String(value) => {
                if value.len() <= STRING_CACHE_MAX_LEN {
                    if let Some(encoded) = self.string_prefixed_cache.get(value.as_str()) {
                        buf.extend_from_slice(encoded);
                        return Ok(());
                    }
                    if let Some(encoded) = self.string_encoded_cache.get(value.as_str()) {
                        let mut prefixed = Vec::with_capacity(encoded.len() + 1);
                        prefixed.push(delimiter_byte);
                        prefixed.extend_from_slice(encoded);
                        if self.string_prefixed_cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                            let entry = self
                                .string_prefixed_cache
                                .entry(SmolStr::new(value))
                                .or_insert_with(|| prefixed);
                            buf.extend_from_slice(entry);
                        } else {
                            buf.extend_from_slice(&prefixed);
                        }
                        return Ok(());
                    }
                    let start = buf.len();
                    buf.push(delimiter_byte);
                    self.append_string(buf, value);
                    if self.string_prefixed_cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                        self.string_prefixed_cache
                            .insert(SmolStr::new(value), buf[start..].to_vec());
                    }
                    return Ok(());
                }
                buf.push(delimiter_byte);
                self.append_string(buf, value);
                Ok(())
            }
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn append_number(&mut self, buf: &mut RowBuf, number: &serde_json::Number) -> Result<()> {
        if let Some(key) = number_cache_key(number) {
            if let Some(encoded) = self.number_encoded_cache.get(&key) {
                buf.extend_from_slice(encoded);
                return Ok(());
            }
            let start = buf.len();
            append_json_number_bytes(buf, number);
            let len = buf.len() - start;
            if len <= NUMBER_CACHE_MAX_LEN
                && self.number_encoded_cache.len() < TABULAR_NUMBER_CACHE_MAX_ITEMS
            {
                self.number_encoded_cache.insert(key, buf[start..].to_vec());
            }
            return Ok(());
        }
        append_json_number_bytes(buf, number);
        Ok(())
    }

    fn append_number_prefixed(
        &mut self,
        buf: &mut RowBuf,
        number: &serde_json::Number,
    ) -> Result<()> {
        let delimiter_byte = self.delimiter as u8;
        if let Some(key) = number_cache_key(number) {
            if let Some(encoded) = self.number_prefixed_cache.get(&key) {
                buf.extend_from_slice(encoded);
                return Ok(());
            }
            if let Some(encoded) = self.number_encoded_cache.get(&key) {
                let mut prefixed = Vec::with_capacity(encoded.len() + 1);
                prefixed.push(delimiter_byte);
                prefixed.extend_from_slice(encoded);
                if self.number_prefixed_cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS {
                    let entry = self
                        .number_prefixed_cache
                        .entry(key)
                        .or_insert_with(|| prefixed);
                    buf.extend_from_slice(entry);
                } else {
                    buf.extend_from_slice(&prefixed);
                }
                return Ok(());
            }
            let start = buf.len();
            buf.push(delimiter_byte);
            append_json_number_bytes(buf, number);
            let len = buf.len() - start - 1;
            if len <= NUMBER_CACHE_MAX_LEN
                && self.number_prefixed_cache.len() < TABULAR_PREFIXED_CACHE_MAX_ITEMS
            {
                self.number_prefixed_cache
                    .insert(key, buf[start..].to_vec());
            }
            return Ok(());
        }
        buf.push(delimiter_byte);
        append_json_number_bytes(buf, number);
        Ok(())
    }

    fn append_string(&mut self, buf: &mut RowBuf, value: &str) {
        if is_canonical_unquoted_key(value) && !matches!(value, "true" | "false" | "null") {
            buf.extend_from_slice(value.as_bytes());
            return;
        }
        let (needs_quote, needs_escape) = self.analyze_string_cached(value);
        if !needs_quote {
            buf.extend_from_slice(value.as_bytes());
            return;
        }
        buf.push(b'"');
        if needs_escape {
            escape_string_into_bytes(buf, value);
        } else {
            buf.extend_from_slice(value.as_bytes());
        }
        buf.push(b'"');
    }

    fn analyze_string_cached(&mut self, value: &str) -> (bool, bool) {
        if value.len() > STRING_CACHE_MAX_LEN {
            return analyze_string(value, self.delimiter);
        }
        if let Some(flags) = self.string_cache.get(value) {
            return *flags;
        }
        let flags = analyze_string(value, self.delimiter);
        if self.string_cache.len() < STRING_CACHE_MAX_ITEMS {
            self.string_cache.insert(SmolStr::new(value), flags);
        }
        flags
    }
}
