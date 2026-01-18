//! Canonical-only encode entry point.

use super::{
    profile::CanonicalProfile,
    validate::{analyze_string, is_canonical_number, is_canonical_unquoted_key},
};
use crate::parallel::encode::map_items_parallel;
use crate::{ToonError, ToonResult};

use serde::ser::{self, Serialize};
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

const PARALLEL_MIN_FIELDS: usize = 64;
const PARALLEL_MIN_ITEMS: usize = 256;
const PARALLEL_MIN_BYTES: usize = 16 * 1024;
const ESTIMATE_MIN_FIELDS: usize = 128;
const ESTIMATE_MIN_ITEMS: usize = 512;
const ESTIMATE_MIN_GUESS: usize = 8 * 1024;
const DEFAULT_CONTAINER_CAPACITY: usize = 1024;
const TABULAR_MIN_ROWS: usize = 2;
const SMALL_CAPACITY: usize = 64;
const INLINE_FIELDS: usize = 8;
const VALUE_POOL_MAX_ITEMS: usize = 64;
const ENTRY_POOL_MAX_ITEMS: usize = 64;
const VALUE_POOL_MAX_CAPACITY: usize = 1 << 16;
const ENTRY_POOL_MAX_CAPACITY: usize = 1 << 14;
const KEY_INTERN_MAX_ITEMS: usize = 4096;
const ANALYZE_CACHE_MAX_ITEMS: usize = 4096;
const ANALYZE_CACHE_MAX_LEN: usize = 128;
const INDENT_CHUNK: &str = "                                                                ";

type CanonicalFields = SmallVec<[CanonicalField; INLINE_FIELDS]>;

thread_local! {
    static VALUE_VEC_POOL: RefCell<Vec<Vec<CanonicalValue>>> = RefCell::new(Vec::new());
    static ENTRY_VEC_POOL: RefCell<Vec<Vec<(SmolStr, CanonicalValue)>>> = RefCell::new(Vec::new());
    static KEY_INTERNER: RefCell<HashMap<String, SmolStr>> = RefCell::new(HashMap::new());
    static ANALYZE_CACHE_COMMA: RefCell<HashMap<SmolStr, (bool, bool)>> = RefCell::new(HashMap::new());
    static ANALYZE_CACHE_TAB: RefCell<HashMap<SmolStr, (bool, bool)>> = RefCell::new(HashMap::new());
    static ANALYZE_CACHE_PIPE: RefCell<HashMap<SmolStr, (bool, bool)>> = RefCell::new(HashMap::new());
}

fn take_value_vec(capacity: usize) -> Vec<CanonicalValue> {
    VALUE_VEC_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if let Some(mut vec) = pool.pop() {
            if vec.capacity() < capacity {
                vec.reserve(capacity - vec.capacity());
            }
            vec
        } else {
            Vec::with_capacity(capacity)
        }
    })
}

fn recycle_value_vec(mut vec: Vec<CanonicalValue>) {
    vec.clear();
    if vec.capacity() == 0 || vec.capacity() > VALUE_POOL_MAX_CAPACITY {
        return;
    }
    VALUE_VEC_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < VALUE_POOL_MAX_ITEMS {
            pool.push(vec);
        }
    });
}

fn take_entry_vec(capacity: usize) -> Vec<(SmolStr, CanonicalValue)> {
    ENTRY_VEC_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if let Some(mut vec) = pool.pop() {
            if vec.capacity() < capacity {
                vec.reserve(capacity - vec.capacity());
            }
            vec
        } else {
            Vec::with_capacity(capacity)
        }
    })
}

fn recycle_entry_vec(mut vec: Vec<(SmolStr, CanonicalValue)>) {
    vec.clear();
    if vec.capacity() == 0 || vec.capacity() > ENTRY_POOL_MAX_CAPACITY {
        return;
    }
    ENTRY_VEC_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < ENTRY_POOL_MAX_ITEMS {
            pool.push(vec);
        }
    });
}

fn intern_key(value: &str) -> SmolStr {
    KEY_INTERNER.with(|interner| {
        let mut interner = interner.borrow_mut();
        if let Some(existing) = interner.get(value) {
            return existing.clone();
        }
        if interner.len() >= KEY_INTERN_MAX_ITEMS {
            return SmolStr::new(value);
        }
        let smol = SmolStr::new(value);
        interner.insert(value.to_string(), smol.clone());
        smol
    })
}

fn analyze_string_cached(value: &str, delimiter: char) -> (bool, bool) {
    if value.len() > ANALYZE_CACHE_MAX_LEN {
        return analyze_string(value, delimiter);
    }
    let cache = match delimiter {
        ',' => &ANALYZE_CACHE_COMMA,
        '\t' => &ANALYZE_CACHE_TAB,
        '|' => &ANALYZE_CACHE_PIPE,
        _ => return analyze_string(value, delimiter),
    };
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(flags) = cache.get(value) {
            return *flags;
        }
        let flags = analyze_string(value, delimiter);
        if cache.len() < ANALYZE_CACHE_MAX_ITEMS {
            cache.insert(SmolStr::new(value), flags);
        }
        flags
    })
}

#[derive(Debug, Clone)]
enum CanonicalValue {
    Null,
    Bool(bool),
    Number(CanonicalNumber),
    String(CanonicalString),
    Array(CanonicalArray),
    Object(Vec<(SmolStr, CanonicalValue)>),
}

#[derive(Debug, Clone)]
struct CanonicalString {
    value: SmolStr,
    needs_quote: bool,
    needs_escape: bool,
}

impl CanonicalString {
    fn new(value: &str, delimiter: char) -> Self {
        let (needs_quote, needs_escape) = analyze_string_cached(value, delimiter);
        Self {
            value: SmolStr::new(value),
            needs_quote,
            needs_escape,
        }
    }

    fn from_smol(value: SmolStr, delimiter: char) -> Self {
        let (needs_quote, needs_escape) = analyze_string_cached(value.as_str(), delimiter);
        Self {
            value,
            needs_quote,
            needs_escape,
        }
    }

