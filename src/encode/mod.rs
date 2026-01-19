use std::collections::{HashMap, HashSet};
use std::io::Write;

use serde::Serialize;
use serde_json::Value;

use crate::num::number::format_json_number;
use crate::text::string::{
    analyze_string, escape_string_into, is_canonical_unquoted_key, is_identifier_segment,
};
use crate::{EncodeOptions, Error, Indent, Result};

const STRING_CACHE_MAX_LEN: usize = 64;
const STRING_CACHE_MAX_ITEMS: usize = 1024;
const KEY_CACHE_MAX_LEN: usize = 64;
const KEY_CACHE_MAX_ITEMS: usize = 1024;
const LARGE_CONTAINER_THRESHOLD: usize = 64;
const MAX_RESERVE_DEPTH: usize = 2;
const RESERVE_SAMPLE_ITEMS: usize = 4;

pub fn to_string<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<String> {
    let value = serde_json::to_value(value)
        .map_err(|err| Error::serialize(format!("serialize failed: {err}")))?;
    let encoder = encode_value(&value, options)?;
    encoder.finish_string()
}

pub fn to_vec<T: Serialize>(value: &T, options: &EncodeOptions) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)
        .map_err(|err| Error::serialize(format!("serialize failed: {err}")))?;
    let encoder = encode_value(&value, options)?;
    Ok(encoder.finish_bytes())
}

pub fn to_writer<T: Serialize, W: Write>(
    mut writer: W,
    value: &T,
    options: &EncodeOptions,
) -> Result<()> {
    let bytes = to_vec(value, options)?;
    writer
        .write_all(&bytes)
        .map_err(|err| Error::encode(format!("write failed: {err}")))?;
    Ok(())
}

fn encode_value(value: &Value, options: &EncodeOptions) -> Result<Encoder> {
    let mut encoder = Encoder::new(options);
    encoder.reserve_for_value(value);
    encoder.precompute_string_flags(value);
    encoder.encode_root(value)?;
    Ok(encoder)
}

