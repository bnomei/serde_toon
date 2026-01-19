use std::io::Read;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::num::number::format_json_number;
use crate::text::string::{is_canonical_unquoted_key, is_identifier_segment};
use crate::{DecodeOptions, Error, ExpandPaths, Indent, Result};

pub fn from_str<T: DeserializeOwned>(input: &str, options: &DecodeOptions) -> Result<T> {
    let mut decoder = Decoder::new(options);
    let value = decoder.decode_document(input)?;
    serde_json::from_value(value)
        .map_err(|err| Error::deserialize(format!("deserialize failed: {err}")))
}

pub fn from_slice<T: DeserializeOwned>(input: &[u8], options: &DecodeOptions) -> Result<T> {
    let text =
        std::str::from_utf8(input).map_err(|err| Error::decode(format!("invalid utf-8: {err}")))?;
    from_str(text, options)
}

pub fn from_reader<T: DeserializeOwned, R: Read>(
    mut reader: R,
    options: &DecodeOptions,
) -> Result<T> {
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .map_err(|err| Error::decode(format!("read failed: {err}")))?;
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
}

impl Decoder {
    fn new(options: &DecodeOptions) -> Self {
        let Indent::Spaces(indent_size) = options.indent;
        Self {
            indent_size,
            strict: options.strict,
            expand_paths: options.expand_paths,
            validate: false,
        }
    }

    fn new_validator(options: &DecodeOptions) -> Self {
        let mut decoder = Self::new(options);
        decoder.validate = true;
        decoder
    }

    fn validate_document(&mut self, input: &str) -> Result<()> {
        self.validate_whitespace(input)?;
        self.decode_document(input).map(|_| ())
    }

    fn validate_whitespace(&self, input: &str) -> Result<()> {
        if input.ends_with('\n') {
            return Err(Error::decode("trailing newline not allowed"));
        }
        for line in input.split('\n') {
            if line.ends_with(' ') || line.ends_with('\t') {
                return Err(Error::decode("trailing whitespace not allowed"));
            }
        }
        Ok(())
    }

