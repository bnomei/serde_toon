mod parser;
mod pool;
mod scan;
mod serde;

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read};

use ::serde::de::DeserializeOwned;
use memchr::{memchr, memchr2, memchr3, memchr_iter};
use serde_json::{Map, Value};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::arena::{ArenaView, NodeData, NodeKind};
use crate::{DecodeOptions, Error, Indent, Location, Result};

#[cfg(feature = "parallel")]
use ::serde::Deserialize;
#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[cfg(feature = "parallel")]
const PARALLEL_ARRAY_MIN_ITEMS: usize = 64;

pub fn from_str<T: DeserializeOwned>(input: &str, options: &DecodeOptions) -> Result<T> {
    let mut arena = ArenaView::with_parts(input, pool::take_arena_parts());
    let result = (|| {
        let root = parser::parse_into(&mut arena, options)?;
        let mut de = self::serde::ArenaDeserializer::new(&arena, root);
        T::deserialize(&mut de).map_err(|err| {
            Error::deserialize_with_source(format!("deserialize failed: {err}"), err)
        })
    })();
    pool::put_arena_parts(arena.into_parts());
    result
}

pub fn from_str_value(input: &str, options: &DecodeOptions) -> Result<Value> {
    let mut arena = ArenaView::with_parts(input, pool::take_arena_parts());
    let result = (|| {
        let root = parser::parse_into(&mut arena, options)?;
        arena_to_value(&arena, root)
    })();
    pool::put_arena_parts(arena.into_parts());
    result
}

#[cfg(feature = "parallel")]
pub fn from_str_parallel<T: DeserializeOwned + Send>(
    input: &str,
    options: &DecodeOptions,
) -> Result<Vec<T>> {
    let mut arena = ArenaView::with_parts(input, pool::take_arena_parts());
    let result = (|| {
        let root = parser::parse_into(&mut arena, options)?;
        let node = &arena.nodes[root];
        if matches!(node.kind, NodeKind::Array) {
            let children = arena.children(node);
            if children.len() >= PARALLEL_ARRAY_MIN_ITEMS {
                let results: Vec<Result<T>> = children
                    .par_iter()
                    .map(|child| {
                        let mut de = self::serde::ArenaDeserializer::new(&arena, *child);
                        T::deserialize(&mut de).map_err(|err| {
                            Error::deserialize_with_source(
                                format!("deserialize failed: {err}"),
                                err,
                            )
                        })
                    })
                    .collect();
                return results.into_iter().collect();
            }
        }
        let mut de = self::serde::ArenaDeserializer::new(&arena, root);
        Vec::<T>::deserialize(&mut de).map_err(|err| {
            Error::deserialize_with_source(format!("deserialize failed: {err}"), err)
        })
    })();
    pool::put_arena_parts(arena.into_parts());
    result
}

pub fn from_slice<T: DeserializeOwned>(input: &[u8], options: &DecodeOptions) -> Result<T> {
    let text = std::str::from_utf8(input)
        .map_err(|err| Error::decode_with_source(format!("invalid utf-8: {err}"), err))?;
    from_str(text, options)
}

/// Decode a value from a reader by buffering the entire input into memory.
///
/// This reads all bytes from `reader` into a `Vec<u8>` before decoding, so large or
/// untrusted inputs can exhaust memory. Prefer [`from_reader_streaming`] for
/// incremental, bounded-memory processing. For alternate decoding behavior, see
/// [`DecodeOptions`].
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R, options: &DecodeOptions) -> Result<T> {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .map_err(|err| Error::decode_with_source(format!("read failed: {err}"), err))?;
    from_slice(&buffer, options)
}

pub fn from_reader_streaming<T: DeserializeOwned, R: BufRead>(
    reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    let mut decoder = Decoder::new(options);
    let value = decoder.decode_reader_streaming(reader)?;
    serde_json::from_value(value)
        .map_err(|err| Error::deserialize_with_source(format!("deserialize failed: {err}"), err))
}

pub fn validate_str(input: &str, options: &DecodeOptions) -> Result<()> {
    let mut arena = ArenaView::with_parts(input, pool::take_arena_parts());
    let result = (|| {
        parser::parse_into_validate(&mut arena, options)?;
        Ok(())
    })();
    pool::put_arena_parts(arena.into_parts());
    result
}

fn arena_to_value(arena: &ArenaView<'_>, node_index: usize) -> Result<Value> {
    let node = &arena.nodes[node_index];
    match node.kind {
        NodeKind::Null => Ok(Value::Null),
        NodeKind::Bool => match node.data {
            NodeData::Bool(value) => Ok(Value::Bool(value)),
            _ => Err(Error::decode("invalid bool payload")),
        },
        NodeKind::String => match node.data {
            NodeData::String(index) => arena
                .get_str(index)
                .map(|value| Value::String(value.to_string()))
                .ok_or_else(|| Error::decode("invalid string span")),
            _ => Err(Error::decode("invalid string payload")),
        },
        NodeKind::Number => match node.data {
            NodeData::Number(index) => {
                let token = arena
                    .get_num_str(index)
                    .ok_or_else(|| Error::decode("invalid number span"))?;
                let number =
                    parse_number_token(token).ok_or_else(|| Error::decode("invalid number"))?;
                Ok(Value::Number(number))
            }
            _ => Err(Error::decode("invalid number payload")),
        },
        NodeKind::Array => {
            let mut items = Vec::with_capacity(node.child_len);
            for &child in arena.children(node) {
                items.push(arena_to_value(arena, child)?);
            }
            Ok(Value::Array(items))
        }
        NodeKind::Object => {
            let mut map = Map::with_capacity(node.child_len);
            for pair in arena.pairs(node) {
                let key = arena
                    .get_key(pair.key)
                    .ok_or_else(|| Error::decode("invalid object key"))?;
                let value = arena_to_value(arena, pair.value)?;
                map.insert(key.to_string(), value);
            }
            Ok(Value::Object(map))
        }
    }
}

struct Decoder {
    indent_size: usize,
    strict: bool,
    active_delimiter: char,
    delimiter_stack: Vec<char>,
    header_depths: Vec<usize>,
}

type TokenBuf<'a> = SmallVec<[&'a str; 16]>;

impl Decoder {
    fn new(options: &DecodeOptions) -> Self {
        let Indent::Spaces(indent_size) = options.indent;
        Self {
            indent_size,
            strict: options.strict,
            active_delimiter: ',',
            delimiter_stack: Vec::new(),
            header_depths: Vec::new(),
        }
    }

    fn push_delimiter(&mut self, delimiter: char) {
        self.delimiter_stack.push(self.active_delimiter);
        self.active_delimiter = delimiter;
    }

    fn pop_delimiter(&mut self) {
        if let Some(previous) = self.delimiter_stack.pop() {
            self.active_delimiter = previous;
        }
    }