    fn from_static(value: &'static str, delimiter: char) -> Self {
        let (needs_quote, needs_escape) = analyze_string_cached(value, delimiter);
        Self {
            value: SmolStr::new_static(value),
            needs_quote,
            needs_escape,
        }
    }

    fn as_str(&self) -> &str {
        self.value.as_str()
    }
}

#[derive(Debug, Clone)]
struct CanonicalField {
    name: SmolStr,
    header_needs_quote: bool,
    header_needs_escape: bool,
}

impl CanonicalField {
    fn new(name: SmolStr, delimiter: char) -> Self {
        let (needs_quote, needs_escape) = analyze_string_cached(name.as_str(), delimiter);
        let header_needs_quote = needs_quote || !is_canonical_unquoted_key(name.as_str());
        Self {
            name,
            header_needs_quote,
            header_needs_escape: needs_escape,
        }
    }

    fn as_str(&self) -> &str {
        self.name.as_str()
    }
}

#[derive(Debug, Clone)]
enum CanonicalNumber {
    I64(i64),
    U64(u64),
    Text(SmolStr),
}

impl CanonicalNumber {
    fn len(&self) -> usize {
        match self {
            CanonicalNumber::I64(value) => {
                let mut buffer = itoa::Buffer::new();
                buffer.format(*value).len()
            }
            CanonicalNumber::U64(value) => {
                let mut buffer = itoa::Buffer::new();
                buffer.format(*value).len()
            }
            CanonicalNumber::Text(value) => value.len(),
        }
    }
}

#[derive(Debug, Clone)]
enum CanonicalArray {
    Inline(Vec<CanonicalValue>),
    Tabular {
        fields: CanonicalFields,
        rows: Vec<CanonicalValue>,
    },
    List(Vec<CanonicalValue>),
}

fn recycle_array(array: &mut CanonicalArray) {
    match array {
        CanonicalArray::Inline(values) | CanonicalArray::List(values) => {
            let values = std::mem::take(values);
            recycle_value_vec(values);
        }
        CanonicalArray::Tabular { rows, .. } => {
            let rows = std::mem::take(rows);
            recycle_value_vec(rows);
        }
    }
}

impl Drop for CanonicalValue {
    fn drop(&mut self) {
        match self {
            CanonicalValue::Array(array) => recycle_array(array),
            CanonicalValue::Object(entries) => {
                let entries = std::mem::take(entries);
                recycle_entry_vec(entries);
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
struct CanonicalSerError {
    message: String,
}

impl CanonicalSerError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        CanonicalSerError {
            message: msg.to_string(),
        }
    }
}

impl fmt::Display for CanonicalSerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CanonicalSerError {}

impl ser::Error for CanonicalSerError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        CanonicalSerError {
            message: msg.to_string(),
        }
    }
}

impl From<CanonicalSerError> for ToonError {
    fn from(err: CanonicalSerError) -> Self {
        ToonError::SerializationError(err.message)
    }
}

#[derive(Debug, Clone, Copy)]
struct CanonicalSerializer {
    delimiter: char,
}

impl CanonicalSerializer {
    fn new(profile: CanonicalProfile) -> Self {
        Self {
            delimiter: profile.delimiter.as_char(),
        }
    }
}

impl ser::Serializer for CanonicalSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    type SerializeSeq = CanonicalSeqSerializer;
    type SerializeTuple = CanonicalSeqSerializer;
    type SerializeTupleStruct = CanonicalSeqSerializer;
    type SerializeTupleVariant = CanonicalTupleVariantSerializer;
    type SerializeMap = CanonicalMapSerializer;
    type SerializeStruct = CanonicalStructSerializer;
    type SerializeStructVariant = CanonicalStructVariantSerializer;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Bool(value))
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Number(CanonicalNumber::I64(value)))
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(CanonicalValue::Number(CanonicalNumber::Text(SmolStr::new(
            buffer.format(value),
        ))))
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Number(CanonicalNumber::U64(value)))
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(CanonicalValue::Number(CanonicalNumber::Text(SmolStr::new(
            buffer.format(value),
        ))))
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        serialize_float(value as f64)
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        serialize_float(value)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        let smol = smol_from_char(value);
        Ok(CanonicalValue::String(CanonicalString::from_smol(
            smol,
            self.delimiter,
        )))
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::String(CanonicalString::new(
            value,
            self.delimiter,
        )))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        let mut out = take_value_vec(value.len());
        for b in value {
            out.push(CanonicalValue::Number(CanonicalNumber::U64(u64::from(*b))));
        }
        Ok(CanonicalValue::Array(canonicalize_array(
            out,
            self.delimiter,
        )))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(CanonicalValue::String(CanonicalString::from_static(
            variant,
            self.delimiter,
        )))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        Ok(CanonicalValue::Object(vec![(
            SmolStr::new_static(variant),
            value,
        )]))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(CanonicalSeqSerializer {
            items: take_value_vec(len.unwrap_or(0)),
            delimiter: self.delimiter,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(CanonicalTupleVariantSerializer {
            name: SmolStr::new_static(variant),
            items: take_value_vec(len),
            delimiter: self.delimiter,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(CanonicalMapSerializer {
            entries: take_entry_vec(len.unwrap_or(0)),
            next_key: None,
            is_sorted: true,
            last_key_index: None,
            delimiter: self.delimiter,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(CanonicalStructSerializer {
            entries: take_entry_vec(len),
            is_sorted: true,
            last_key_index: None,
            delimiter: self.delimiter,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(CanonicalStructVariantSerializer {
            name: SmolStr::new_static(variant),
            entries: take_entry_vec(len),
            is_sorted: true,
            last_key_index: None,
            delimiter: self.delimiter,
        })
    }
}

struct CanonicalSeqSerializer {
    items: Vec<CanonicalValue>,
    delimiter: char,
}

impl Drop for CanonicalSeqSerializer {
    fn drop(&mut self) {
        let items = std::mem::take(&mut self.items);
        recycle_value_vec(items);
    }
}

impl SerializeSeq for CanonicalSeqSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        self.items.push(value);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut this = self;
        let items = std::mem::take(&mut this.items);
        Ok(CanonicalValue::Array(canonicalize_array(
            items,
            this.delimiter,
        )))
    }
}

impl SerializeTuple for CanonicalSeqSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for CanonicalSeqSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

struct CanonicalTupleVariantSerializer {
    name: SmolStr,
    items: Vec<CanonicalValue>,
    delimiter: char,
}

impl Drop for CanonicalTupleVariantSerializer {
    fn drop(&mut self) {
        let items = std::mem::take(&mut self.items);
        recycle_value_vec(items);
    }
}

impl SerializeTupleVariant for CanonicalTupleVariantSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        self.items.push(value);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut this = self;
        let name = std::mem::replace(&mut this.name, SmolStr::new_static(""));
        let items = std::mem::take(&mut this.items);
        Ok(CanonicalValue::Object(vec![(
            name,
            CanonicalValue::Array(canonicalize_array(items, this.delimiter)),
        )]))
    }
}

struct CanonicalMapSerializer {
    entries: Vec<(SmolStr, CanonicalValue)>,
    next_key: Option<SmolStr>,
    is_sorted: bool,
    last_key_index: Option<usize>,
    delimiter: char,
}

impl Drop for CanonicalMapSerializer {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        recycle_entry_vec(entries);
    }
}

impl SerializeMap for CanonicalMapSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        let key = key.serialize(CanonicalKeySerializer)?;
        self.next_key = Some(key);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| CanonicalSerError::custom("map value without key"))?;
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        if self.is_sorted {
            if let Some(last) = self.last_key_index {
                let last_key = &self.entries[last].0;
                if key.as_bytes() == last_key.as_bytes() {
                    return Err(CanonicalSerError::custom("duplicate object key"));
                }
                if key.as_bytes() < last_key.as_bytes() {
                    self.is_sorted = false;
                }
            }
        }
        self.entries.push((key, value));
        self.last_key_index = Some(self.entries.len().saturating_sub(1));
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut this = self;
        let entries = std::mem::take(&mut this.entries);
        finish_object(entries, this.is_sorted)
    }
}