struct Encoder {
    document_delimiter: char,
    key_folding: bool,
    flatten_depth: usize,
    indent_unit: Vec<u8>,
    indent_cache: Vec<Vec<u8>>,
    delimiter_stack: Vec<char>,
    string_cache: HashMap<char, HashMap<String, (bool, bool)>>,
    key_cache: HashMap<String, String>,
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
            string_cache: HashMap::new(),
            key_cache: HashMap::new(),
            key_intern: HashMap::new(),
            interned_keys: Vec::new(),
            line_buf: Vec::with_capacity(128),
            out: Vec::with_capacity(128),
        }
    }

    fn finish_string(self) -> Result<String> {
        String::from_utf8(self.out)
            .map_err(|err| Error::encode(format!("utf-8 conversion failed: {err}")))
    }

    fn finish_bytes(self) -> Vec<u8> {
        self.out
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
            Value::String(value) => {
                if value.len() <= STRING_CACHE_MAX_LEN {
                    let _ = self.analyze_string_cached(value, self.document_delimiter);
                }
            }
            Value::Array(array) => {
                for item in array {
                    self.precompute_string_flags_inner(item);
                }
            }
            Value::Object(map) => {
                for value in map.values() {
                    self.precompute_string_flags_inner(value);
                }
            }
            _ => {}
        }
    }

    fn encode_root(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::Object(map) => self.encode_object(map, 0),
            Value::Array(array) => self.encode_array_value(array, 0, None, b""),
            _ => {
                let encoded = self.encode_scalar_document(value)?;
                self.write_line_bytes(0, encoded.as_bytes());
                Ok(())
            }
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
            _ => {
                let encoded = self.encode_scalar_document(value)?;
                self.with_line_buf(|encoder, line| {
                    line.clear();
                    encoder.append_encoded_key(line, key);
                    line.extend_from_slice(b": ");
                    line.extend_from_slice(encoded.as_bytes());
                    encoder.write_line_bytes(indent_level, line);
                });
                Ok(())
            }
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
            for item in array {
                let obj = item
                    .as_object()
                    .ok_or_else(|| Error::encode("tabular row is not an object"))?;
                self.with_line_buf(|encoder, row| -> Result<()> {
                    row.clear();
                    let row_capacity = fields.len() * 4;
                    if row_capacity > row.capacity() {
                        row.reserve(row_capacity - row.capacity());
                    }
                    for (idx, field) in fields.iter().enumerate() {
                        let value = obj
                            .get(encoder.interned_key(*field))
                            .ok_or_else(|| Error::encode("tabular row missing field"))?;
                        if idx > 0 {
                            row.push(encoder.active_delimiter() as u8);
                        }
                        let encoded = encoder.encode_scalar_active(value)?;
                        row.extend_from_slice(encoded.as_bytes());
                    }
                    encoder.write_line_bytes(row_indent, row);
                    Ok(())
                })?;
            }
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
            _ => {
                let encoded = self.encode_scalar_document(value)?;
                self.write_line_with_prefix_bytes(indent_level, b"- ", encoded.as_bytes());
                Ok(())
            }
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
                let encoded = self.encode_scalar_document(first_value)?;
                self.with_line_buf(|encoder, line| {
                    line.clear();
                    encoder.append_encoded_key(line, first_key);
                    line.extend_from_slice(b": ");
                    line.extend_from_slice(encoded.as_bytes());
                    encoder.write_line_with_prefix_bytes(indent_level, b"- ", line);
                });
            }
        }

        for (key, value) in iter {
            self.encode_object_entry(key, value, indent_level + 1)?;
        }
        Ok(())
    }

    fn encode_scalar_with_delimiter(&mut self, value: &Value, delimiter: char) -> Result<String> {
        match value {
            Value::Null => Ok("null".to_string()),
            Value::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
            Value::Number(number) => Ok(format_json_number(number)),
            Value::String(value) => Ok(self.encode_string(value, delimiter)),
            _ => Err(Error::encode("non-scalar value in scalar position")),
        }
    }

    fn encode_scalar_document(&mut self, value: &Value) -> Result<String> {
        self.encode_scalar_with_delimiter(value, self.document_delimiter)
    }

    fn encode_scalar_active(&mut self, value: &Value) -> Result<String> {
        let delimiter = self.active_delimiter();
        self.encode_scalar_with_delimiter(value, delimiter)
    }

    fn encode_string(&mut self, value: &str, delimiter: char) -> String {
        let (needs_quote, needs_escape) = self.analyze_string_cached(value, delimiter);
        if !needs_quote {
            return value.to_string();
        }
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        if needs_escape {
            escape_string_into(&mut out, value);
        } else {
            out.push_str(value);
        }
        out.push('"');
        out
    }

    fn analyze_string_cached(&mut self, value: &str, delimiter: char) -> (bool, bool) {
        if value.len() > STRING_CACHE_MAX_LEN {
            return analyze_string(value, delimiter);
        }
        let cache = self
            .string_cache
            .entry(delimiter)
            .or_default();
        if let Some(flags) = cache.get(value) {
            return *flags;
        }
        let flags = analyze_string(value, delimiter);
        if cache.len() >= STRING_CACHE_MAX_ITEMS {
            cache.clear();
        }
        cache.insert(value.to_string(), flags);
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
        if key.len() <= KEY_CACHE_MAX_LEN {
            if self.key_cache.len() >= KEY_CACHE_MAX_ITEMS {
                self.key_cache.clear();
            }
            let entry = self.key_cache.entry(key.to_string()).or_insert(encoded);
            buf.extend_from_slice(entry.as_bytes());
        } else {
            buf.extend_from_slice(encoded.as_bytes());
        }
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
            let encoded = self.encode_scalar_active(value)?;
            buf.extend_from_slice(encoded.as_bytes());
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

    fn write_line_with_prefix_bytes(&mut self, indent_level: usize, prefix: &[u8], content: &[u8]) {
        if !self.out.is_empty() {
            self.out.push(b'\n');
        }
        if indent_level > 0 && !self.indent_unit.is_empty() {
            self.ensure_indent_cache(indent_level);
            let indent = self.indent_cache[indent_level].clone();
            Self::append_bytes(&mut self.out, &indent);
        }
        Self::append_bytes(&mut self.out, prefix);
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