    fn decode_single_line(
        &mut self,
        line: &str,
        line_meta: &Line<'_>,
        line_idx: usize,
    ) -> Result<Value> {
        if let Some(array) = self
            .parse_array_line(line)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?
        {
            return Ok(array);
        }
        if let Some(header) = self
            .parse_array_header(line)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?
        {
            if let Some(key) = header.key.as_ref() {
                let value = self.build_array_value(&header).map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, line, err)
                })?;
                let mut map = Map::new();
                self.insert_key_value(&mut map, key.clone(), value)
                    .map_err(|err| {
                        self.attach_location_for_slice(line_meta, line_idx, line, err)
                    })?;
                return Ok(Value::Object(map));
            }
        }
        if let Some((key, value)) = self
            .split_key_value(line)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?
        {
            let mut map = Map::new();
            let key = self
                .parse_key_token(trim_ascii(key))
                .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, key, err))?;
            let value = if trim_ascii(value).is_empty() {
                Value::Object(Map::new())
            } else {
                let value_trimmed = trim_ascii(value);
                self.parse_value_token(value).map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, value_trimmed, err)
                })?
            };
            self.insert_key_value(&mut map, key, value)
                .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?;
            return Ok(Value::Object(map));
        }
        if self.strict {
            self.parse_array_header(line)
                .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?;
        }
        self.parse_value_token(line)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))
    }

    fn parse_array_line(&self, line: &str) -> Result<Option<Value>> {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('[') {
            return Ok(None);
        }
        let header = match self.parse_array_header(trimmed)? {
            Some(header) => header,
            None => return Ok(None),
        };
        if header.key.is_some() {
            return Ok(None);
        }
        self.build_array_value(&header).map(Some)
    }

    fn build_array_value(&self, header: &HeaderLine) -> Result<Value> {
        if header.keyed {
            if self.strict && header.len != 0 {
                return Err(Error::decode("array length mismatch"));
            }
            return Ok(Value::Object(Map::new()));
        }
        let items = match header.inline.as_deref() {
            Some(inline) => self.parse_inline_array(inline, header.delimiter, header.len)?,
            None => Vec::new(),
        };
        if self.strict && header.inline.is_none() && header.len > 0 {
            return Err(Error::decode("array payload required"));
        }
        if self.strict && header.len != items.len() {
            return Err(Error::decode("array length mismatch"));
        }
        Ok(Value::Array(items))
    }

    fn parse_inline_array(
        &self,
        inline: &str,
        delimiter: char,
        expected_len: usize,
    ) -> Result<Vec<Value>> {
        let tokens = self.split_delimited_with_capacity(inline, delimiter, expected_len)?;
        let mut values = Vec::new();
        for token in tokens {
            if token.is_empty() {
                values.push(Value::String(String::new()));
            } else if token == "[]" {
                values.push(Value::String("[]".into()));
            } else {
                values.push(self.parse_value_token(token)?);
            }
        }
        Ok(values)
    }

    fn split_delimited<'a>(&self, input: &'a str, delimiter: char) -> Result<TokenBuf<'a>> {
        self.split_delimited_with_capacity(input, delimiter, 0)
    }

    fn split_delimited_with_capacity<'a>(
        &self,
        input: &'a str,
        delimiter: char,
        _expected_len: usize,
    ) -> Result<TokenBuf<'a>> {
        let mut tokens = TokenBuf::new();
        self.split_delimited_into(input, delimiter, &mut tokens)?;
        Ok(tokens)
    }

    fn split_delimited_into<'a>(
        &self,
        input: &'a str,
        delimiter: char,
        tokens: &mut TokenBuf<'a>,
    ) -> Result<()> {
        tokens.clear();
        let bytes = input.as_bytes();
        if input.is_ascii() && !bytes.contains(&b'"') && !bytes.contains(&b'\\') {
            let delim_byte = delimiter as u8;
            let mut start = 0;
            for idx in memchr_iter(delim_byte, bytes) {
                let token = trim_ascii(&input[start..idx]);
                tokens.push(token);
                start = idx + 1;
            }
            if start < bytes.len() || input.ends_with(delimiter) {
                let token = trim_ascii(&input[start..]);
                tokens.push(token);
            }
            return Ok(());
        }

        let mut in_quotes = false;
        let mut escape = false;
        let delim_byte = delimiter as u8;
        let mut start = 0;
        let mut idx = 0;

        while idx < bytes.len() {
            if escape {
                escape = false;
                idx += 1;
                continue;
            }
            if in_quotes {
                match memchr2(b'\\', b'"', &bytes[idx..]) {
                    Some(offset) => {
                        let pos = idx + offset;
                        match bytes[pos] {
                            b'\\' => {
                                escape = true;
                                idx = pos + 1;
                            }
                            b'"' => {
                                in_quotes = false;
                                idx = pos + 1;
                            }
                            _ => unreachable!("memchr2 returned unexpected byte"),
                        }
                    }
                    None => {
                        idx = bytes.len();
                    }
                }
                continue;
            }
            match memchr2(delim_byte, b'"', &bytes[idx..]) {
                Some(offset) => {
                    let pos = idx + offset;
                    if bytes[pos] == b'"' {
                        in_quotes = true;
                        idx = pos + 1;
                        continue;
                    }
                    let token = trim_ascii(&input[start..pos]);
                    tokens.push(token);
                    start = pos + 1;
                    idx = start;
                }
                None => {
                    break;
                }
            }
        }

        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }

        if start < bytes.len() || input.ends_with(delimiter) {
            let token = trim_ascii(&input[start..]);
            tokens.push(token);
        }
        Ok(())
    }

    fn parse_value_token(&self, token: &str) -> Result<Value> {
        let token = trim_ascii(token);
        if token.is_empty() {
            return Err(Error::decode("empty value"));
        }
        if token.starts_with('"') {
            return Ok(Value::String(self.parse_quoted(token)?));
        }
        if token == "[]" {
            return Ok(Value::Array(Vec::new()));
        }
        match token {
            "null" => return Ok(Value::Null),
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            _ => {}
        }
        if let Some(number) = self.parse_number(token) {
            return Ok(Value::Number(number));
        }
        Ok(Value::String(token.to_string()))
    }

    fn parse_number(&self, token: &str) -> Option<serde_json::Number> {
        parse_number_token(token)
    }

    fn parse_key_token(&self, token: &str) -> Result<KeyToken> {
        let token = trim_ascii(token);
        if token.starts_with('"') {
            let value = self.parse_quoted(token)?;
            Ok(KeyToken {
                value: SmolStr::new(value.as_str()),
            })
        } else {
            Ok(KeyToken {
                value: SmolStr::new(token),
            })
        }
    }

    fn parse_quoted(&self, token: &str) -> Result<String> {
        let token = trim_ascii(token);
        if token.len() < 2 || !token.starts_with('"') || !token.ends_with('"') {
            return Err(Error::decode("unterminated string"));
        }
        let inner = &token[1..token.len() - 1];
        let bytes = inner.as_bytes();
        if memchr(b'\\', bytes).is_none() {
            if memchr(b'"', bytes).is_some() {
                return Err(Error::decode("characters after quoted token"));
            }
            if inner.chars().any(|ch| ch < ' ' && ch != '\t') {
                return Err(Error::decode("unescaped control character"));
            }
            return Ok(inner.to_string());
        }
        let mut out = String::with_capacity(inner.len());
        let mut idx = 0;
        while let Some(offset) = memchr(b'\\', &bytes[idx..]) {
            let esc_pos = idx + offset;
            if memchr(b'"', &bytes[idx..esc_pos]).is_some() {
                return Err(Error::decode("characters after quoted token"));
            }
            if inner[idx..esc_pos].chars().any(|ch| ch < ' ' && ch != '\t') {
                return Err(Error::decode("unescaped control character"));
            }
            out.push_str(&inner[idx..esc_pos]);
            let next_idx = esc_pos + 1;
            let next = bytes
                .get(next_idx)
                .ok_or_else(|| Error::decode("unterminated escape"))?;
            match next {
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'u' => {
                    let hex_end = next_idx + 5;
                    let hex = inner
                        .get(next_idx + 1..hex_end)
                        .ok_or_else(|| Error::decode("unterminated unicode escape"))?;
                    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(Error::decode("invalid unicode escape"));
                    }
                    let code = u16::from_str_radix(hex, 16)
                        .map_err(|_| Error::decode("invalid unicode escape"))?;
                    if (0xd800..=0xdfff).contains(&code) {
                        return Err(Error::decode("unicode surrogate escapes not allowed"));
                    }
                    out.push(
                        char::from_u32(code as u32)
                            .ok_or_else(|| Error::decode("invalid unicode escape"))?,
                    );
                    idx = hex_end;
                    continue;
                }
                _ => return Err(Error::decode("invalid escape")),
            }
            idx = esc_pos + 2;
        }
        if memchr(b'"', &bytes[idx..]).is_some() {
            return Err(Error::decode("characters after quoted token"));
        }
        if inner[idx..].chars().any(|ch| ch < ' ' && ch != '\t') {
            return Err(Error::decode("unescaped control character"));
        }
        out.push_str(&inner[idx..]);
        Ok(out)
    }

    fn split_key_value<'a>(&self, line: &'a str) -> Result<Option<(&'a str, &'a str)>> {
        let mut in_quotes = false;
        let mut escape = false;
        for (idx, byte) in line.as_bytes().iter().enumerate() {
            if escape {
                escape = false;
                continue;
            }
            if in_quotes {
                if *byte == b'\\' {
                    escape = true;
                    continue;
                }
                if *byte == b'"' {
                    in_quotes = false;
                }
                continue;
            }
            if *byte == b'"' {
                in_quotes = true;
                continue;
            }
            if *byte == b':' {
                return Ok(Some((&line[..idx], &line[idx + 1..])));
            }
        }
        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }
        Ok(None)
    }

    fn parse_array_header(&self, line: &str) -> Result<Option<HeaderLine>> {
        let mut bracket_start = None;
        let mut in_quotes = false;
        let mut escape = false;
        for (idx, ch) in line.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if in_quotes {
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    in_quotes = false;
                }
                continue;
            }
            if ch == '"' {
                in_quotes = true;
                continue;
            }
            if ch == '[' {
                bracket_start = Some(idx);
                break;
            }
            if ch == ':' {
                return Ok(None);
            }
        }
        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }
        let bracket_start = match bracket_start {
            Some(idx) => idx,
            None => return Ok(None),
        };
        let bracket_end = match line[bracket_start + 1..].find(']') {
            Some(idx) => bracket_start + 1 + idx,
            None => return Err(Error::decode("unterminated array header")),
        };

        let raw_key_part = &line[..bracket_start];
        if raw_key_part.ends_with(' ') || raw_key_part.ends_with('\t') {
            if self.strict {
                return Err(Error::decode("whitespace before bracket segment"));
            }
            return Ok(None);
        }
        let key_part = trim_ascii(raw_key_part);
        let key = if key_part.is_empty() {
            None
        } else {
            Some(self.parse_key_token(key_part)?)
        };

        let inner = &line[bracket_start + 1..bracket_end];
        if inner.is_empty() {
            if !self.strict {
                return Ok(None);
            }
            return Err(Error::decode("array length missing"));
        }
        let mut digits_end = 0;
        for (idx, ch) in inner.char_indices() {
            if ch.is_ascii_digit() {
                digits_end = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if digits_end == 0 {
            if !self.strict {
                return Ok(None);
            }
            return Err(Error::decode("array length missing"));
        }
        let len: usize = inner[..digits_end]
            .parse()
            .map_err(|_| Error::decode("invalid array length"))?;
        if digits_end > 1 && inner.starts_with('0') {
            if !self.strict {
                return Ok(None);
            }
            return Err(Error::decode("invalid array length"));
        }
        let mut remainder = &inner[digits_end..];
        let keyed = remainder.starts_with(':');
        if keyed {
            remainder = &remainder[1..];
        }
        let delimiter = match remainder {
            "" => ',',
            "\t" => '\t',
            "|" => '|',
            _ if !self.strict => return Ok(None),
            _ => return Err(Error::decode("invalid array delimiter")),
        };

        let mut rest = &line[bracket_end + 1..];
        if rest.starts_with(char::is_whitespace) {
            if !self.strict {
                return Ok(None);
            }
            return Err(Error::decode("invalid array header suffix"));
        }
        let mut fields = None;
        if rest.starts_with('{') {
            let end =
                matching_brace(rest).ok_or_else(|| Error::decode("unterminated field list"))?;
            let field_segment = &rest[1..end];
            let has_mismatched_delimiter = match delimiter {
                '|' => {
                    first_unquoted(field_segment, b',').is_some()
                        || first_unquoted(field_segment, b'\t').is_some()
                }
                '\t' => {
                    first_unquoted(field_segment, b',').is_some()
                        || first_unquoted(field_segment, b'|').is_some()
                }
                ',' => {
                    first_unquoted(field_segment, b'|').is_some()
                        || first_unquoted(field_segment, b'\t').is_some()
                }
                _ => false,
            };
            if has_mismatched_delimiter {
                if !self.strict {
                    return Ok(None);
                }
                return Err(Error::decode("field delimiter mismatch"));
            }
            fields = Some(self.parse_field_entries(field_segment, delimiter)?);
            rest = &rest[end + 1..];
        }
        if keyed && fields.is_none() {
            return Err(Error::decode("keyed header requires fields"));
        }

        if !rest.starts_with(':') {
            if !self.strict && rest.contains(':') {
                return Ok(None);
            }
            return Err(Error::decode(if rest.contains(':') {
                "invalid array header suffix"
            } else {
                "array header missing ':'"
            }));
        }
        let inline = trim_ascii(&rest[1..]);
        let inline = if inline.is_empty() {
            None
        } else {
            Some(inline.to_string())
        };
        if fields.is_some() && inline.is_some() {
            return Err(Error::decode(
                "fields-bearing header cannot contain inline data",
            ));
        }
        if keyed && inline.is_some() {
            return Err(Error::decode("keyed header cannot contain inline data"));
        }

        Ok(Some(HeaderLine {
            key,
            len,
            delimiter,
            keyed,
            fields,
            inline,
        }))
    }

    fn parse_field_entries(&self, input: &str, delimiter: char) -> Result<Vec<FieldEntry>> {
        let mut result = Vec::new();
        let mut names = HashMap::<SmolStr, ()>::new();
        for token in split_top_level(input, delimiter)? {
            let token = trim_ascii(token);
            if token.is_empty() {
                return Err(Error::decode("empty field name"));
            }
            let (name, children) = if let Some(start) = first_unquoted(token, b'{') {
                let end = matching_brace(&token[start..])
                    .ok_or_else(|| Error::decode("unterminated field list"))?;
                if start + end + 1 != token.len() {
                    return Err(Error::decode("invalid field name"));
                }
                let inner = &token[start + 1..start + end];
                if inner.is_empty() {
                    return Err(Error::decode("empty field name"));
                }
                (
                    trim_ascii(&token[..start]),
                    self.parse_field_entries(inner, delimiter)?,
                )
            } else {
                (token, Vec::new())
            };
            let key = self.parse_key_token(name)?;
            if self.strict && names.insert(key.value.clone(), ()).is_some() {
                return Err(Error::decode("duplicate field name"));
            }
            result.push(FieldEntry { key, children });
        }
        if result.is_empty() {
            return Err(Error::decode("empty field name"));
        }
        Ok(result)
    }

    fn build_line_owned(&self, line: &str, raw_start: usize) -> Result<Line<'static>> {
        let Some((indent_columns, indent_chars, level)) = self.build_line_parts(line)? else {
            return Ok(Line {
                raw_start,
                content_start: raw_start,
                indent: 0,
                level: 0,
                content: Cow::Borrowed(""),
                is_blank: true,
                is_comment: false,
            });
        };
        let content_start = raw_start + indent_chars;
        let content = Cow::Owned(line[indent_chars..].to_string());
        Ok(Line {
            raw_start,
            content_start,
            indent: indent_columns,
            level,
            content,
            is_blank: false,
            is_comment: false,
        })
    }

    fn build_line_parts(&self, line: &str) -> Result<Option<(usize, usize, usize)>> {
        let spaces = line.bytes().take_while(|&byte| byte == b' ').count();
        let possible_tsv = spaces > 0
            && spaces.is_multiple_of(self.indent_size)
            && line.as_bytes().get(spaces) == Some(&b'\t');
        if is_blank_line(line) && !possible_tsv {
            return Ok(None);
        }
        let mut indent_columns: usize = 0;
        let mut indent_chars: usize = 0;
        for &byte in line.as_bytes() {
            match byte {
                b' ' => {
                    indent_columns += 1;
                    indent_chars += 1;
                }
                b'\t' => {
                    // Leave possible empty TSV cells for the scope-aware parser.
                    if indent_columns > 0
                        && indent_columns.is_multiple_of(self.indent_size)
                        && line.as_bytes()[..indent_chars]
                            .iter()
                            .all(|&byte| byte == b' ')
                    {
                        break;
                    }
                    if self.strict {
                        return Err(Error::decode("tabs not allowed in indentation"));
                    }
                    indent_columns = indent_columns.saturating_add(self.indent_size);
                    indent_chars += 1;
                }
                _ => break,
            }
        }
        if self.strict && !indent_columns.is_multiple_of(self.indent_size) {
            return Err(Error::decode("invalid indentation"));
        }
        let level = indent_columns / self.indent_size;
        Ok(Some((indent_columns, indent_chars, level)))
    }

    fn location_for_slice(
        &self,
        line: &Line<'_>,
        line_idx: usize,
        slice: &str,
    ) -> Option<Location> {
        let base = line.content.as_ptr() as usize;
        let slice_ptr = slice.as_ptr() as usize;
        if slice_ptr < base || slice_ptr > base + line.content.len() {
            return None;
        }
        let offset = line.content_start + (slice_ptr - base);
        let column = offset.saturating_sub(line.raw_start) + 1;
        Some(Location {
            offset,
            line: line_idx + 1,
            column,
        })
    }

    fn attach_location_for_slice(
        &self,
        line: &Line<'_>,
        line_idx: usize,
        slice: &str,
        err: Error,
    ) -> Error {
        if err.location.is_some() {
            return err;
        }
        match self.location_for_slice(line, line_idx, slice) {
            Some(location) => err.with_location(location),
            None => {
                let offset = line.raw_start;
                err.with_location(Location {
                    offset,
                    line: line_idx + 1,
                    column: 1,
                })
            }
        }
    }

    fn split_tabular_row_into<'c>(
        &self,
        input: &'c str,
        delimiter: char,
        tokens: &mut TokenBuf<'c>,
    ) -> Result<bool> {
        tokens.clear();
        let bytes = input.as_bytes();
        if input.is_ascii() && !bytes.contains(&b'"') && !bytes.contains(&b'\\') {
            let delim_byte = delimiter as u8;
            let delim_pos = memchr(delim_byte, bytes);
            let colon_pos = memchr(b':', bytes);
            if let Some(colon) = colon_pos {
                if delim_pos.is_none() || delim_pos.is_some_and(|pos| colon < pos) {
                    return Ok(false);
                }
            }
            let mut start = 0;
            for idx in memchr_iter(delim_byte, bytes) {
                let token = trim_ascii(&input[start..idx]);
                tokens.push(token);
                start = idx + 1;
            }
            if start < bytes.len() || input.ends_with(delimiter) {
                let token = trim_ascii(&input[start..]);
                tokens.push(token);
            }
            return Ok(true);
        }

        let mut in_quotes = false;
        let mut escape = false;
        let delim_byte = delimiter as u8;
        let mut start = 0;
        let mut idx = 0;
        let mut saw_delim = false;
        let mut colon_before_delim = false;

        while idx < bytes.len() {
            if escape {
                escape = false;
                idx += 1;
                continue;
            }
            if in_quotes {
                match memchr2(b'\\', b'"', &bytes[idx..]) {
                    Some(offset) => {
                        let pos = idx + offset;
                        match bytes[pos] {
                            b'\\' => {
                                escape = true;
                                idx = pos + 1;
                            }
                            b'"' => {
                                in_quotes = false;
                                idx = pos + 1;
                            }
                            _ => unreachable!("memchr2 returned unexpected byte"),
                        }
                    }
                    None => {
                        idx = bytes.len();
                    }
                }
                continue;
            }

            match memchr3(delim_byte, b'"', b':', &bytes[idx..]) {
                Some(offset) => {
                    let pos = idx + offset;
                    match bytes[pos] {
                        b'"' => {
                            in_quotes = true;
                            idx = pos + 1;
                        }
                        b':' => {
                            if !saw_delim {
                                colon_before_delim = true;
                            }
                            idx = pos + 1;
                        }
                        _ => {
                            let token = trim_ascii(&input[start..pos]);
                            tokens.push(token);
                            start = pos + 1;
                            idx = start;
                            saw_delim = true;
                        }
                    }
                }
                None => break,
            }
        }

        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }
        if colon_before_delim {
            return Ok(false);
        }
        if start < bytes.len() || input.ends_with(delimiter) {
            let token = trim_ascii(&input[start..]);
            tokens.push(token);
        }
        Ok(true)
    }

    fn insert_key_value(
        &self,
        map: &mut Map<String, Value>,
        key: KeyToken,
        value: Value,
    ) -> Result<()> {
        if self.strict && map.contains_key(key.value.as_str()) {
            return Err(Error::decode("duplicate object key"));
        }
        map.insert(key.value.to_string(), value);
        Ok(())
    }

    fn merge_objects_owned(
        &self,
        target: &mut Map<String, Value>,
        source: Map<String, Value>,
    ) -> Result<()> {
        for (key, value) in source {
            match target.get_mut(&key) {
                None => {
                    target.insert(key, value);
                }
                Some(existing) => {
                    if self.strict {
                        return Err(Error::decode("duplicate object key"));
                    }
                    *existing = value;
                }
            }
        }
        Ok(())
    }

    fn decode_tabular_cells(
        &self,
        tokens: &[&str],
        line: &Line<'_>,
        line_idx: usize,
    ) -> Result<Vec<Value>> {
        let mut cells = Vec::new();
        for token in tokens {
            let value = if token.is_empty() {
                Value::String(String::new())
            } else if *token == "[]" {
                Value::String("[]".into())
            } else {
                self.parse_value_token(token)
                    .map_err(|err| self.attach_location_for_slice(line, line_idx, token, err))?
            };
            cells.push(value);
        }
        Ok(cells)
    }

    fn materialize_fields(
        &self,
        fields: &[FieldEntry],
        cells: &[Value],
        cursor: &mut usize,
    ) -> Result<Map<String, Value>> {
        let mut map = Map::new();
        for field in fields {
            let value = if field.children.is_empty() {
                let value = cells.get(*cursor).cloned();
                *cursor = cursor.saturating_add(1);
                value
            } else {
                Some(Value::Object(self.materialize_fields(
                    &field.children,
                    cells,
                    cursor,
                )?))
            };
            if let Some(value) = value {
                self.insert_key_value(&mut map, field.key.clone(), value)?;
            }
        }
        Ok(map)
    }
}