struct CanonicalStructSerializer {
    entries: Vec<(SmolStr, CanonicalValue)>,
    is_sorted: bool,
    last_key_index: Option<usize>,
    delimiter: char,
}

impl Drop for CanonicalStructSerializer {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        recycle_entry_vec(entries);
    }
}

impl SerializeStruct for CanonicalStructSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        let key = SmolStr::new_static(key);
        if self.is_sorted {
            if let Some(last) = self.last_key_index {
                let last_key = &self.entries[last].0;
                if key.as_bytes() == last_key.as_bytes() {
                    return Err(CanonicalSerError::custom("duplicate object key"));
                }
                if key.as_bytes() < last_key.as_bytes() {
                    self.is_sorted = false;
                }
            }
        }
        self.entries.push((key, value));
        self.last_key_index = Some(self.entries.len().saturating_sub(1));
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut this = self;
        let entries = std::mem::take(&mut this.entries);
        finish_object(entries, this.is_sorted)
    }
}

struct CanonicalStructVariantSerializer {
    name: SmolStr,
    entries: Vec<(SmolStr, CanonicalValue)>,
    is_sorted: bool,
    last_key_index: Option<usize>,
    delimiter: char,
}

impl Drop for CanonicalStructVariantSerializer {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        recycle_entry_vec(entries);
    }
}

impl SerializeStructVariant for CanonicalStructVariantSerializer {
    type Ok = CanonicalValue;
    type Error = CanonicalSerError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let value = value.serialize(CanonicalSerializer {
            delimiter: self.delimiter,
        })?;
        let key = SmolStr::new_static(key);
        if self.is_sorted {
            if let Some(last) = self.last_key_index {
                let last_key = &self.entries[last].0;
                if key.as_bytes() == last_key.as_bytes() {
                    return Err(CanonicalSerError::custom("duplicate object key"));
                }
                if key.as_bytes() < last_key.as_bytes() {
                    self.is_sorted = false;
                }
            }
        }
        self.entries.push((key, value));
        self.last_key_index = Some(self.entries.len().saturating_sub(1));
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut this = self;
        let entries = std::mem::take(&mut this.entries);
        let name = std::mem::replace(&mut this.name, SmolStr::new_static(""));
        let value = finish_object(entries, this.is_sorted)?;
        Ok(CanonicalValue::Object(vec![(name, value)]))
    }
}

struct CanonicalKeySerializer;

impl ser::Serializer for CanonicalKeySerializer {
    type Ok = SmolStr;
    type Error = CanonicalSerError;

    type SerializeSeq = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeTuple = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeTupleStruct = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeTupleVariant = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeMap = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeStruct = ser::Impossible<SmolStr, CanonicalSerError>;
    type SerializeStructVariant = ser::Impossible<SmolStr, CanonicalSerError>;

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(intern_key(value))
    }

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(if value {
            intern_key("true")
        } else {
            intern_key("false")
        })
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        let mut buffer = itoa::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        let mut buffer = ryu::Buffer::new();
        Ok(intern_key(buffer.format(f64::from(value))))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        let mut buffer = ryu::Buffer::new();
        Ok(intern_key(buffer.format(value)))
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(smol_from_char(value))
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(intern_key(variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(CanonicalSerError::custom("map key must be a string"))
    }
}

fn finish_object(
    entries: Vec<(SmolStr, CanonicalValue)>,
    _already_sorted: bool,
) -> Result<CanonicalValue, CanonicalSerError> {
    if entries.len() > 1 {
        let mut seen = std::collections::HashSet::with_capacity(entries.len());
        for (key, _value) in &entries {
            if !seen.insert(key.as_str()) {
                recycle_entry_vec(entries);
                return Err(CanonicalSerError::custom("duplicate object key"));
            }
        }
    }
    Ok(CanonicalValue::Object(entries))
}