    fn decode_document(&mut self, input: &str) -> Result<Value> {
        let lines = self.collect_lines(input)?;

        let non_blank: Vec<&Line> = lines.iter().filter(|line| !line.is_blank).collect();

        if non_blank.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let first_non_blank_idx = lines.iter().position(|line| !line.is_blank).unwrap_or(0);
        let first_line = &lines[first_non_blank_idx];
        let first_content = first_line.content.trim();
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
            let content = non_blank[0].content.trim();
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
            let key = self.parse_key_token(key.trim())?;
            let value = if value.trim().is_empty() {
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
        if self.strict
            && !line.starts_with('"')
            && line.is_ascii()
            && line.chars().any(|ch| ch.is_whitespace())
        {
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
        let tokens = self.split_delimited(inline, delimiter)?;
        let mut values = Vec::with_capacity(expected_len.max(tokens.len()));
        for token in tokens {
            if token.trim().is_empty() {
                values.push(Value::String(String::new()));
            } else {
                values.push(self.parse_value_token(&token)?);
            }
        }
        Ok(values)
    }

    fn split_delimited(&self, input: &str, delimiter: char) -> Result<Vec<String>> {
        let mut tokens = Vec::new();
        let mut buf = String::new();
        let mut in_quotes = false;
        let mut escape = false;

        for ch in input.chars() {
            if escape {
                buf.push(ch);
                escape = false;
                continue;
            }
            if in_quotes {
                if ch == '\\' {
                    escape = true;
                    buf.push(ch);
                    continue;
                }
                if ch == '"' {
                    in_quotes = false;
                }
                buf.push(ch);
                continue;
            }
            if ch == '"' {
                in_quotes = true;
                buf.push(ch);
                continue;
            }
            if ch == delimiter {
                tokens.push(buf.trim().to_string());
                buf.clear();
                continue;
            }
            buf.push(ch);
        }

        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }

        if !buf.is_empty() || input.ends_with(delimiter) {
            tokens.push(buf.trim().to_string());
        }
        Ok(tokens)
    }

    fn parse_value_token(&self, token: &str) -> Result<Value> {
        if self.validate {
            self.validate_value_token(token)?;
        }
        let token = token.trim();
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
        let token = token.trim();
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
            let value: Value =
                serde_json::from_str(token).map_err(|_| Error::decode("invalid number"))?;
            let number = value
                .as_number()
                .ok_or_else(|| Error::decode("invalid number"))?;
            let canonical = format_json_number(number);
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

    fn parse_key_token(&self, token: &str) -> Result<KeyToken> {
        let token = token.trim();
        if token.starts_with('"') {
            let value = self.parse_quoted(token)?;
            Ok(KeyToken {
                value,
                quoted: true,
            })
        } else {
            if self.strict {
                if token.chars().any(|ch| ch.is_whitespace()) {
                    return Err(Error::decode("invalid unquoted key"));
                }
                if token.is_ascii() && !is_canonical_unquoted_key(token) {
                    return Err(Error::decode("invalid unquoted key"));
                }
            }
            Ok(KeyToken {
                value: token.to_string(),
                quoted: false,
            })
        }
    }

    fn parse_quoted(&self, token: &str) -> Result<String> {
        let token = token.trim();
        if token.len() < 2 || !token.starts_with('"') || !token.ends_with('"') {
            return Err(Error::decode("unterminated string"));
        }
        let inner = &token[1..token.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let next = chars
                    .next()
                    .ok_or_else(|| Error::decode("unterminated escape"))?;
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    _ => return Err(Error::decode("invalid escape")),
                }
            } else {
                out.push(ch);
            }
        }
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

        let key_part = line[..bracket_start].trim();
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
                if token.trim().is_empty() {
                    return Err(Error::decode("empty field name"));
                }
                parsed_fields.push(self.parse_key_token(&token)?);
            }
            fields = Some(parsed_fields);
            rest = rest[end + 1..].trim_start();
        }

        let colon_idx = rest
            .find(':')
            .ok_or_else(|| Error::decode("array header missing ':'"))?;
        if !rest[..colon_idx].trim().is_empty() {
            return Err(Error::decode("invalid array header suffix"));
        }
        let inline = rest[colon_idx + 1..].trim();
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
        let mut lines = Vec::new();
        for raw in input.split('\n') {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() {
                lines.push(Line {
                    indent: 0,
                    level: 0,
                    content: String::new(),
                    is_blank: true,
                });
                continue;
            }
            let mut indent_columns: usize = 0;
            let mut indent_chars: usize = 0;
            for ch in line.chars() {
                match ch {
                    ' ' => {
                        indent_columns += 1;
                        indent_chars += 1;
                    }
                    '\t' => {
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
            lines.push(Line {
                indent: indent_columns,
                level,
                content,
                is_blank: false,
            });
        }
        Ok(lines)
    }

    fn decode_object_lines(&self, lines: &[Line]) -> Result<Map<String, Value>> {
        let (map, idx) = self.parse_object_block(lines, 0, 0)?;
        if idx < lines.len() {
            return Err(Error::decode("unexpected trailing content"));
        }
        Ok(map)
    }

    fn parse_object_block(
        &self,
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
            let content = line.content.trim();

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
                let key = self.parse_key_token(key.trim())?;
                if value.trim().is_empty() {
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
        &self,
        header: &HeaderLine,
        lines: &[Line],
        idx: usize,
        base_level: usize,
    ) -> Result<ParsedArray> {
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

        let (items, next_idx) = self.parse_list_block(lines, idx, base_level + 1, header.len)?;
        if self.strict && items.len() != header.len {
            return Err(Error::decode("array length mismatch"));
        }
        Ok(ParsedArray {
            value: Value::Array(items),
            next_idx,
            deindent_next: false,
        })
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
            let mut row_content = line.content.trim();
            if let Some(stripped) = row_content.strip_prefix('-') {
                if stripped.starts_with(' ') || stripped.starts_with('\t') {
                    row_content = stripped.trim_start();
                }
            }
            if !self.is_tabular_row(row_content, delimiter)? {
                return Ok((rows, idx, true));
            }
            let mut tokens = self.split_delimited(row_content, delimiter)?;
            if tokens.len() != fields.len() {
                if self.strict {
                    return Err(Error::decode("tabular row field count mismatch"));
                }
                if tokens.len() < fields.len() {
                    tokens.extend(std::iter::repeat_n(
                        String::new(),
                        fields.len() - tokens.len(),
                    ));
                } else {
                    tokens.truncate(fields.len());
                }
            }
            let mut obj = Map::new();
            for (field, token) in fields.iter().zip(tokens) {
                let value = if token.trim().is_empty() {
                    Value::String(String::new())
                } else {
                    self.parse_value_token(&token)?
                };
                self.insert_key_value(&mut obj, field.clone(), value)?;
            }
            rows.push(Value::Object(obj));
            idx += 1;
        }
        Ok((rows, idx, false))
    }

    fn is_tabular_row(&self, line: &str, delimiter: char) -> Result<bool> {
        let mut in_quotes = false;
        let mut escape = false;
        let mut colon_pos = None;
        let mut delim_pos = None;
        let delim_byte = delimiter as u8;
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
            if *byte == b':' && colon_pos.is_none() {
                colon_pos = Some(idx);
            }
            if *byte == delim_byte && delim_pos.is_none() {
                delim_pos = Some(idx);
            }
        }
        if in_quotes {
            return Err(Error::decode("unterminated string"));
        }
        if let Some(colon) = colon_pos {
            if delim_pos.is_none() || delim_pos.is_some_and(|pos| colon < pos) {
                return Ok(false);
            }
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
            if let Some(existing) = map.get(&key.value) {
                if self.strict && existing.is_object() != value.is_object() {
                    return Err(Error::decode("path conflict"));
                }
            }
        }
        map.insert(key.value, value);
        Ok(())
    }

    fn expandable_path_parts<'a>(&self, key: &'a KeyToken) -> Option<Vec<&'a str>> {
        if self.expand_paths != ExpandPaths::Safe {
            return None;
        }
        if key.quoted || !key.value.contains('.') {
            return None;
        }
        let parts: Vec<&str> = key.value.split('.').collect();
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
        &self,
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
            let content = line.content.trim();
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
        &self,
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
        &self,
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
    value: String,
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

fn is_int_with_leading_zero(token: &str) -> bool {
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

fn is_numeric_like(token: &str) -> bool {
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