#[derive(Clone)]
struct KeyToken {
    value: SmolStr,
}

struct HeaderLine {
    key: Option<KeyToken>,
    len: usize,
    delimiter: char,
    keyed: bool,
    fields: Option<Vec<FieldEntry>>,
    inline: Option<String>,
}

struct FieldEntry {
    key: KeyToken,
    children: Vec<FieldEntry>,
}

struct ParsedArray {
    value: Value,
}

#[derive(Clone)]
struct Line<'a> {
    raw_start: usize,
    content_start: usize,
    indent: usize,
    level: usize,
    content: Cow<'a, str>,
    is_blank: bool,
    is_comment: bool,
}

struct StreamLine {
    idx: usize,
    line: Line<'static>,
}

struct LineStream<R: BufRead> {
    reader: R,
    buffer: String,
    pending: VecDeque<StreamLine>,
    line_idx: usize,
    offset: usize,
}

impl<R: BufRead> LineStream<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: String::new(),
            pending: VecDeque::new(),
            line_idx: 0,
            offset: 0,
        }
    }

    fn push_back(&mut self, line: StreamLine) {
        self.pending.push_front(line);
    }

    fn next_line(&mut self, decoder: &Decoder) -> Result<Option<StreamLine>> {
        if let Some(line) = self.pending.pop_front() {
            return Ok(Some(line));
        }
        self.buffer.clear();
        let read = self
            .reader
            .read_line(&mut self.buffer)
            .map_err(|err| Error::decode_with_source(format!("read failed: {err}"), err))?;
        if read == 0 {
            return Ok(None);
        }
        let raw_len = self.buffer.len();
        let mut line = self.buffer.as_str();
        if line.ends_with('\n') {
            line = &line[..line.len().saturating_sub(1)];
            if line.ends_with('\r') {
                line = &line[..line.len().saturating_sub(1)];
            }
        } else if line.ends_with('\r') {
            line = &line[..line.len().saturating_sub(1)];
        }
        let raw_start = self.offset;
        let line_idx = self.line_idx;
        self.offset += raw_len;
        self.line_idx += 1;
        if line_idx == 0 {
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        let line = line.trim_end_matches(' ');
        let is_comment = line.trim_start_matches(' ').starts_with('#');
        let line_to_build = if is_comment { "" } else { line };
        let mut built = decoder
            .build_line_owned(line_to_build, raw_start)
            .map_err(|err| {
                err.with_location(Location {
                    offset: raw_start,
                    line: line_idx + 1,
                    column: 1,
                })
            })?;
        if is_comment {
            built.is_blank = true;
            built.is_comment = true;
            built.content = Cow::Borrowed("");
        }
        Ok(Some(StreamLine {
            idx: line_idx,
            line: built,
        }))
    }

    fn next_non_blank(&mut self, decoder: &Decoder) -> Result<Option<StreamLine>> {
        while let Some(line) = self.next_line(decoder)? {
            if line.line.is_blank {
                continue;
            }
            return Ok(Some(line));
        }
        Ok(None)
    }

    fn has_more_non_blank(&mut self, decoder: &Decoder) -> Result<bool> {
        let mut consumed = Vec::new();
        let mut found = false;
        while let Some(line) = self.next_line(decoder)? {
            found = !line.line.is_blank;
            consumed.push(line);
            if found {
                break;
            }
        }
        for line in consumed.into_iter().rev() {
            self.push_back(line);
        }
        Ok(found)
    }

    fn drain_to_end(&mut self, decoder: &Decoder) -> Result<()> {
        while self.next_line(decoder)?.is_some() {}
        Ok(())
    }
}