fn canonicalize_array(items: Vec<CanonicalValue>, delimiter: char) -> CanonicalArray {
    if items.is_empty() {
        return CanonicalArray::List(items);
    }
    if items.iter().all(is_scalar) {
        return CanonicalArray::Inline(items);
    }
    if items.len() >= min_tabular_rows() {
        if let Some(fields) = detect_tabular_fields(&items, delimiter) {
            let row_len = fields.len();
            let mut rows = take_value_vec(items.len().saturating_mul(row_len));
            let mut field_index = HashMap::with_capacity(row_len);
            for (idx, field) in fields.iter().enumerate() {
                field_index.insert(field.as_str(), idx);
            }
            for mut item in items {
                if let CanonicalValue::Object(entries) = &mut item {
                    let mut entries = std::mem::take(entries);
                    let mut ordered = vec![None; row_len];
                    for (key, value) in entries.drain(..) {
                        let idx = *field_index
                            .get(key.as_str())
                            .expect("tabular field missing");
                        ordered[idx] = Some(value);
                    }
                    for value in ordered.into_iter() {
                        rows.push(value.expect("tabular field missing"));
                    }
                    recycle_entry_vec(entries);
                }
            }
            return CanonicalArray::Tabular { fields, rows };
        }
    }
    CanonicalArray::List(items)
}

fn detect_tabular_fields(items: &[CanonicalValue], delimiter: char) -> Option<CanonicalFields> {
    let first = items.first()?;
    let row = match first {
        CanonicalValue::Object(entries) => entries,
        _ => return None,
    };
    if row.is_empty() {
        return None;
    }
    let mut fields: CanonicalFields = SmallVec::with_capacity(row.len());
    let mut field_set = std::collections::HashSet::with_capacity(row.len());
    for (key, value) in row.iter() {
        if !is_scalar(value) {
            return None;
        }
        if !field_set.insert(key.as_str()) {
            return None;
        }
        fields.push(CanonicalField::new(key.clone(), delimiter));
    }
    for item in items.iter().skip(1) {
        let row = match item {
            CanonicalValue::Object(entries) => entries,
            _ => return None,
        };
        if row.len() != fields.len() {
            return None;
        }
        let mut row_seen = std::collections::HashSet::with_capacity(fields.len());
        for (key, value) in row.iter() {
            if !is_scalar(value) {
                return None;
            }
            if !field_set.contains(key.as_str()) {
                return None;
            }
            if !row_seen.insert(key.as_str()) {
                return None;
            }
        }
    }
    Some(fields)
}

fn serialize_float(value: f64) -> Result<CanonicalValue, CanonicalSerError> {
    if !value.is_finite() {
        return Ok(CanonicalValue::Null);
    }
    if value == 0.0 {
        return Ok(CanonicalValue::Number(CanonicalNumber::U64(0)));
    }
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format(value);
    let canonical = if !has_exponent(raw) && is_canonical_number(raw) {
        SmolStr::new(raw)
    } else {
        let normalized = normalize_number_str(raw);
        if !is_canonical_number(&normalized) {
            return Err(CanonicalSerError::custom(format!(
                "non-canonical number: {normalized}"
            )));
        }
        SmolStr::from(normalized)
    };
    Ok(CanonicalValue::Number(CanonicalNumber::Text(canonical)))
}

fn smol_from_char(value: char) -> SmolStr {
    let mut buffer = [0u8; 4];
    SmolStr::new(value.encode_utf8(&mut buffer))
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok().and_then(|value| value.parse().ok())
}

fn min_estimate_fields() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| env_usize("TOON_ESTIMATE_MIN_FIELDS").unwrap_or(ESTIMATE_MIN_FIELDS))
}

fn min_estimate_items() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| env_usize("TOON_ESTIMATE_MIN_ITEMS").unwrap_or(ESTIMATE_MIN_ITEMS))
}

fn min_tabular_rows() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| env_usize("TOON_TABULAR_MIN_ROWS").unwrap_or(TABULAR_MIN_ROWS))
}

fn should_parallelize_encode(count: usize, estimate_bytes: usize, min_items: usize) -> bool {
    count >= min_items && estimate_bytes >= PARALLEL_MIN_BYTES
}

fn estimate_value_len(value: &CanonicalValue, profile: CanonicalProfile, indent: usize) -> usize {
    match value {
        CanonicalValue::Null => 4,
        CanonicalValue::Bool(true) => 4,
        CanonicalValue::Bool(false) => 5,
        CanonicalValue::Number(value) => value.len(),
        CanonicalValue::String(value) => encoded_string_len(value),
        CanonicalValue::Array(values) => estimate_array_len(values, profile, indent),
        CanonicalValue::Object(map) => estimate_object_len(map, profile, indent),
    }
}

fn estimate_object_len(
    map: &[(SmolStr, CanonicalValue)],
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let mut len = 0;
    for (idx, (key, value)) in map.iter().enumerate() {
        if idx > 0 {
            len += 1;
        }
        len += indent;
        len += encoded_key_len(key.as_str());
        match value {
            CanonicalValue::Array(values) => {
                len += estimate_array_field_len(values, profile, indent);
            }
            CanonicalValue::Object(inner) => {
                len += 1;
                if !inner.is_empty() {
                    len += 1;
                    len += estimate_object_len(inner, profile, indent + profile.indent_spaces);
                }
            }
            _ => {
                len += 2;
                len += estimate_value_len(value, profile, indent);
            }
        }
    }
    len
}

fn estimate_array_len(values: &CanonicalArray, profile: CanonicalProfile, indent: usize) -> usize {
    match values {
        CanonicalArray::Inline(items) | CanonicalArray::List(items) => {
            estimate_array_list_len(items, profile, indent)
        }
        CanonicalArray::Tabular { fields, rows } => {
            estimate_tabular_as_list_len(fields, rows, profile, indent)
        }
    }
}

fn estimate_array_list_len(
    values: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let mut len = 0;
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            len += 1;
        }
        len += indent;
        if is_scalar(value) {
            len += 2;
            len += estimate_value_len(value, profile, indent + profile.indent_spaces);
        } else {
            len += 1;
            if !is_empty_container(value) {
                len += 1;
                len += estimate_value_len(value, profile, indent + profile.indent_spaces);
            }
        }
    }
    len
}

