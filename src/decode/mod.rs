mod parser;
mod pool;
mod scan;
mod serde;

use std::io::Read;

use ::serde::de::DeserializeOwned;
use memchr::{memchr, memchr2, memchr3, memchr_iter};
use serde_json::{Map, Value};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::arena::ArenaView;
use crate::num::number::format_json_number;
use crate::text::string::{is_canonical_unquoted_key, is_identifier_segment};
use crate::{DecodeOptions, Error, ExpandPaths, Indent, Result};

#[cfg(feature = "parallel")]
use crate::arena::NodeKind;
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
    let mut decoder = Decoder::new(options);
    decoder.decode_document(input)
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

pub fn from_reader<T: DeserializeOwned, R: Read>(
    mut reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .map_err(|err| Error::decode_with_source(format!("read failed: {err}"), err))?;
    from_str(&buf, options)
}

pub fn validate_str(input: &str, options: &DecodeOptions) -> Result<()> {
    let mut validator = Decoder::new_validator(options);
    validator.validate_document(input)
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
            if let Some(header) = self.parse_array_header(first_content)? {
                if header.key.is_none() {
                    if first_line.indent != 0 {
                        return Err(Error::decode("unexpected indentation"));
                    }
                    let parsed =
                        self.parse_array_from_header(&header, &lines, first_non_blank_idx + 1, 0)?;
                    self.ensure_no_trailing_content(&lines, parsed.next_idx)?;
                    return Ok(parsed.value);
                }
            }
        }

        if non_blank.len() == 1 && non_blank[0].indent == 0 {
            let content = trim_ascii(&non_blank[0].content);
            if self.validate && self.reject_root_unquoted_string(content) {
                return Err(Error::decode("root string must be quoted"));
            }
            return self.decode_single_line(content);
        }

        if non_blank.len() == 1 && self.strict && non_blank[0].indent != 0 {
            return Err(Error::decode("unexpected indentation"));
        }

        let map = self.decode_object_lines(&lines)?;
        Ok(Value::Object(map))
    }

    fn ensure_no_trailing_content(&self, lines: &[Line], start_idx: usize) -> Result<()> {
        if lines[start_idx..].iter().any(|line| !line.is_blank) {
            return Err(Error::decode("unexpected trailing content"));
        }
        Ok(())
    }

    fn decode_single_line(&mut self, line: &str) -> Result<Value> {
        if let Some(array) = self.parse_array_line(line)? {
            return Ok(array);
        }
        if let (Some(bracket_idx), Some(colon_idx)) = (line.find('['), line.find(':')) {
            if bracket_idx < colon_idx {
                if let Some(header) = self.parse_array_header(line)? {
                    if let Some(key) = header.key.as_ref() {
                        let value = self.build_array_value(&header)?;
                        let mut map = Map::new();
                        self.insert_key_value(&mut map, key.clone(), value)?;
                        return Ok(Value::Object(map));
                    }
                }
            }
        }
        if let Some((key, value)) = self.split_key_value(line)? {
            let mut map = Map::new();
            let key = self.parse_key_token(trim_ascii(key))?;
            let value = if trim_ascii(value).is_empty() {
                Value::Object(Map::new())
            } else {
                self.parse_value_token(value)?
            };
            self.insert_key_value(&mut map, key, value)?;
            return Ok(Value::Object(map));
        }
        if self.strict {
            self.parse_array_header(line)?;
        }
        if self.strict && line.is_ascii() && !line.starts_with('"') && contains_whitespace(line) {
            return Err(Error::decode("unquoted primitive contains whitespace"));
        }
        self.parse_value_token(line)
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
            return Err(Error::decode("array payload not implemented"));
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
            let line = &input[start..end];
            lines.push(self.build_line(line)?);
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
        let line = &input[start..end];
        lines.push(self.build_line(line)?);

        Ok(lines)
    }

    fn build_line(&self, line: &str) -> Result<Line> {
        if is_blank_line(line) {
            return Ok(Line {
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
        let content = line[indent_chars..].to_string();
        Ok(Line {
            indent: indent_columns,
            level,
            content,
            is_blank: false,
        })
    }

    fn decode_object_lines(&mut self, lines: &[Line]) -> Result<Map<String, Value>> {
        let (map, idx) = self.parse_object_block(lines, 0, 0)?;
        if idx < lines.len() {
            return Err(Error::decode("unexpected trailing content"));
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
                return Err(Error::decode("unexpected indentation"));
            }
            let content = trim_ascii(&line.content);

            if let Some(header) = self.parse_array_header(content)? {
                let key = header
                    .key
                    .as_ref()
                    .ok_or_else(|| Error::decode("array header missing key in object context"))?;
                let parsed = self.parse_array_from_header(&header, lines, idx + 1, base_level)?;
                self.insert_key_value(&mut map, key.clone(), parsed.value)?;
                if parsed.deindent_next {
                    override_level = Some(base_level);
                }
                idx = parsed.next_idx;
                continue;
            }

            if let Some((key, value)) = self.split_key_value(content)? {
                let key = self.parse_key_token(trim_ascii(key))?;
                if trim_ascii(value).is_empty() {
                    let (nested, next_idx) =
                        self.parse_object_block(lines, idx + 1, base_level + 1)?;
                    self.insert_key_value(&mut map, key, Value::Object(nested))?;
                    idx = next_idx;
                } else {
                    let value = self.parse_value_token(value)?;
                    self.insert_key_value(&mut map, key, value)?;
                    idx += 1;
                }
                continue;
            }

            if self.strict {
                return Err(Error::decode("bare key not allowed in strict mode"));
            }
            let key = self.parse_key_token(content)?;
            self.insert_key_value(&mut map, key, Value::Null)?;
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
        let mut tokens = TokenBuf::with_capacity(fields.len());
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
                return Err(Error::decode("blank line not allowed in array"));
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
                return Err(Error::decode("unexpected indentation"));
            }
            let mut row_content = trim_ascii(&line.content);
            if let Some(stripped) = row_content.strip_prefix('-') {
                if stripped.starts_with(' ') || stripped.starts_with('\t') {
                    row_content = stripped.trim_start();
                }
            }
            if !self.split_tabular_row_into(row_content, delimiter, &mut tokens)? {
                return Ok((rows, idx, true));
            }
            if tokens.len() != fields.len() {
                if self.strict {
                    return Err(Error::decode("tabular row field count mismatch"));
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
                        self.parse_value_token(token)?
                    };
                    obj.insert(field_names[idx].clone(), value);
                }
            } else {
                for (idx, token) in tokens.iter().enumerate() {
                    let value = if token.is_empty() {
                        Value::String(String::new())
                    } else {
                        self.parse_value_token(token)?
                    };
                    if let Some(parts) = field_paths[idx].as_deref() {
                        self.insert_path(&mut obj, parts, value)?;
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
                return Err(Error::decode("blank line not allowed in array"));
            }
            let level = line.level;
            if level < item_level {
                break;
            }
            if level > item_level {
                return Err(Error::decode("unexpected indentation"));
            }
            let content = trim_ascii(&line.content);
            if !content.starts_with('-') {
                return Err(Error::decode("expected list item"));
            }
            let item_content = content[1..].trim_start();
            let (item, next_idx) =
                self.parse_list_item(item_content, lines, idx + 1, item_level)?;
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

        if let Some(header) = self.parse_array_header(item_content)? {
            if header.key.is_none() {
                let parsed = self.parse_array_from_header(&header, lines, idx, item_level)?;
                return Ok((parsed.value, parsed.next_idx));
            }
            let key = header
                .key
                .clone()
                .ok_or_else(|| Error::decode("array header missing key in object context"))?;
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
                let fields = header
                    .fields
                    .as_ref()
                    .ok_or_else(|| Error::decode("missing tabular fields"))?;
                let (rows, next_idx, _) = self.parse_tabular_block(
                    lines,
                    idx,
                    array_base_level,
                    fields,
                    header.delimiter,
                    header.len,
                )?;
                if self.strict && rows.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                ParsedArray {
                    value: Value::Array(rows),
                    next_idx,
                    deindent_next: false,
                }
            } else {
                self.parse_array_from_header(&header, lines, idx, array_base_level)?
            };
            let mut map = Map::new();
            self.insert_key_value(&mut map, key, parsed.value)?;
            let (extra, next_idx) =
                self.parse_object_block(lines, parsed.next_idx, item_level + 1)?;
            self.merge_objects_owned(&mut map, extra)?;
            return Ok((Value::Object(map), next_idx));
        }

        if self.split_key_value(item_content)?.is_some() {
            return self.parse_object_item_from_line(item_content, lines, idx, item_level);
        }

        let value = self.parse_value_token(item_content)?;
        Ok((value, idx))
    }

    fn parse_object_item_from_line(
        &mut self,
        first_content: &str,
        lines: &[Line],
        idx: usize,
        item_level: usize,
    ) -> Result<(Value, usize)> {
        let base_level = item_level + 1;
        let mut combined = Vec::with_capacity(1 + lines.len().saturating_sub(idx));
        combined.push(Line {
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
    indent: usize,
    level: usize,
    content: String,
    is_blank: bool,
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