impl Decoder {
    fn decode_reader_streaming<R: BufRead>(&mut self, reader: R) -> Result<Value> {
        if self.indent_size == 0 {
            return Err(Error::decode("indent size must be greater than zero"));
        }
        let mut stream = LineStream::new(reader);
        let first_non_blank = stream.next_non_blank(self)?;
        let Some(first) = first_non_blank else {
            return Ok(Value::Object(Map::new()));
        };
        let first_content = trim_ascii(&first.line.content);
        if first_content.starts_with('\t') {
            return Err(Error::decode("tabs not allowed in indentation"));
        }
        if first_content == "[]" {
            if self.strict && first.line.indent != 0 {
                return Err(Error::decode("unexpected indentation"));
            }
            if let Some(line) = stream.next_non_blank(self)? {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected trailing content"),
                ));
            }
            return Ok(Value::Array(Vec::new()));
        }
        if first_content.starts_with('[') {
            let header = match self.parse_array_header(first_content) {
                Ok(header) => header,
                Err(err) => {
                    return Err(self.attach_location_for_slice(
                        &first.line,
                        first.idx,
                        first_content,
                        err,
                    ));
                }
            };
            if let Some(header) = header {
                if header.key.is_none() {
                    if first.line.indent != 0 {
                        return Err(self.attach_location_for_line_meta(
                            first.idx,
                            &first.line,
                            Error::decode("unexpected indentation"),
                        ));
                    }
                    let parsed = self
                        .parse_array_from_header_stream(&header, &mut stream, 0)
                        .map_err(|err| {
                            self.attach_location_for_slice(
                                &first.line,
                                first.idx,
                                first_content,
                                err,
                            )
                        })?;
                    while let Some(line) = stream.next_line(self)? {
                        if line.line.is_blank {
                            continue;
                        }
                        return Err(self.attach_location_for_line_meta(
                            line.idx,
                            &line.line,
                            Error::decode("unexpected trailing content"),
                        ));
                    }
                    return Ok(parsed.value);
                }
            }
        }

        if !stream.has_more_non_blank(self)? {
            if self.strict && first.line.indent != 0 {
                return Err(self.attach_location_for_line_meta(
                    first.idx,
                    &first.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let value = self
                .decode_single_line(first_content, &first.line, first.idx)
                .map_err(|err| {
                    self.attach_location_for_slice(&first.line, first.idx, first_content, err)
                })?;
            stream.drain_to_end(self)?;
            return Ok(value);
        }

        stream.push_back(first);
        let map = self.parse_object_block_stream(&mut stream, 0, false)?;
        stream.drain_to_end(self)?;
        Ok(Value::Object(map))
    }

    fn attach_location_for_line_meta(&self, line_idx: usize, line: &Line<'_>, err: Error) -> Error {
        if err.location.is_some() {
            return err;
        }
        let offset = line.raw_start;
        err.with_location(Location {
            offset,
            line: line_idx + 1,
            column: 1,
        })
    }

    fn parse_object_block_stream<R: BufRead>(
        &mut self,
        stream: &mut LineStream<R>,
        base_level: usize,
        reject_internal_blank: bool,
    ) -> Result<Map<String, Value>> {
        let mut map = Map::new();
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_comment {
                continue;
            }
            if line.line.is_blank {
                if self.strict && reject_internal_blank {
                    let next = stream.next_non_blank(self)?;
                    if let Some(next_line) = next {
                        let next_level = next_line.line.level;
                        stream.push_back(next_line);
                        if next_level >= base_level {
                            return Err(self.attach_location_for_line_meta(
                                line.idx,
                                &line.line,
                                Error::decode("blank line not allowed inside list item"),
                            ));
                        }
                        stream.push_back(line);
                    }
                    break;
                }
                continue;
            }
            let level = line.line.level;
            if level < base_level {
                stream.push_back(line);
                break;
            }
            if level > base_level {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(&line.line.content);
            if content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }
            let header = match self.parse_array_header(content) {
                Ok(header) => header,
                Err(err) => {
                    return Err(self.attach_location_for_slice(&line.line, line.idx, content, err));
                }
            };
            if let Some(header) = header {
                let key = header.key.as_ref().ok_or_else(|| {
                    self.attach_location_for_slice(
                        &line.line,
                        line.idx,
                        content,
                        Error::decode("array header missing key in object context"),
                    )
                })?;
                let parsed = self
                    .parse_array_from_header_stream(&header, stream, base_level)
                    .map_err(|err| {
                        self.attach_location_for_slice(&line.line, line.idx, content, err)
                    })?;
                self.insert_key_value(&mut map, key.clone(), parsed.value)
                    .map_err(|err| {
                        self.attach_location_for_slice(&line.line, line.idx, content, err)
                    })?;
                continue;
            }

            if let Some((key, value)) = self
                .split_key_value(content)
                .map_err(|err| self.attach_location_for_slice(&line.line, line.idx, content, err))?
            {
                let key = self.parse_key_token(trim_ascii(key)).map_err(|err| {
                    self.attach_location_for_slice(&line.line, line.idx, key, err)
                })?;
                if trim_ascii(value).is_empty() {
                    let nested = self
                        .parse_object_block_stream(stream, base_level + 1, reject_internal_blank)
                        .map_err(|err| {
                            self.attach_location_for_slice(&line.line, line.idx, content, err)
                        })?;
                    self.insert_key_value(&mut map, key, Value::Object(nested))
                        .map_err(|err| {
                            self.attach_location_for_slice(&line.line, line.idx, content, err)
                        })?;
                } else {
                    let value_trimmed = trim_ascii(value);
                    let value = self.parse_value_token(value).map_err(|err| {
                        self.attach_location_for_slice(&line.line, line.idx, value_trimmed, err)
                    })?;
                    self.insert_key_value(&mut map, key, value).map_err(|err| {
                        self.attach_location_for_slice(&line.line, line.idx, content, err)
                    })?;
                }
                continue;
            }

            return Err(self.attach_location_for_slice(
                &line.line,
                line.idx,
                content,
                Error::decode("scalar line not allowed in object scope"),
            ));
        }
        Ok(map)
    }

    fn parse_array_from_header_stream<R: BufRead>(
        &mut self,
        header: &HeaderLine,
        stream: &mut LineStream<R>,
        base_level: usize,
    ) -> Result<ParsedArray> {
        self.push_delimiter(header.delimiter);
        self.header_depths.push(base_level);
        let result = (|| {
            if header.keyed {
                let fields = header
                    .fields
                    .as_ref()
                    .ok_or_else(|| Error::decode("keyed header requires fields"))?;
                let map = self.parse_keyed_block_stream(
                    stream,
                    base_level,
                    fields,
                    header.delimiter,
                    header.len,
                )?;
                return Ok(ParsedArray {
                    value: Value::Object(map),
                });
            }
            if let Some(inline) = header.inline.as_deref() {
                let items = self.parse_inline_array(inline, header.delimiter, header.len)?;
                if self.strict && items.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    value: Value::Array(items),
                });
            }

            if let Some(fields) = header.fields.as_ref() {
                let rows = self.parse_tabular_block_stream(
                    stream,
                    base_level,
                    fields,
                    header.delimiter,
                    header.len,
                )?;
                if self.strict && rows.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    value: Value::Array(rows),
                });
            }

            if header.len == 0 && self.strict {
                return Ok(ParsedArray {
                    value: Value::Array(Vec::new()),
                });
            }

            let items = self.parse_list_block_stream(stream, base_level + 1, header.len)?;
            if self.strict && header.len > 0 && items.is_empty() {
                return Err(Error::decode("array payload required"));
            }
            if self.strict && items.len() != header.len {
                return Err(Error::decode("array length mismatch"));
            }
            Ok(ParsedArray {
                value: Value::Array(items),
            })
        })();
        self.pop_delimiter();
        self.header_depths.pop();
        result
    }

    fn check_leading_blank<R: BufRead>(&self, stream: &mut LineStream<R>) -> Result<()> {
        // Entering a nested header already started every ancestor's payload.
        // Its leading blanks are therefore internal blanks in those spans.
        if self.strict && self.header_depths.len() > 1 {
            if let Some(next) = stream.next_non_blank(self)? {
                let inside_ancestor = self.header_depths[..self.header_depths.len() - 1]
                    .iter()
                    .any(|&depth| next.line.level > depth);
                stream.push_back(next);
                if inside_ancestor {
                    return Err(Error::decode("blank line not allowed in array"));
                }
            }
        }
        Ok(())
    }

    fn parse_keyed_block_stream<R: BufRead>(
        &self,
        stream: &mut LineStream<R>,
        base_level: usize,
        fields: &[FieldEntry],
        delimiter: char,
        expected_len: usize,
    ) -> Result<Map<String, Value>> {
        let leaf_count = count_field_leaves(fields);
        let mut map = Map::new();
        let mut entries = 0;
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_comment {
                continue;
            }
            if line.line.is_blank {
                if !self.strict || entries == 0 {
                    self.check_leading_blank(stream)?;
                    continue;
                }
                let next = stream.next_non_blank(self)?;
                if let Some(next_line) = next {
                    let next_level = next_line.line.level;
                    stream.push_back(next_line);
                    if next_level <= base_level {
                        stream.push_back(line);
                        break;
                    }
                    return Err(self.attach_location_for_line_meta(
                        line.idx,
                        &line.line,
                        Error::decode("blank line not allowed in array"),
                    ));
                }
                break;
            }
            if line.line.level <= base_level {
                stream.push_back(line);
                break;
            }
            if line.line.level != base_level + 1 {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(&line.line.content);
            if content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }
            let Some((key, cells)) = self.split_key_value(content)? else {
                if self.strict {
                    return Err(self.attach_location_for_slice(
                        &line.line,
                        line.idx,
                        content,
                        Error::decode("entry row missing ':'"),
                    ));
                }
                continue;
            };
            let key = self.parse_key_token(key)?;
            let cells = trim_ascii(cells);
            let tokens = if cells.is_empty() {
                TokenBuf::new()
            } else {
                self.split_delimited(cells, delimiter)?
            };
            if self.strict && tokens.len() != leaf_count {
                return Err(self.attach_location_for_slice(
                    &line.line,
                    line.idx,
                    content,
                    Error::decode("tabular row field count mismatch"),
                ));
            }
            let cells = self.decode_tabular_cells(&tokens, &line.line, line.idx)?;
            let mut cursor = 0;
            let value = self.materialize_fields(fields, &cells, &mut cursor)?;
            self.insert_key_value(&mut map, key, Value::Object(value))?;
            entries += 1;
        }
        if self.strict && entries != expected_len {
            return Err(Error::decode("array length mismatch"));
        }
        Ok(map)
    }

    fn parse_tabular_block_stream<R: BufRead>(
        &self,
        stream: &mut LineStream<R>,
        base_level: usize,
        fields: &[FieldEntry],
        delimiter: char,
        _expected_len: usize,
    ) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        let leaf_count = count_field_leaves(fields);
        let mut row_level: Option<usize> = None;
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_comment {
                continue;
            }
            if line.line.is_blank {
                if !self.strict {
                    continue;
                }
                if rows.is_empty() {
                    self.check_leading_blank(stream)?;
                    continue;
                }
                let next = stream.next_non_blank(self)?;
                if let Some(next_line) = next {
                    let next_level = next_line.line.level;
                    stream.push_back(next_line);
                    if next_level <= base_level {
                        stream.push_back(line);
                        return Ok(rows);
                    }
                    return Err(self.attach_location_for_line_meta(
                        line.idx,
                        &line.line,
                        Error::decode("blank line not allowed in array"),
                    ));
                }
                return Ok(rows);
            }
            let level = line.line.level;
            if row_level.is_none() {
                if level <= base_level {
                    stream.push_back(line);
                    return Ok(rows);
                }
                if self.strict && level != base_level + 1 {
                    return Err(self.attach_location_for_line_meta(
                        line.idx,
                        &line.line,
                        Error::decode("unexpected indentation"),
                    ));
                }
                row_level = Some(level);
            }
            let row_level = row_level.unwrap();
            if level < row_level {
                stream.push_back(line);
                return Ok(rows);
            }
            if level > row_level {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let row_content = trim_ascii(&line.line.content);
            if delimiter != '\t' && row_content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }
            let mut tokens = TokenBuf::new();
            if !self
                .split_tabular_row_into(row_content, delimiter, &mut tokens)
                .map_err(|err| {
                    self.attach_location_for_slice(&line.line, line.idx, row_content, err)
                })?
            {
                stream.push_back(StreamLine {
                    idx: line.idx,
                    line: line.line.clone(),
                });
                return Ok(rows);
            }
            if tokens.len() != leaf_count && self.strict {
                return Err(self.attach_location_for_slice(
                    &line.line,
                    line.idx,
                    row_content,
                    Error::decode("tabular row field count mismatch"),
                ));
            }
            let cells = self.decode_tabular_cells(&tokens, &line.line, line.idx)?;
            let mut cursor = 0;
            let obj = self.materialize_fields(fields, &cells, &mut cursor)?;
            rows.push(Value::Object(obj));
        }
        Ok(rows)
    }

    fn parse_list_block_stream<R: BufRead>(
        &mut self,
        stream: &mut LineStream<R>,
        item_level: usize,
        _expected_len: usize,
    ) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_comment {
                continue;
            }
            if line.line.is_blank {
                if !self.strict {
                    continue;
                }
                if items.is_empty() {
                    self.check_leading_blank(stream)?;
                    continue;
                }
                let next = stream.next_non_blank(self)?;
                if let Some(next_line) = next {
                    let next_level = next_line.line.level;
                    stream.push_back(next_line);
                    if next_level < item_level {
                        stream.push_back(line);
                        return Ok(items);
                    }
                    return Err(self.attach_location_for_line_meta(
                        line.idx,
                        &line.line,
                        Error::decode("blank line not allowed in array"),
                    ));
                }
                return Ok(items);
            }
            let level = line.line.level;
            if level < item_level {
                stream.push_back(line);
                break;
            }
            if level > item_level {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(&line.line.content);
            if content != "-" && !content.starts_with("- ") {
                return Err(self.attach_location_for_slice(
                    &line.line,
                    line.idx,
                    content,
                    Error::decode("expected list item"),
                ));
            }
            let item_content = if content == "-" {
                ""
            } else {
                trim_ascii(&content[2..])
            };
            let item = self
                .parse_list_item_stream(item_content, stream, item_level, &line.line, line.idx)
                .map_err(|err| {
                    self.attach_location_for_slice(&line.line, line.idx, item_content, err)
                })?;
            items.push(item);
        }
        Ok(items)
    }

    fn parse_list_item_stream<R: BufRead>(
        &mut self,
        item_content: &str,
        stream: &mut LineStream<R>,
        item_level: usize,
        line_meta: &Line<'_>,
        line_idx: usize,
    ) -> Result<Value> {
        if item_content.is_empty() {
            return Ok(Value::Object(Map::new()));
        }
        if item_content == "[]" {
            return Ok(Value::Array(Vec::new()));
        }

        let header = match self.parse_array_header(item_content) {
            Ok(header) => header,
            Err(err) => {
                return Err(self.attach_location_for_slice(line_meta, line_idx, item_content, err));
            }
        };
        if let Some(header) = header {
            if header.key.is_none() {
                if header.fields.is_some() {
                    return Err(self.attach_location_for_slice(
                        line_meta,
                        line_idx,
                        item_content,
                        Error::decode("keyless fields-bearing header not allowed as list item"),
                    ));
                }
                let parsed = self
                    .parse_array_from_header_stream(&header, stream, item_level)
                    .map_err(|err| {
                        self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                    })?;
                return Ok(parsed.value);
            }
            let key = header.key.clone().ok_or_else(|| {
                self.attach_location_for_slice(
                    line_meta,
                    line_idx,
                    item_content,
                    Error::decode("array header missing key in object context"),
                )
            })?;
            let array_base_level = item_level + 1;
            let parsed = self
                .parse_array_from_header_stream(&header, stream, array_base_level)
                .map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                })?;
            let mut map = Map::new();
            self.insert_key_value(&mut map, key, parsed.value)
                .map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                })?;
            let extra = self
                .parse_object_block_stream(stream, item_level + 1, true)
                .map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                })?;
            self.merge_objects_owned(&mut map, extra).map_err(|err| {
                self.attach_location_for_slice(line_meta, line_idx, item_content, err)
            })?;
            return Ok(Value::Object(map));
        }

        if self
            .split_key_value(item_content)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, item_content, err))?
            .is_some()
        {
            return self
                .parse_object_item_from_line_stream(
                    item_content,
                    stream,
                    item_level,
                    line_meta,
                    line_idx,
                )
                .map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                });
        }

        let value = self.parse_value_token(item_content).map_err(|err| {
            self.attach_location_for_slice(line_meta, line_idx, item_content, err)
        })?;
        Ok(value)
    }

    fn parse_object_item_from_line_stream<R: BufRead>(
        &mut self,
        item_content: &str,
        stream: &mut LineStream<R>,
        item_level: usize,
        line_meta: &Line<'_>,
        line_idx: usize,
    ) -> Result<Value> {
        let base_level = item_level + 1;
        let base_ptr = line_meta.content.as_ptr() as usize;
        let slice_ptr = item_content.as_ptr() as usize;
        let item_offset =
            if slice_ptr >= base_ptr && slice_ptr <= base_ptr + line_meta.content.len() {
                line_meta.content_start + (slice_ptr - base_ptr)
            } else {
                line_meta.raw_start
            };
        let synthetic = Line {
            raw_start: item_offset,
            content_start: item_offset,
            indent: base_level * self.indent_size,
            level: base_level,
            content: Cow::Owned(item_content.to_string()),
            is_blank: false,
            is_comment: false,
        };
        stream.push_back(StreamLine {
            idx: line_idx,
            line: synthetic,
        });
        let map = self.parse_object_block_stream(stream, base_level, true)?;
        Ok(Value::Object(map))
    }
}

