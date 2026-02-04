mod parser;
mod pool;
mod scan;
mod serde;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};

use ::serde::de::DeserializeOwned;
use memchr::{memchr, memchr2, memchr3, memchr_iter};
use serde_json::{Map, Value};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::arena::{ArenaView, NodeData, NodeKind};
use crate::num::number::format_json_number;
use crate::text::string::{is_canonical_unquoted_key, is_identifier_segment};
use crate::{DecodeOptions, Error, ExpandPaths, Indent, Location, Result};

#[cfg(feature = "parallel")]
use ::serde::Deserialize;
#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[cfg(feature = "parallel")]
const PARALLEL_ARRAY_MIN_ITEMS: usize = 64;

pub fn from_str<T: DeserializeOwned>(input: &str, options: &DecodeOptions) -> Result<T> {
    if options.expand_paths != ExpandPaths::Off {
        let mut decoder = Decoder::new(options);
        let value = decoder.decode_document(input)?;
        return serde_json::from_value(value).map_err(|err| {
            Error::deserialize_with_source(format!("deserialize failed: {err}"), err)
        });
    }
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
    if options.expand_paths != ExpandPaths::Off {
        let mut decoder = Decoder::new(options);
        return decoder.decode_document(input);
    }
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
    if options.expand_paths != ExpandPaths::Off {
        return from_str::<Vec<T>>(input, options);
    }
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
    if options.expand_paths != ExpandPaths::Off {
        let mut validator = Decoder::new_validator(options);
        return validator.validate_document(input);
    }
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
    expand_paths: ExpandPaths,
    validate: bool,
    active_delimiter: char,
    delimiter_stack: Vec<char>,
}

type TokenBuf<'a> = SmallVec<[&'a str; 16]>;

impl Decoder {
    fn new(options: &DecodeOptions) -> Self {
        let Indent::Spaces(indent_size) = options.indent;
        Self {
            indent_size,
            strict: options.strict,
            expand_paths: options.expand_paths,
            validate: false,
            active_delimiter: ',',
            delimiter_stack: Vec::new(),
        }
    }

    fn new_validator(options: &DecodeOptions) -> Self {
        let mut decoder = Self::new(options);
        decoder.validate = true;
        decoder
    }