fn estimate_tabular_as_list_len(
    fields: &[CanonicalField],
    rows: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let row_len = fields.len();
    if row_len == 0 {
        return 0;
    }
    let mut len = 0;
    for (idx, row) in rows.chunks_exact(row_len).enumerate() {
        if idx > 0 {
            len += 1;
        }
        len += indent;
        len += 1;
        len += 1;
        len +=
            estimate_object_from_fields_len(fields, row, profile, indent + profile.indent_spaces);
    }
    len
}

fn estimate_array_field_len(
    values: &CanonicalArray,
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let len_items = array_len(values);
    let mut len = array_header_len(len_items, profile.delimiter);
    if len_items == 0 {
        return len + 1;
    }

    match values {
        CanonicalArray::Inline(values) => {
            len += 2;
            len += estimate_inline_array_len(values, profile);
        }
        CanonicalArray::Tabular { fields, rows } => {
            len += 1;
            len += header_fields_len(fields, profile.delimiter);
            len += 1;
            len += 2;
            let row_len = fields.len();
            if row_len == 0 {
                return len;
            }
            for (row_idx, row) in rows.chunks_exact(row_len).enumerate() {
                if row_idx > 0 {
                    len += 1;
                }
                len += indent + profile.indent_spaces;
                for (field_idx, cell) in row.iter().enumerate() {
                    if field_idx > 0 {
                        len += inline_separator_len(profile.delimiter);
                    }
                    len += estimate_value_len(cell, profile, 0);
                }
            }
        }
        CanonicalArray::List(values) => {
            len += 2;
            len += estimate_array_list_len(values, profile, indent + profile.indent_spaces);
        }
    }
    len
}

fn estimate_object_from_fields_len(
    fields: &[CanonicalField],
    values: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let mut len = 0;
    for (idx, (field, value)) in fields.iter().zip(values.iter()).enumerate() {
        if idx > 0 {
            len += 1;
        }
        len += indent;
        len += encoded_key_len(field.as_str());
        match value {
            CanonicalValue::Array(values) => {
                len += estimate_array_field_len(values, profile, indent);
            }
            CanonicalValue::Object(inner) => {
                len += 1;
                if !inner.is_empty() {
                    len += 1;
                    len += estimate_object_len(inner, profile, indent + profile.indent_spaces);
                }
            }
            _ => {
                len += 2;
                len += estimate_value_len(value, profile, indent);
            }
        }
    }
    len
}

fn estimate_inline_array_len(values: &[CanonicalValue], profile: CanonicalProfile) -> usize {
    let mut len = 0;
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            len += inline_separator_len(profile.delimiter);
        }
        len += estimate_value_len(value, profile, 0);
    }
    len
}

fn estimate_field_len(
    key: &str,
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let mut len = indent + encoded_key_len(key);
    match value {
        CanonicalValue::Array(values) => {
            len += estimate_array_field_len(values, profile, indent);
        }
        CanonicalValue::Object(inner) => {
            len += 1;
            if !inner.is_empty() {
                len += 1;
                len += estimate_object_len(inner, profile, indent + profile.indent_spaces);
            }
        }
        _ => {
            len += 2;
            len += estimate_value_len(value, profile, indent);
        }
    }
    len
}

fn estimate_array_item_len(
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> usize {
    let mut len = indent + 1;
    if is_scalar(value) {
        len += 1;
        len += estimate_value_len(value, profile, indent + profile.indent_spaces);
    } else if !is_empty_container(value) {
        len += 1;
        len += estimate_value_len(value, profile, indent + profile.indent_spaces);
    }
    len
}

fn array_header_len(len: usize, delimiter: super::profile::CanonicalDelimiter) -> usize {
    let mut buffer = itoa::Buffer::new();
    let mut size = 2 + buffer.format(len).len();
    if delimiter != super::profile::CanonicalDelimiter::Comma {
        size += 1;
    }
    size
}

fn header_fields_len(
    fields: &[CanonicalField],
    delimiter: super::profile::CanonicalDelimiter,
) -> usize {
    let mut len = 0;
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            len += header_separator_len(delimiter);
        }
        len += encoded_header_field_len(field);
    }
    len
}

fn inline_separator_len(delimiter: super::profile::CanonicalDelimiter) -> usize {
    match delimiter {
        super::profile::CanonicalDelimiter::Comma => 2,
        super::profile::CanonicalDelimiter::Tab => 1,
        super::profile::CanonicalDelimiter::Pipe => 1,
    }
}

fn header_separator_len(delimiter: super::profile::CanonicalDelimiter) -> usize {
    match delimiter {
        super::profile::CanonicalDelimiter::Comma => 1,
        super::profile::CanonicalDelimiter::Tab => 1,
        super::profile::CanonicalDelimiter::Pipe => 1,
    }
}

fn encoded_key_len(key: &str) -> usize {
    if is_canonical_unquoted_key(key) {
        key.len()
    } else {
        2 + escaped_len(key)
    }
}

fn encoded_string_len(value: &CanonicalString) -> usize {
    if value.needs_quote {
        2 + escaped_len_with_flag(value.as_str(), value.needs_escape)
    } else {
        value.value.len()
    }
}

fn encoded_header_field_len(field: &CanonicalField) -> usize {
    if field.header_needs_quote {
        2 + escaped_len_with_flag(field.as_str(), field.header_needs_escape)
    } else {
        field.name.len()
    }
}

fn escaped_len(s: &str) -> usize {
    escaped_len_with_flag(s, needs_escaping(s))
}

fn escaped_len_with_flag(s: &str, needs_escape: bool) -> usize {
    if !needs_escape {
        return s.len();
    }
    s.len()
        + s.as_bytes()
            .iter()
            .filter(|byte| matches!(byte, b'\n' | b'\r' | b'\t' | b'"' | b'\\'))
            .count()
}

pub fn encode_canonical<T: Serialize>(value: &T, profile: CanonicalProfile) -> ToonResult<String> {
    let value = value
        .serialize(CanonicalSerializer::new(profile))
        .map_err(ToonError::from)?;
    let capacity = estimate_capacity(&value, profile);
    let mut out = String::with_capacity(capacity);
    encode_value_root(&mut out, &value, profile)?;
    Ok(out)
}