fn count_field_leaves(fields: &[FieldEntry]) -> usize {
    fields
        .iter()
        .map(|field| {
            if field.children.is_empty() {
                1
            } else {
                count_field_leaves(&field.children)
            }
        })
        .sum()
}

fn first_unquoted(input: &str, needle: u8) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (idx, &byte) in input.as_bytes().iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
            continue;
        }
        if !quoted && byte == needle {
            return Some(idx);
        }
    }
    None
}

fn matching_brace(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (idx, &byte) in input.as_bytes().iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(input: &str, delimiter: char) -> Result<Vec<&str>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::decode("unmatched field brace"))?
            }
            _ if ch == delimiter && depth == 0 => {
                result.push(&input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    if quoted {
        return Err(Error::decode("unterminated string"));
    }
    if depth != 0 {
        return Err(Error::decode("unterminated field list"));
    }
    result.push(&input[start..]);
    Ok(result)
}

pub(super) fn parse_number_token(token: &str) -> Option<serde_json::Number> {
    if !matches_number_grammar(token) {
        return None;
    }
    if is_int_with_leading_zero(token) {
        return None;
    }
    if token == "-0" {
        return serde_json::Number::from_f64(0.0);
    }
    let has_float = token
        .as_bytes()
        .iter()
        .any(|byte| matches!(byte, b'.' | b'e' | b'E'));
    if !has_float {
        if let Ok(value) = token.parse::<i64>() {
            return Some(serde_json::Number::from(value));
        }
        if let Ok(value) = token.parse::<u64>() {
            return Some(serde_json::Number::from(value));
        }
    }
    token
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
}

fn matches_number_grammar(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    let integer_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index == integer_start {
        return false;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len()
}

pub(super) fn is_int_with_leading_zero(token: &str) -> bool {
    let mut chars = token.chars();
    let first = chars.next();
    let rest = if first == Some('-') {
        chars.as_str()
    } else {
        token
    };
    if rest.contains('.') || rest.contains('e') || rest.contains('E') {
        return false;
    }
    rest.len() > 1 && rest.starts_with('0')
}

pub(super) fn trim_ascii(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start] == b' ' {
        start += 1;
    }
    while end > start && bytes[end - 1] == b' ' {
        end -= 1;
    }
    &input[start..end]
}

pub(super) fn is_blank_line(line: &str) -> bool {
    line.as_bytes()
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t'))
}