    fn validate_document(&mut self, input: &str) -> Result<()> {
        self.decode_document(input).map(|_| ())
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

    fn decode_document(&mut self, input: &str) -> Result<Value> {
        let lines = self.collect_lines(input)?;

        let non_blank: Vec<&Line> = lines.iter().filter(|line| !line.is_blank).collect();

        if non_blank.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let first_non_blank_idx = lines.iter().position(|line| !line.is_blank).unwrap_or(0);
        let first_line = &lines[first_non_blank_idx];
        let first_content = trim_ascii(&first_line.content);
        if first_content.starts_with('[') {
            let header = match self.parse_array_header(first_content) {
                Ok(header) => header,
                Err(err) => {
                    return Err(self.attach_location_for_slice(
                        first_line,
                        first_non_blank_idx,
                        first_content,
                        err,
                    ));
                }
            };
            if let Some(header) = header {
                if header.key.is_none() {
                    if first_line.indent != 0 {
                        return Err(self.attach_location_for_line(
                            &lines,
                            first_non_blank_idx,
                            Error::decode("unexpected indentation"),
                        ));
                    }
                    let parsed = self
                        .parse_array_from_header(&header, &lines, first_non_blank_idx + 1, 0)
                        .map_err(|err| {
                            self.attach_location_for_slice(
                                first_line,
                                first_non_blank_idx,
                                first_content,
                                err,
                            )
                        })?;
                    self.ensure_no_trailing_content(&lines, parsed.next_idx)?;
                    return Ok(parsed.value);
                }
            }
        }

        if non_blank.len() == 1 && non_blank[0].indent == 0 {
            let content = trim_ascii(&non_blank[0].content);
            if self.validate && self.reject_root_unquoted_string(content) {
                return Err(self.attach_location_for_slice(
                    first_line,
                    first_non_blank_idx,
                    content,
                    Error::decode("root string must be quoted"),
                ));
            }
            return self
                .decode_single_line(content, first_line, first_non_blank_idx)
                .map_err(|err| {
                    self.attach_location_for_slice(first_line, first_non_blank_idx, content, err)
                });
        }

        if non_blank.len() == 1 && self.strict && non_blank[0].indent != 0 {
            return Err(self.attach_location_for_line(
                &lines,
                first_non_blank_idx,
                Error::decode("unexpected indentation"),
            ));
        }

        let map = self.decode_object_lines(&lines)?;
        Ok(Value::Object(map))
    }

    fn ensure_no_trailing_content(&self, lines: &[Line], start_idx: usize) -> Result<()> {
        if let Some((idx, _)) = lines
            .iter()
            .enumerate()
            .skip(start_idx)
            .find(|(_, line)| !line.is_blank)
        {
            return Err(self.attach_location_for_line(
                lines,
                idx,
                Error::decode("unexpected trailing content"),
            ));
        }
        Ok(())
    }

    fn decode_single_line(
        &mut self,
        line: &str,
        line_meta: &Line,
        line_idx: usize,
    ) -> Result<Value> {
        if let Some(array) = self
            .parse_array_line(line)
            .map_err(|err| self.attach_location_for_slice(line_meta, line_idx, line, err))?
        {
            return Ok(array);
        }
        if let (Some(bracket_idx), Some(colon_idx)) = (line.find('['), line.find(':')) {
            if bracket_idx < colon_idx {
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
        if self.strict && line.is_ascii() && !line.starts_with('"') && contains_whitespace(line) {
            return Err(self.attach_location_for_slice(
                line_meta,
                line_idx,
                line,
                Error::decode("unquoted primitive contains whitespace"),
            ));
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
        let items = match header.inline.as_deref() {
            Some(inline) => self.parse_inline_array(inline, header.delimiter, header.len)?,
            None => Vec::new(),
        };
        if header.inline.is_none() && header.len > 0 {
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
        let mut values = Vec::with_capacity(expected_len.max(tokens.len()));
        for token in tokens {
            if token.is_empty() {
                values.push(Value::String(String::new()));
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
        expected_len: usize,
    ) -> Result<TokenBuf<'a>> {
        let mut tokens = if expected_len > 0 {
            TokenBuf::with_capacity(expected_len)
        } else {
            TokenBuf::new()
        };
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
        if self.validate {
            self.validate_value_token(token)?;
        }
        let token = trim_ascii(token);
        if token.is_empty() {
            return Err(Error::decode("empty value"));
        }
        if token.starts_with('"') {
            return Ok(Value::String(self.parse_quoted(token)?));
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

    fn validate_value_token(&self, token: &str) -> Result<()> {
        let token = trim_ascii(token);
        if token.is_empty() {
            return Err(Error::decode("empty value"));
        }
        if token.starts_with('"') {
            self.parse_quoted(token)?;
            return Ok(());
        }
        match token {
            "true" | "false" | "null" => return Ok(()),
            "NaN" | "Infinity" | "-Infinity" | "+Infinity" => {
                return Err(Error::decode("non-finite numbers must be null"))
            }
            _ => {}
        }
        if is_numeric_like(token) {
            let number = self
                .parse_number(token)
                .ok_or_else(|| Error::decode("invalid number"))?;
            let canonical = format_json_number(&number);
            if canonical != token {
                return Err(Error::decode("non-canonical number"));
            }
        }
        Ok(())
    }

    fn reject_root_unquoted_string(&self, token: &str) -> bool {
        if token.starts_with('"') {
            return false;
        }
        if matches!(token, "true" | "false" | "null") {
            return false;
        }
        if is_numeric_like(token) {
            return false;
        }
        token.is_ascii() && is_canonical_unquoted_key(token)
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
                quoted: true,
            })
        } else {
            if self.strict {
                if contains_whitespace(token) {
                    return Err(Error::decode("invalid unquoted key"));
                }
                if token.is_ascii() && !is_canonical_unquoted_key(token) {
                    return Err(Error::decode("invalid unquoted key"));
                }
            }
            Ok(KeyToken {
                value: SmolStr::new(token),
                quoted: false,
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
            return Ok(inner.to_string());
        }
        let mut out = String::with_capacity(inner.len());
        let mut idx = 0;
        while let Some(offset) = memchr(b'\\', &bytes[idx..]) {
            let esc_pos = idx + offset;
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
                _ => return Err(Error::decode("invalid escape")),
            }
            idx = esc_pos + 2;
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

        let key_part = trim_ascii(&line[..bracket_start]);
        let key = if key_part.is_empty() {
            None
        } else {
            Some(self.parse_key_token(key_part)?)
        };

        let inner = line[bracket_start + 1..bracket_end].trim_matches(' ');
        if inner.is_empty() {
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
            return Err(Error::decode("array length missing"));
        }
        let len: usize = inner[..digits_end]
            .parse()
            .map_err(|_| Error::decode("invalid array length"))?;
        let remainder = &inner[digits_end..];
        let mut chars = remainder.chars().peekable();
        while matches!(chars.peek(), Some(' ')) {
            chars.next();
        }
        let delimiter = match chars.next() {
            None => ',',
            Some(delimiter) => {
                if chars.any(|ch| ch != ' ') {
                    return Err(Error::decode("invalid array delimiter"));
                }
                if !matches!(delimiter, ',' | '\t' | '|') {
                    return Err(Error::decode("invalid array delimiter"));
                }
                delimiter
            }
        };

        let mut rest = line[bracket_end + 1..].trim_start();
        let mut fields = None;
        if rest.starts_with('{') {
            let end = rest
                .find('}')
                .ok_or_else(|| Error::decode("unterminated field list"))?;
            let field_segment = &rest[1..end];
            let mut parsed_fields = Vec::new();
            for token in self.split_delimited(field_segment, delimiter)? {
                if token.is_empty() {
                    return Err(Error::decode("empty field name"));
                }
                parsed_fields.push(self.parse_key_token(token)?);
            }
            fields = Some(parsed_fields);
            rest = rest[end + 1..].trim_start();
        }

        let colon_idx = rest
            .find(':')
            .ok_or_else(|| Error::decode("array header missing ':'"))?;
        if !trim_ascii(&rest[..colon_idx]).is_empty() {
            return Err(Error::decode("invalid array header suffix"));
        }
        let inline = trim_ascii(&rest[colon_idx + 1..]);
        let inline = if inline.is_empty() {
            None
        } else {
            Some(inline.to_string())
        };

        Ok(Some(HeaderLine {
            key,
            len,
            delimiter,
            fields,
            inline,
        }))
    }

    fn collect_lines(&self, input: &str) -> Result<Vec<Line>> {
        if self.indent_size == 0 {
            return Err(Error::decode("indent size must be greater than zero"));
        }
        let bytes = input.as_bytes();
        if self.validate && bytes.last() == Some(&b'\n') {
            return Err(Error::decode("trailing newline not allowed"));
        }

        let mut lines = Vec::new();
        let mut start = 0;
        for idx in memchr_iter(b'\n', bytes) {
            let mut end = idx;
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1;
            }
            if self.validate && end > start {
                let last = bytes[end - 1];
                if last == b' ' || last == b'\t' {
                    return Err(Error::decode("trailing whitespace not allowed"));
                }
            }
            let line_idx = lines.len();
            let line = &input[start..end];
            let built = self.build_line(line, start).map_err(|err| {
                err.with_location(Location {
                    offset: start,
                    line: line_idx + 1,
                    column: 1,
                })
            })?;
            lines.push(built);
            start = idx + 1;
        }

        let mut end = bytes.len();
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        if self.validate && end > start {
            let last = bytes[end - 1];
            if last == b' ' || last == b'\t' {
                return Err(Error::decode("trailing whitespace not allowed"));
            }
        }
        let line_idx = lines.len();
        let line = &input[start..end];
        let built = self.build_line(line, start).map_err(|err| {
            err.with_location(Location {
                offset: start,
                line: line_idx + 1,
                column: 1,
            })
        })?;
        lines.push(built);

        Ok(lines)
    }

    fn build_line(&self, line: &str, raw_start: usize) -> Result<Line> {
        if is_blank_line(line) {
            return Ok(Line {
                raw_start,
                content_start: raw_start,
                indent: 0,
                level: 0,
                content: String::new(),
                is_blank: true,
            });
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
        let content_start = raw_start + indent_chars;
        let content = line[indent_chars..].to_string();
        Ok(Line {
            raw_start,
            content_start,
            indent: indent_columns,
            level,
            content,
            is_blank: false,
        })
    }

    fn location_for_line(&self, lines: &[Line], line_idx: usize) -> Option<Location> {
        let line = lines.get(line_idx)?;
        let offset = line.raw_start;
        Some(Location {
            offset,
            line: line_idx + 1,
            column: 1,
        })
    }

    fn location_for_slice(&self, line: &Line, line_idx: usize, slice: &str) -> Option<Location> {
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

    fn attach_location_for_line(&self, lines: &[Line], line_idx: usize, err: Error) -> Error {
        if err.location.is_some() {
            return err;
        }
        match self.location_for_line(lines, line_idx) {
            Some(location) => err.with_location(location),
            None => err,
        }
    }

    fn attach_location_for_slice(
        &self,
        line: &Line,
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

    fn decode_object_lines(&mut self, lines: &[Line]) -> Result<Map<String, Value>> {
        let (map, idx) = self.parse_object_block(lines, 0, 0)?;
        if idx < lines.len() {
            return Err(self.attach_location_for_line(
                lines,
                idx,
                Error::decode("unexpected trailing content"),
            ));
        }
        Ok(map)
    }

    fn parse_object_block(
        &mut self,
        lines: &[Line],
        mut idx: usize,
        base_level: usize,
    ) -> Result<(Map<String, Value>, usize)> {
        let mut map = Map::new();
        let mut override_level: Option<usize> = None;
        while idx < lines.len() {
            let line = &lines[idx];
            if line.is_blank {
                idx += 1;
                continue;
            }
            let actual_level = line.level;
            let level = override_level.take().unwrap_or(actual_level);
            if level < base_level {
                break;
            }
            if level > base_level {
                return Err(self.attach_location_for_line(
                    lines,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(&line.content);

            let header = match self.parse_array_header(content) {
                Ok(header) => header,
                Err(err) => {
                    return Err(self.attach_location_for_slice(line, idx, content, err));
                }
            };
            if let Some(header) = header {
                let key = header.key.as_ref().ok_or_else(|| {
                    self.attach_location_for_slice(
                        line,
                        idx,
                        content,
                        Error::decode("array header missing key in object context"),
                    )
                })?;
                let parsed = self
                    .parse_array_from_header(&header, lines, idx + 1, base_level)
                    .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
                self.insert_key_value(&mut map, key.clone(), parsed.value)
                    .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
                if parsed.deindent_next {
                    override_level = Some(base_level);
                }
                idx = parsed.next_idx;
                continue;
            }

            if let Some((key, value)) = self
                .split_key_value(content)
                .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?
            {
                let key = self
                    .parse_key_token(trim_ascii(key))
                    .map_err(|err| self.attach_location_for_slice(line, idx, key, err))?;
                if trim_ascii(value).is_empty() {
                    let (nested, next_idx) = self
                        .parse_object_block(lines, idx + 1, base_level + 1)
                        .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
                    self.insert_key_value(&mut map, key, Value::Object(nested))
                        .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
                    idx = next_idx;
                } else {
                    let value_trimmed = trim_ascii(value);
                    let value = self.parse_value_token(value).map_err(|err| {
                        self.attach_location_for_slice(line, idx, value_trimmed, err)
                    })?;
                    self.insert_key_value(&mut map, key, value)
                        .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
                    idx += 1;
                }
                continue;
            }

            if self.strict {
                return Err(self.attach_location_for_slice(
                    line,
                    idx,
                    content,
                    Error::decode("bare key not allowed in strict mode"),
                ));
            }
            let key = self
                .parse_key_token(content)
                .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
            self.insert_key_value(&mut map, key, Value::Null)
                .map_err(|err| self.attach_location_for_slice(line, idx, content, err))?;
            idx += 1;
        }
        Ok((map, idx))
    }

    fn parse_array_from_header(
        &mut self,
        header: &HeaderLine,
        lines: &[Line],
        idx: usize,
        base_level: usize,
    ) -> Result<ParsedArray> {
        self.push_delimiter(header.delimiter);
        let result = (|| {
            if let Some(inline) = header.inline.as_deref() {
                let items = self.parse_inline_array(inline, header.delimiter, header.len)?;
                if self.strict && items.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    value: Value::Array(items),
                    next_idx: idx,
                    deindent_next: false,
                });
            }

            if let Some(fields) = header.fields.as_ref() {
                let (rows, next_idx, deindent_next) = self.parse_tabular_block(
                    lines,
                    idx,
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
                    next_idx,
                    deindent_next,
                });
            }

            if header.len == 0 {
                return Ok(ParsedArray {
                    value: Value::Array(Vec::new()),
                    next_idx: idx,
                    deindent_next: false,
                });
            }

            let (items, next_idx) =
                self.parse_list_block(lines, idx, base_level + 1, header.len)?;
            if header.len > 0 && items.is_empty() {
                return Err(Error::decode("array payload required"));
            }
            if self.strict && items.len() != header.len {
                return Err(Error::decode("array length mismatch"));
            }
            Ok(ParsedArray {
                value: Value::Array(items),
                next_idx,
                deindent_next: false,
            })
        })();
        self.pop_delimiter();
        result
    }

    fn parse_tabular_block(
        &self,
        lines: &[Line],
        mut idx: usize,
        base_level: usize,
        fields: &[KeyToken],
        delimiter: char,
        expected_len: usize,
    ) -> Result<(Vec<Value>, usize, bool)> {
        let mut rows = Vec::with_capacity(expected_len);
        let mut field_paths: Vec<Option<Vec<&str>>> = Vec::new();
        let mut fast_path = self.expand_paths != ExpandPaths::Safe;
        if !fast_path {
            field_paths = Vec::with_capacity(fields.len());
            for field in fields {
                field_paths.push(self.expandable_path_parts(field));
            }
            fast_path = field_paths.iter().all(|parts| parts.is_none());
        }
        let field_names: Vec<String> = fields.iter().map(|field| field.value.to_string()).collect();
        let mut row_level = None;
        while idx < lines.len() {
            let line = &lines[idx];
            if line.is_blank {
                if !self.strict {
                    idx += 1;
                    continue;
                }
                let mut peek = idx + 1;
                while peek < lines.len() && lines[peek].is_blank {
                    peek += 1;
                }
                if peek >= lines.len() || lines[peek].level <= base_level {
                    break;
                }
                return Err(self.attach_location_for_line(
                    lines,
                    idx,
                    Error::decode("blank line not allowed in array"),
                ));
            }
            let level = line.level;
            if row_level.is_none() {
                if level <= base_level {
                    return Ok((rows, idx, false));
                }
                row_level = Some(level);
            }
            let row_level = row_level.unwrap();
            if level < row_level {
                return Ok((rows, idx, false));
            }
            if level > row_level {
                return Err(self.attach_location_for_line(
                    lines,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let mut row_content = trim_ascii(&line.content);
            if let Some(stripped) = row_content.strip_prefix('-') {
                if stripped.starts_with(' ') || stripped.starts_with('\t') {
                    row_content = stripped.trim_start();
                }
            }
            let mut tokens = TokenBuf::with_capacity(fields.len());
            if !self
                .split_tabular_row_into(row_content, delimiter, &mut tokens)
                .map_err(|err| self.attach_location_for_slice(line, idx, row_content, err))?
            {
                return Ok((rows, idx, true));
            }
            if tokens.len() != fields.len() {
                if self.strict {
                    return Err(self.attach_location_for_slice(
                        line,
                        idx,
                        row_content,
                        Error::decode("tabular row field count mismatch"),
                    ));
                }
                if tokens.len() < fields.len() {
                    tokens.extend(std::iter::repeat_n("", fields.len() - tokens.len()));
                } else {
                    tokens.truncate(fields.len());
                }
            }
            let mut obj = Map::with_capacity(fields.len());
            if fast_path {
                for (idx, token) in tokens.iter().enumerate() {
                    let value = if token.is_empty() {
                        Value::String(String::new())
                    } else {
                        self.parse_value_token(token)
                            .map_err(|err| self.attach_location_for_slice(line, idx, token, err))?
                    };
                    obj.insert(field_names[idx].clone(), value);
                }
            } else {
                for (idx, token) in tokens.iter().enumerate() {
                    let value = if token.is_empty() {
                        Value::String(String::new())
                    } else {
                        self.parse_value_token(token)
                            .map_err(|err| self.attach_location_for_slice(line, idx, token, err))?
                    };
                    if let Some(parts) = field_paths[idx].as_deref() {
                        self.insert_path(&mut obj, parts, value)
                            .map_err(|err| self.attach_location_for_slice(line, idx, token, err))?;
                    } else {
                        obj.insert(field_names[idx].clone(), value);
                    }
                }
            }
            rows.push(Value::Object(obj));
            idx += 1;
        }
        Ok((rows, idx, false))
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
        if let Some(parts) = self.expandable_path_parts(&key) {
            return self.insert_path(map, &parts, value);
        }
        if self.expand_paths == ExpandPaths::Safe {
            if let Some(existing) = map.get(key.value.as_str()) {
                if self.strict && existing.is_object() != value.is_object() {
                    return Err(Error::decode("path conflict"));
                }
            }
        }
        map.insert(key.value.to_string(), value);
        Ok(())
    }

    fn expandable_path_parts<'a>(&self, key: &'a KeyToken) -> Option<Vec<&'a str>> {
        if self.expand_paths != ExpandPaths::Safe {
            return None;
        }
        if key.quoted || !key.value.as_str().contains('.') {
            return None;
        }
        let parts: Vec<&str> = key.value.as_str().split('.').collect();
        if parts.iter().all(|part| is_identifier_segment(part)) {
            Some(parts)
        } else {
            None
        }
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
                Some(existing) => match (existing, value) {
                    (Value::Object(existing_obj), Value::Object(new_obj)) => {
                        self.merge_objects_owned(existing_obj, new_obj)?;
                    }
                    (existing_value, new_value) => {
                        if self.expand_paths == ExpandPaths::Safe && self.strict {
                            return Err(Error::decode("path conflict"));
                        }
                        *existing_value = new_value;
                    }
                },
            }
        }
        Ok(())
    }

    fn insert_path(
        &self,
        map: &mut Map<String, Value>,
        parts: &[&str],
        value: Value,
    ) -> Result<()> {
        if parts.is_empty() {
            return Err(Error::decode("invalid path"));
        }
        let key = parts[0];
        if parts.len() == 1 {
            if let Some(existing) = map.get_mut(key) {
                match (existing, value) {
                    (Value::Object(existing_obj), Value::Object(new_obj)) => {
                        return self.merge_objects_owned(existing_obj, new_obj);
                    }
                    (existing_value, new_value) => {
                        if self.strict {
                            return Err(Error::decode("path conflict"));
                        }
                        *existing_value = new_value;
                        return Ok(());
                    }
                }
            }
            map.insert(key.to_string(), value);
            return Ok(());
        }
        match map.get_mut(key) {
            Some(Value::Object(obj)) => return self.insert_path(obj, &parts[1..], value),
            Some(_) => {
                if self.strict {
                    return Err(Error::decode("path conflict"));
                }
                map.insert(key.to_string(), Value::Object(Map::new()));
            }
            None => {
                map.insert(key.to_string(), Value::Object(Map::new()));
            }
        }
        let next = map
            .get_mut(key)
            .and_then(|value| value.as_object_mut())
            .ok_or_else(|| Error::decode("expected object"))?;
        self.insert_path(next, &parts[1..], value)
    }

    fn parse_list_block(
        &mut self,
        lines: &[Line],
        mut idx: usize,
        item_level: usize,
        expected_len: usize,
    ) -> Result<(Vec<Value>, usize)> {
        let mut items = Vec::with_capacity(expected_len);
        while idx < lines.len() {
            let line = &lines[idx];
            if line.is_blank {
                if !self.strict {
                    idx += 1;
                    continue;
                }
                let mut peek = idx + 1;
                while peek < lines.len() && lines[peek].is_blank {
                    peek += 1;
                }
                if peek >= lines.len() || lines[peek].level < item_level {
                    break;
                }
                return Err(self.attach_location_for_line(
                    lines,
                    idx,
                    Error::decode("blank line not allowed in array"),
                ));
            }
            let level = line.level;
            if level < item_level {
                break;
            }
            if level > item_level {
                return Err(self.attach_location_for_line(
                    lines,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(&line.content);
            if !content.starts_with('-') {
                return Err(self.attach_location_for_slice(
                    line,
                    idx,
                    content,
                    Error::decode("expected list item"),
                ));
            }
            let item_content = content[1..].trim_start();
            let (item, next_idx) = self
                .parse_list_item(item_content, lines, idx + 1, item_level)
                .map_err(|err| self.attach_location_for_slice(line, idx, item_content, err))?;
            items.push(item);
            idx = next_idx;
        }
        Ok((items, idx))
    }

    fn parse_list_item(
        &mut self,
        item_content: &str,
        lines: &[Line],
        idx: usize,
        item_level: usize,
    ) -> Result<(Value, usize)> {
        if item_content.is_empty() {
            return Ok((Value::Object(Map::new()), idx));
        }

        let header = match self.parse_array_header(item_content) {
            Ok(header) => header,
            Err(err) => {
                return Err(self.attach_location_for_slice(
                    lines.get(idx.saturating_sub(1)).unwrap_or(&lines[0]),
                    idx.saturating_sub(1),
                    item_content,
                    err,
                ));
            }
        };
        if let Some(header) = header {
            if header.key.is_none() {
                let parsed = self
                    .parse_array_from_header(&header, lines, idx, item_level)
                    .map_err(|err| {
                        let line_idx = idx.saturating_sub(1);
                        let line = lines.get(line_idx).unwrap_or(&lines[0]);
                        self.attach_location_for_slice(line, line_idx, item_content, err)
                    })?;
                return Ok((parsed.value, parsed.next_idx));
            }
            let key = header.key.clone().ok_or_else(|| {
                let line_idx = idx.saturating_sub(1);
                let line = lines.get(line_idx).unwrap_or(&lines[0]);
                self.attach_location_for_slice(
                    line,
                    line_idx,
                    item_content,
                    Error::decode("array header missing key in object context"),
                )
            })?;
            let array_base_level = if header.fields.is_some() {
                if self.validate || self.strict {
                    item_level + 1
                } else {
                    item_level
                }
            } else {
                item_level + 1
            };
            let parsed = if self.validate && header.fields.is_some() && header.inline.is_none() {
                let fields = header.fields.as_ref().ok_or_else(|| {
                    let line_idx = idx.saturating_sub(1);
                    let line = lines.get(line_idx).unwrap_or(&lines[0]);
                    self.attach_location_for_slice(
                        line,
                        line_idx,
                        item_content,
                        Error::decode("missing tabular fields"),
                    )
                })?;
                let (rows, next_idx, _) = self
                    .parse_tabular_block(
                        lines,
                        idx,
                        array_base_level,
                        fields,
                        header.delimiter,
                        header.len,
                    )
                    .map_err(|err| {
                        let line_idx = idx.saturating_sub(1);
                        let line = lines.get(line_idx).unwrap_or(&lines[0]);
                        self.attach_location_for_slice(line, line_idx, item_content, err)
                    })?;
                if self.strict && rows.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                ParsedArray {
                    value: Value::Array(rows),
                    next_idx,
                    deindent_next: false,
                }
            } else {
                self.parse_array_from_header(&header, lines, idx, array_base_level)
                    .map_err(|err| {
                        let line_idx = idx.saturating_sub(1);
                        let line = lines.get(line_idx).unwrap_or(&lines[0]);
                        self.attach_location_for_slice(line, line_idx, item_content, err)
                    })?
            };
            let mut map = Map::new();
            self.insert_key_value(&mut map, key, parsed.value)
                .map_err(|err| {
                    let line_idx = idx.saturating_sub(1);
                    let line = lines.get(line_idx).unwrap_or(&lines[0]);
                    self.attach_location_for_slice(line, line_idx, item_content, err)
                })?;
            let (extra, next_idx) = self
                .parse_object_block(lines, parsed.next_idx, item_level + 1)
                .map_err(|err| {
                    let line_idx = idx.saturating_sub(1);
                    let line = lines.get(line_idx).unwrap_or(&lines[0]);
                    self.attach_location_for_slice(line, line_idx, item_content, err)
                })?;
            self.merge_objects_owned(&mut map, extra).map_err(|err| {
                let line_idx = idx.saturating_sub(1);
                let line = lines.get(line_idx).unwrap_or(&lines[0]);
                self.attach_location_for_slice(line, line_idx, item_content, err)
            })?;
            return Ok((Value::Object(map), next_idx));
        }

        if self
            .split_key_value(item_content)
            .map_err(|err| {
                let line_idx = idx.saturating_sub(1);
                let line = lines.get(line_idx).unwrap_or(&lines[0]);
                self.attach_location_for_slice(line, line_idx, item_content, err)
            })?
            .is_some()
        {
            let line_idx = idx.saturating_sub(1);
            let line = lines.get(line_idx).unwrap_or(&lines[0]);
            let base = line.content.as_ptr() as usize;
            let slice_ptr = item_content.as_ptr() as usize;
            let item_offset = if slice_ptr >= base && slice_ptr <= base + line.content.len() {
                line.content_start + (slice_ptr - base)
            } else {
                line.raw_start
            };
            return self
                .parse_object_item_from_line(item_content, lines, idx, item_level, item_offset)
                .map_err(|err| self.attach_location_for_slice(line, line_idx, item_content, err));
        }

        let value = self.parse_value_token(item_content).map_err(|err| {
            let line_idx = idx.saturating_sub(1);
            let line = lines.get(line_idx).unwrap_or(&lines[0]);
            self.attach_location_for_slice(line, line_idx, item_content, err)
        })?;
        Ok((value, idx))
    }

    fn parse_object_item_from_line(
        &mut self,
        first_content: &str,
        lines: &[Line],
        idx: usize,
        item_level: usize,
        item_offset: usize,
    ) -> Result<(Value, usize)> {
        let base_level = item_level + 1;
        let mut combined = Vec::with_capacity(1 + lines.len().saturating_sub(idx));
        combined.push(Line {
            raw_start: item_offset,
            content_start: item_offset,
            indent: base_level * self.indent_size,
            level: base_level,
            content: first_content.to_string(),
            is_blank: false,
        });
        combined.extend_from_slice(&lines[idx..]);
        let (map, consumed) = self.parse_object_block(&combined, 0, base_level)?;
        let next_idx = idx + consumed.saturating_sub(1);
        Ok((Value::Object(map), next_idx))
    }
}

#[derive(Clone)]
struct KeyToken {
    value: SmolStr,
    quoted: bool,
}

struct HeaderLine {
    key: Option<KeyToken>,
    len: usize,
    delimiter: char,
    fields: Option<Vec<KeyToken>>,
    inline: Option<String>,
}

struct ParsedArray {
    value: Value,
    next_idx: usize,
    deindent_next: bool,
}

#[derive(Clone)]
struct Line {
    raw_start: usize,
    content_start: usize,
    indent: usize,
    level: usize,
    content: String,
    is_blank: bool,
}

struct StreamLine {
    idx: usize,
    line: Line,
}

struct LineStream<R: BufRead> {
    reader: R,
    buffer: String,
    pending: VecDeque<StreamLine>,
    line_idx: usize,
    offset: usize,
    last_line_had_newline: bool,
}

impl<R: BufRead> LineStream<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: String::new(),
            pending: VecDeque::new(),
            line_idx: 0,
            offset: 0,
            last_line_had_newline: false,
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
        self.last_line_had_newline = self.buffer.ends_with('\n');
        let raw_len = self.buffer.len();
        let mut line = self.buffer.as_str();
        if line.ends_with('\n') {
            line = &line[..line.len().saturating_sub(1)];
            if line.ends_with('\r') {
                line = &line[..line.len().saturating_sub(1)];
            }
        }
        if decoder.validate && !line.is_empty() {
            if let Some(&last) = line.as_bytes().last() {
                if last == b' ' || last == b'\t' {
                    return Err(Error::decode("trailing whitespace not allowed"));
                }
            }
        }
        let raw_start = self.offset;
        let line_idx = self.line_idx;
        self.offset += raw_len;
        self.line_idx += 1;
        let built = decoder.build_line(line, raw_start).map_err(|err| {
            err.with_location(Location {
                offset: raw_start,
                line: line_idx + 1,
                column: 1,
            })
        })?;
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

    fn check_trailing_newline(&self, validate: bool) -> Result<()> {
        if validate && self.last_line_had_newline {
            return Err(Error::decode("trailing newline not allowed"));
        }
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
            stream.check_trailing_newline(self.validate)?;
            return Ok(Value::Object(Map::new()));
        };
        let first_content = trim_ascii(&first.line.content);
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
                    stream.check_trailing_newline(self.validate)?;
                    return Ok(parsed.value);
                }
            }
        }

        if !stream.has_more_non_blank(self)? {
            if self.validate && self.reject_root_unquoted_string(first_content) {
                return Err(self.attach_location_for_slice(
                    &first.line,
                    first.idx,
                    first_content,
                    Error::decode("root string must be quoted"),
                ));
            }
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
            stream.check_trailing_newline(self.validate)?;
            return Ok(value);
        }

        stream.push_back(first);
        let map = self.parse_object_block_stream(&mut stream, 0)?;
        stream.drain_to_end(self)?;
        stream.check_trailing_newline(self.validate)?;
        Ok(Value::Object(map))
    }

    fn attach_location_for_line_meta(&self, line_idx: usize, line: &Line, err: Error) -> Error {
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
    ) -> Result<Map<String, Value>> {
        let mut map = Map::new();
        let mut override_level: Option<usize> = None;
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_blank {
                continue;
            }
            let actual_level = line.line.level;
            let level = override_level.take().unwrap_or(actual_level);
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
                if parsed.deindent_next {
                    override_level = Some(base_level);
                }
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
                        .parse_object_block_stream(stream, base_level + 1)
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

            if self.strict {
                return Err(self.attach_location_for_slice(
                    &line.line,
                    line.idx,
                    content,
                    Error::decode("bare key not allowed in strict mode"),
                ));
            }
            let key = self.parse_key_token(content).map_err(|err| {
                self.attach_location_for_slice(&line.line, line.idx, content, err)
            })?;
            self.insert_key_value(&mut map, key, Value::Null)
                .map_err(|err| {
                    self.attach_location_for_slice(&line.line, line.idx, content, err)
                })?;
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
        let result = (|| {
            if let Some(inline) = header.inline.as_deref() {
                let items = self.parse_inline_array(inline, header.delimiter, header.len)?;
                if self.strict && items.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    value: Value::Array(items),
                    next_idx: 0,
                    deindent_next: false,
                });
            }

            if let Some(fields) = header.fields.as_ref() {
                let (rows, deindent_next) = self.parse_tabular_block_stream(
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
                    next_idx: 0,
                    deindent_next,
                });
            }

            if header.len == 0 {
                return Ok(ParsedArray {
                    value: Value::Array(Vec::new()),
                    next_idx: 0,
                    deindent_next: false,
                });
            }

            let items = self.parse_list_block_stream(stream, base_level + 1, header.len)?;
            if header.len > 0 && items.is_empty() {
                return Err(Error::decode("array payload required"));
            }
            if self.strict && items.len() != header.len {
                return Err(Error::decode("array length mismatch"));
            }
            Ok(ParsedArray {
                value: Value::Array(items),
                next_idx: 0,
                deindent_next: false,
            })
        })();
        self.pop_delimiter();
        result
    }

    fn parse_tabular_block_stream<R: BufRead>(
        &self,
        stream: &mut LineStream<R>,
        base_level: usize,
        fields: &[KeyToken],
        delimiter: char,
        expected_len: usize,
    ) -> Result<(Vec<Value>, bool)> {
        let mut rows = Vec::with_capacity(expected_len);
        let mut field_paths: Vec<Option<Vec<String>>> = Vec::new();
        let mut fast_path = self.expand_paths != ExpandPaths::Safe;
        if !fast_path {
            field_paths = Vec::with_capacity(fields.len());
            for field in fields {
                let parts = self.expandable_path_parts(field);
                field_paths
                    .push(parts.map(|parts| parts.iter().map(|part| part.to_string()).collect()));
            }
            fast_path = field_paths.iter().all(|parts| parts.is_none());
        }
        let field_names: Vec<String> = fields.iter().map(|field| field.value.to_string()).collect();
        let mut row_level: Option<usize> = None;
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_blank {
                if !self.strict {
                    continue;
                }
                let next = stream.next_non_blank(self)?;
                if let Some(next_line) = next {
                    let next_level = next_line.line.level;
                    stream.push_back(next_line);
                    if next_level <= base_level {
                        return Ok((rows, false));
                    }
                    return Err(self.attach_location_for_line_meta(
                        line.idx,
                        &line.line,
                        Error::decode("blank line not allowed in array"),
                    ));
                }
                return Ok((rows, false));
            }
            let level = line.line.level;
            if row_level.is_none() {
                if level <= base_level {
                    stream.push_back(line);
                    return Ok((rows, false));
                }
                row_level = Some(level);
            }
            let row_level = row_level.unwrap();
            if level < row_level {
                stream.push_back(line);
                return Ok((rows, false));
            }
            if level > row_level {
                return Err(self.attach_location_for_line_meta(
                    line.idx,
                    &line.line,
                    Error::decode("unexpected indentation"),
                ));
            }
            let mut row_content = trim_ascii(&line.line.content);
            if let Some(stripped) = row_content.strip_prefix('-') {
                if stripped.starts_with(' ') || stripped.starts_with('\t') {
                    row_content = stripped.trim_start();
                }
            }
            let mut tokens = TokenBuf::with_capacity(fields.len());
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
                return Ok((rows, true));
            }
            if tokens.len() != fields.len() {
                if self.strict {
                    return Err(self.attach_location_for_slice(
                        &line.line,
                        line.idx,
                        row_content,
                        Error::decode("tabular row field count mismatch"),
                    ));
                }
                if tokens.len() < fields.len() {
                    tokens.extend(std::iter::repeat_n("", fields.len() - tokens.len()));
                } else {
                    tokens.truncate(fields.len());
                }
            }
            let mut obj = Map::with_capacity(fields.len());
            if fast_path {
                for (idx, token) in tokens.iter().enumerate() {
                    let value = if token.is_empty() {
                        Value::String(String::new())
                    } else {
                        self.parse_value_token(token).map_err(|err| {
                            self.attach_location_for_slice(&line.line, line.idx, token, err)
                        })?
                    };
                    obj.insert(field_names[idx].clone(), value);
                }
            } else {
                for (idx, token) in tokens.iter().enumerate() {
                    let value = if token.is_empty() {
                        Value::String(String::new())
                    } else {
                        self.parse_value_token(token).map_err(|err| {
                            self.attach_location_for_slice(&line.line, line.idx, token, err)
                        })?
                    };
                    if let Some(parts) = field_paths[idx].as_ref() {
                        let parts: Vec<&str> = parts.iter().map(|part| part.as_str()).collect();
                        self.insert_path(&mut obj, &parts, value).map_err(|err| {
                            self.attach_location_for_slice(&line.line, line.idx, token, err)
                        })?;
                    } else {
                        obj.insert(field_names[idx].clone(), value);
                    }
                }
            }
            rows.push(Value::Object(obj));
        }
        Ok((rows, false))
    }

    fn parse_list_block_stream<R: BufRead>(
        &mut self,
        stream: &mut LineStream<R>,
        item_level: usize,
        expected_len: usize,
    ) -> Result<Vec<Value>> {
        let mut items = Vec::with_capacity(expected_len);
        while let Some(line) = stream.next_line(self)? {
            if line.line.is_blank {
                if !self.strict {
                    continue;
                }
                let next = stream.next_non_blank(self)?;
                if let Some(next_line) = next {
                    let next_level = next_line.line.level;
                    stream.push_back(next_line);
                    if next_level < item_level {
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
            if !content.starts_with('-') {
                return Err(self.attach_location_for_slice(
                    &line.line,
                    line.idx,
                    content,
                    Error::decode("expected list item"),
                ));
            }
            let item_content = content[1..].trim_start();
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
        line_meta: &Line,
        line_idx: usize,
    ) -> Result<Value> {
        if item_content.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let header = match self.parse_array_header(item_content) {
            Ok(header) => header,
            Err(err) => {
                return Err(self.attach_location_for_slice(line_meta, line_idx, item_content, err));
            }
        };
        if let Some(header) = header {
            if header.key.is_none() {
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
            let array_base_level = if header.fields.is_some() {
                if self.validate || self.strict {
                    item_level + 1
                } else {
                    item_level
                }
            } else {
                item_level + 1
            };
            let parsed = if self.validate && header.fields.is_some() && header.inline.is_none() {
                let fields = header.fields.as_ref().ok_or_else(|| {
                    self.attach_location_for_slice(
                        line_meta,
                        line_idx,
                        item_content,
                        Error::decode("missing tabular fields"),
                    )
                })?;
                let (rows, _) = self
                    .parse_tabular_block_stream(
                        stream,
                        array_base_level,
                        fields,
                        header.delimiter,
                        header.len,
                    )
                    .map_err(|err| {
                        self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                    })?;
                if self.strict && rows.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                ParsedArray {
                    value: Value::Array(rows),
                    next_idx: 0,
                    deindent_next: false,
                }
            } else {
                self.parse_array_from_header_stream(&header, stream, array_base_level)
                    .map_err(|err| {
                        self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                    })?
            };
            let mut map = Map::new();
            self.insert_key_value(&mut map, key, parsed.value)
                .map_err(|err| {
                    self.attach_location_for_slice(line_meta, line_idx, item_content, err)
                })?;
            let extra = self
                .parse_object_block_stream(stream, item_level + 1)
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
        line_meta: &Line,
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
            content: item_content.to_string(),
            is_blank: false,
        };
        stream.push_back(StreamLine {
            idx: line_idx,
            line: synthetic,
        });
        let map = self.parse_object_block_stream(stream, base_level)?;
        Ok(Value::Object(map))
    }
}

pub(super) fn parse_number_token(token: &str) -> Option<serde_json::Number> {
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
        return None;
    }
    let value: Value = serde_json::from_str(token).ok()?;
    let number = value.as_number()?;
    let float = number.as_f64()?;
    serde_json::Number::from_f64(float)
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

pub(super) fn is_numeric_like(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    if bytes[i] == b'-' {
        i += 1;
    }
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return false;
    }
    for &byte in &bytes[i..] {
        if !byte.is_ascii_digit()
            && byte != b'.'
            && byte != b'e'
            && byte != b'E'
            && byte != b'+'
            && byte != b'-'
        {
            return false;
        }
    }
    true
}

pub(super) fn contains_whitespace(token: &str) -> bool {
    let bytes = token.as_bytes();
    for &byte in bytes {
        if byte.is_ascii_whitespace() {
            return true;
        }
        if byte >= 0x80 {
            return token.chars().any(|ch| ch.is_whitespace());
        }
    }
    false
}

pub(super) fn trim_ascii(input: &str) -> &str {
    if !input.is_ascii() {
        return input.trim();
    }
    let bytes = input.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &input[start..end]
}

pub(super) fn is_blank_line(line: &str) -> bool {
    if line.is_ascii() {
        return line
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_whitespace());
    }
    line.trim().is_empty()
}