fn encode_value_root(
    out: &mut String,
    value: &CanonicalValue,
    profile: CanonicalProfile,
) -> ToonResult<()> {
    match value {
        CanonicalValue::Array(values) => encode_root_array(out, values, profile),
        _ => encode_value(out, value, profile, 0),
    }
}

fn estimate_capacity(value: &CanonicalValue, profile: CanonicalProfile) -> usize {
    if should_estimate(value, profile) {
        estimate_value_len(value, profile, 0)
    } else {
        match value {
            CanonicalValue::Array(_) | CanonicalValue::Object(_) => {
                quick_capacity_hint(value, profile).max(DEFAULT_CONTAINER_CAPACITY)
            }
            _ => quick_capacity_hint(value, profile),
        }
    }
}

fn should_estimate(value: &CanonicalValue, profile: CanonicalProfile) -> bool {
    let guess = quick_capacity_hint(value, profile);
    match value {
        CanonicalValue::Array(values) => {
            array_len(values) >= min_estimate_items() && guess >= ESTIMATE_MIN_GUESS
        }
        CanonicalValue::Object(map) => {
            map.len() >= min_estimate_fields() && guess >= ESTIMATE_MIN_GUESS
        }
        _ => false,
    }
}

fn quick_capacity_hint(value: &CanonicalValue, profile: CanonicalProfile) -> usize {
    match value {
        CanonicalValue::Null => 4,
        CanonicalValue::Bool(true) => 4,
        CanonicalValue::Bool(false) => 5,
        CanonicalValue::Number(value) => value.len(),
        CanonicalValue::String(value) => value.value.len() + 2,
        CanonicalValue::Array(values) => {
            let len_items = array_len(values);
            let cap = array_header_len(len_items, profile.delimiter) + 2 + len_items * 8;
            cap.max(SMALL_CAPACITY)
        }
        CanonicalValue::Object(map) => {
            let cap = map.len() * 12;
            cap.max(SMALL_CAPACITY)
        }
    }
}

fn encode_value(
    out: &mut String,
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    match value {
        CanonicalValue::Null => out.push_str("null"),
        CanonicalValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        CanonicalValue::Number(number) => write_number(out, number)?,
        CanonicalValue::String(value) => encode_string(out, value),
        CanonicalValue::Array(values) => encode_array(out, values, profile, indent)?,
        CanonicalValue::Object(map) => encode_object(out, map, profile, indent)?,
    }
    Ok(())
}

fn write_number(out: &mut String, number: &CanonicalNumber) -> ToonResult<()> {
    match number {
        CanonicalNumber::I64(value) => {
            let mut buffer = itoa::Buffer::new();
            out.push_str(buffer.format(*value));
        }
        CanonicalNumber::U64(value) => {
            let mut buffer = itoa::Buffer::new();
            out.push_str(buffer.format(*value));
        }
        CanonicalNumber::Text(value) => {
            if !is_canonical_number(value.as_str()) {
                return Err(ToonError::SerializationError(format!(
                    "non-canonical number: {value}"
                )));
            }
            out.push_str(value);
        }
    }
    Ok(())
}

fn has_exponent(value: &str) -> bool {
    value
        .as_bytes()
        .iter()
        .any(|byte| matches!(byte, b'e' | b'E'))
}

fn encode_object(
    out: &mut String,
    map: &[(SmolStr, CanonicalValue)],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let estimate_bytes = if map.len() >= PARALLEL_MIN_FIELDS {
        estimate_object_len(map, profile, indent)
    } else {
        0
    };
    if should_parallelize_encode(map.len(), estimate_bytes, PARALLEL_MIN_FIELDS) {
        let parts: Vec<ToonResult<String>> = map_items_parallel(map, |(key, value)| {
            encode_field_to_string(key.as_str(), value, profile, indent)
        });
        for (idx, part) in parts.into_iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&part?);
        }
        return Ok(());
    }

    for (idx, (key, value)) in map.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        push_indent(out, indent);
        encode_key(out, key.as_str());

        match value {
            CanonicalValue::Array(values) => {
                encode_array_field(out, values, profile, indent)?;
            }
            CanonicalValue::Object(inner) => {
                out.push_str(":");
                if !inner.is_empty() {
                    out.push('\n');
                    encode_object(out, inner, profile, indent + profile.indent_spaces)?;
                }
            }
            _ => {
                out.push_str(": ");
                encode_value(out, value, profile, indent)?;
            }
        }
    }

    Ok(())
}

fn encode_root_array(
    out: &mut String,
    values: &CanonicalArray,
    profile: CanonicalProfile,
) -> ToonResult<()> {
    let len = array_len(values);
    push_array_header(out, len, profile.delimiter);

    if len == 0 {
        out.push(':');
        return Ok(());
    }

    match values {
        CanonicalArray::Inline(values) => {
            out.push_str(": ");
            encode_inline_array(out, values, profile)?;
        }
        CanonicalArray::Tabular { fields, rows } => {
            out.push('{');
            for (idx, field) in fields.iter().enumerate() {
                if idx > 0 {
                    push_header_separator(out, profile.delimiter);
                }
                encode_header_field(out, field);
            }
            out.push('}');
            out.push(':');
            out.push('\n');
            encode_tabular_rows(out, fields, rows, profile, profile.indent_spaces)?;
        }
        CanonicalArray::List(values) => {
            out.push(':');
            out.push('\n');
            encode_array_list(out, values, profile, profile.indent_spaces)?;
        }
    }

    Ok(())
}

fn encode_array(
    out: &mut String,
    values: &CanonicalArray,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    match values {
        CanonicalArray::Inline(items) | CanonicalArray::List(items) => {
            return encode_array_list(out, items, profile, indent);
        }
        CanonicalArray::Tabular { fields, rows } => {
            return encode_tabular_as_list(out, fields, rows, profile, indent);
        }
    }
}

fn encode_list_item(
    out: &mut String,
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    if let CanonicalValue::Object(entries) = value {
        if let Some((key, CanonicalValue::Array(CanonicalArray::Tabular { fields, rows }))) =
            entries.first()
        {
            encode_list_item_tabular_array(out, key.as_str(), fields, rows, profile, indent)?;
            if entries.len() > 1 {
                out.push('\n');
                encode_object(out, &entries[1..], profile, indent + profile.indent_spaces)?;
            }
            return Ok(());
        }
        push_indent(out, indent);
        if entries.is_empty() {
            out.push('-');
            return Ok(());
        }
        out.push_str("- ");
        let (first_key, first_value) = &entries[0];
        encode_key(out, first_key.as_str());
        match first_value {
            CanonicalValue::Array(values) => {
                encode_array_field(out, values, profile, indent + profile.indent_spaces)?
            }
            CanonicalValue::Object(inner) => {
                out.push(':');
                if !inner.is_empty() {
                    out.push('\n');
                    encode_object(out, inner, profile, indent + profile.indent_spaces)?;
                }
            }
            _ => {
                out.push_str(": ");
                encode_value(out, first_value, profile, indent)?;
            }
        }
        if entries.len() > 1 {
            out.push('\n');
            encode_object(out, &entries[1..], profile, indent + profile.indent_spaces)?;
        }
        return Ok(());
    }

    if let CanonicalValue::Array(values) = value {
        push_indent(out, indent);
        out.push_str("- ");
        encode_array_field(out, values, profile, indent)?;
        return Ok(());
    }

    push_indent(out, indent);
    if is_scalar(value) {
        out.push_str("- ");
        encode_value(out, value, profile, indent + profile.indent_spaces)?;
    } else {
        out.push('-');
        if !is_empty_container(value) {
            out.push('\n');
            encode_value(out, value, profile, indent + profile.indent_spaces)?;
        }
    }
    Ok(())
}

fn encode_array_list(
    out: &mut String,
    values: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let estimate_bytes = if values.len() >= PARALLEL_MIN_ITEMS {
        estimate_array_list_len(values, profile, indent)
    } else {
        0
    };
    if should_parallelize_encode(values.len(), estimate_bytes, PARALLEL_MIN_ITEMS) {
        let parts: Vec<ToonResult<String>> = map_items_parallel(values, |value| {
            encode_array_item_to_string(value, profile, indent)
        });
        for (idx, part) in parts.into_iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&part?);
        }
        return Ok(());
    }

    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        encode_list_item(out, value, profile, indent)?;
    }
    Ok(())
}

fn encode_list_item_tabular_array(
    out: &mut String,
    key: &str,
    fields: &[CanonicalField],
    rows: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let row_len = fields.len();
    if row_len == 0 {
        return Ok(());
    }
    if rows.len() % row_len != 0 {
        return Err(ToonError::SerializationError(
            "tabular row field count mismatch".to_string(),
        ));
    }
    push_indent(out, indent);
    out.push_str("- ");
    encode_key(out, key);
    let row_count = rows.len() / row_len;
    push_array_header(out, row_count, profile.delimiter);
    out.push('{');
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            push_header_separator(out, profile.delimiter);
        }
        encode_header_field(out, field);
    }
    out.push('}');
    out.push(':');
    if rows.is_empty() {
        return Ok(());
    }
    out.push('\n');
    encode_tabular_rows(
        out,
        fields,
        rows,
        profile,
        indent + profile.indent_spaces * 2,
    )?;
    Ok(())
}

fn encode_tabular_as_list(
    out: &mut String,
    fields: &[CanonicalField],
    rows: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let row_len = fields.len();
    if row_len == 0 {
        return Ok(());
    }
    if rows.len() % row_len != 0 {
        return Err(ToonError::SerializationError(
            "tabular row field count mismatch".to_string(),
        ));
    }
    for (idx, row) in rows.chunks_exact(row_len).enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        push_indent(out, indent);
        out.push('-');
        out.push('\n');
        encode_object_from_fields(out, fields, row, profile, indent + profile.indent_spaces)?;
    }
    Ok(())
}

fn encode_object_from_fields(
    out: &mut String,
    fields: &[CanonicalField],
    values: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    if fields.len() != values.len() {
        return Err(ToonError::SerializationError(
            "object field count mismatch".to_string(),
        ));
    }
    for (idx, (field, value)) in fields.iter().zip(values.iter()).enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        push_indent(out, indent);
        encode_key(out, field.as_str());
        match value {
            CanonicalValue::Array(values) => encode_array_field(out, values, profile, indent)?,
            CanonicalValue::Object(inner) => {
                out.push(':');
                if !inner.is_empty() {
                    out.push('\n');
                    encode_object(out, inner, profile, indent + profile.indent_spaces)?;
                }
            }
            _ => {
                out.push_str(": ");
                encode_value(out, value, profile, indent)?;
            }
        }
    }
    Ok(())
}

fn encode_tabular_rows(
    out: &mut String,
    fields: &[CanonicalField],
    rows: &[CanonicalValue],
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let row_len = fields.len();
    if row_len == 0 {
        return Ok(());
    }
    if rows.len() % row_len != 0 {
        return Err(ToonError::SerializationError(
            "tabular row field count mismatch".to_string(),
        ));
    }
    for (row_idx, row) in rows.chunks_exact(row_len).enumerate() {
        if row_idx > 0 {
            out.push('\n');
        }
        push_indent(out, indent);
        for (field_idx, cell) in row.iter().enumerate() {
            if field_idx > 0 {
                push_inline_separator(out, profile.delimiter);
            }
            encode_value(out, cell, profile, 0)?;
        }
    }
    Ok(())
}

fn encode_array_field(
    out: &mut String,
    values: &CanonicalArray,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<()> {
    let len = array_len(values);
    push_array_header(out, len, profile.delimiter);

    if len == 0 {
        out.push(':');
        return Ok(());
    }

    match values {
        CanonicalArray::Inline(values) => {
            out.push_str(": ");
            encode_inline_array(out, values, profile)?;
        }
        CanonicalArray::Tabular { fields, rows } => {
            out.push('{');
            for (idx, field) in fields.iter().enumerate() {
                if idx > 0 {
                    push_header_separator(out, profile.delimiter);
                }
                encode_header_field(out, field);
            }
            out.push('}');
            out.push(':');
            out.push('\n');
            encode_tabular_rows(out, fields, rows, profile, indent + profile.indent_spaces)?;
        }
        CanonicalArray::List(values) => {
            out.push(':');
            out.push('\n');
            encode_array_list(out, values, profile, indent + profile.indent_spaces)?;
        }
    }

    Ok(())
}

fn encode_inline_array(
    out: &mut String,
    values: &[CanonicalValue],
    profile: CanonicalProfile,
) -> ToonResult<()> {
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            push_inline_separator(out, profile.delimiter);
        }
        encode_value(out, value, profile, 0)?;
    }
    Ok(())
}

fn array_len(array: &CanonicalArray) -> usize {
    match array {
        CanonicalArray::Inline(values) | CanonicalArray::List(values) => values.len(),
        CanonicalArray::Tabular { fields, rows } => {
            let row_len = fields.len();
            if row_len == 0 {
                0
            } else {
                rows.len() / row_len
            }
        }
    }
}

fn encode_key(out: &mut String, key: &str) {
    if is_canonical_unquoted_key(key) {
        out.push_str(key);
    } else {
        out.push('"');
        if needs_escaping(key) {
            escape_string_into(out, key);
        } else {
            out.push_str(key);
        }
        out.push('"');
    }
}

fn encode_string(out: &mut String, value: &CanonicalString) {
    if value.needs_quote {
        out.push('"');
        if value.needs_escape {
            escape_string_into(out, value.as_str());
        } else {
            out.push_str(value.as_str());
        }
        out.push('"');
    } else {
        out.push_str(value.as_str());
    }
}

fn encode_field_to_string(
    key: &str,
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<String> {
    let mut out = String::with_capacity(estimate_field_len(key, value, profile, indent));
    push_indent(&mut out, indent);
    encode_key(&mut out, key);
    match value {
        CanonicalValue::Array(values) => encode_array_field(&mut out, values, profile, indent)?,
        CanonicalValue::Object(inner) => {
            out.push(':');
            if !inner.is_empty() {
                out.push('\n');
                encode_object(&mut out, inner, profile, indent + profile.indent_spaces)?;
            }
        }
        _ => {
            out.push_str(": ");
            encode_value(&mut out, value, profile, indent)?;
        }
    }
    Ok(out)
}

fn encode_array_item_to_string(
    value: &CanonicalValue,
    profile: CanonicalProfile,
    indent: usize,
) -> ToonResult<String> {
    let mut out = String::with_capacity(estimate_array_item_len(value, profile, indent));
    encode_list_item(&mut out, value, profile, indent)?;
    Ok(out)
}

fn push_indent(out: &mut String, indent: usize) {
    let mut remaining = indent;
    while remaining >= INDENT_CHUNK.len() {
        out.push_str(INDENT_CHUNK);
        remaining -= INDENT_CHUNK.len();
    }
    if remaining > 0 {
        out.push_str(&INDENT_CHUNK[..remaining]);
    }
}

fn is_scalar(value: &CanonicalValue) -> bool {
    matches!(
        value,
        CanonicalValue::Null
            | CanonicalValue::Bool(_)
            | CanonicalValue::Number(_)
            | CanonicalValue::String(_)
    )
}

fn is_empty_container(value: &CanonicalValue) -> bool {
    match value {
        CanonicalValue::Array(array) => array_len(array) == 0,
        CanonicalValue::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn escape_string_into(out: &mut String, s: &str) {
    let bytes = s.as_bytes();
    let mut start = 0;
    for (idx, byte) in bytes.iter().enumerate() {
        let escaped = match byte {
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            b'"' => "\\\"",
            b'\\' => "\\\\",
            _ => continue,
        };
        if start < idx {
            out.push_str(&s[start..idx]);
        }
        out.push_str(escaped);
        start = idx + 1;
    }
    if start < s.len() {
        out.push_str(&s[start..]);
    }
}

fn needs_escaping(s: &str) -> bool {
    s.as_bytes()
        .iter()
        .any(|byte| matches!(byte, b'\n' | b'\r' | b'\t' | b'"' | b'\\'))
}

fn push_array_header(out: &mut String, len: usize, delimiter: super::profile::CanonicalDelimiter) {
    out.push('[');
    let mut buffer = itoa::Buffer::new();
    out.push_str(buffer.format(len));
    if delimiter != super::profile::CanonicalDelimiter::Comma {
        out.push(delimiter.as_char());
    }
    out.push(']');
}

fn push_inline_separator(out: &mut String, delimiter: super::profile::CanonicalDelimiter) {
    match delimiter {
        super::profile::CanonicalDelimiter::Comma => out.push(','),
        super::profile::CanonicalDelimiter::Tab => out.push('\t'),
        super::profile::CanonicalDelimiter::Pipe => out.push('|'),
    }
}

fn push_header_separator(out: &mut String, delimiter: super::profile::CanonicalDelimiter) {
    match delimiter {
        super::profile::CanonicalDelimiter::Comma => out.push(','),
        super::profile::CanonicalDelimiter::Tab => out.push('\t'),
        super::profile::CanonicalDelimiter::Pipe => out.push('|'),
    }
}

fn encode_header_field(out: &mut String, field: &CanonicalField) {
    if field.header_needs_quote {
        out.push('"');
        if field.header_needs_escape {
            escape_string_into(out, field.as_str());
        } else {
            out.push_str(field.as_str());
        }
        out.push('"');
    } else {
        out.push_str(field.as_str());
    }
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
        out.extend(std::iter::repeat('0').take((-new_pos) as usize));
        out.push_str(&digits);
        return trim_number(out);
    }

    if new_pos as usize >= digits.len() {
        out.push_str(&digits);
        out.extend(std::iter::repeat('0').take(new_pos as usize - digits.len()));
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

#[cfg(test)]
mod tests {
    use super::normalize_number_str;

    #[rstest::rstest]
    fn test_normalize_number_str() {
        assert_eq!(normalize_number_str("1e3"), "1000");
        assert_eq!(normalize_number_str("1.2300"), "1.23");
        assert_eq!(normalize_number_str("-0.0"), "0");
        assert_eq!(normalize_number_str("0e3"), "0");
        assert_eq!(normalize_number_str("1e-3"), "0.001");
    }
}
