use std::collections::HashMap;

use memchr::{memchr, memchr2, memchr3, memchr_iter};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::arena::{ArenaView, Node, NodeData, NodeKind, Pair, Span, StringRef};
use crate::text::string::is_canonical_unquoted_key;
use crate::{DecodeOptions, Error, Indent, Result};

use super::scan::{scan_lines, ScanLine, ScanResult};
use super::{contains_whitespace, parse_number_token, trim_ascii};

type TokenBuf<'a> = SmallVec<[&'a str; 16]>;
pub fn parse_into<'a>(arena: &mut ArenaView<'a>, options: &DecodeOptions) -> Result<usize> {
    let mut parser = ArenaParser::new(arena, options);
    parser.parse_document()
}

struct ArenaParser<'a, 'b> {
    arena: &'b mut ArenaView<'a>,
    indent_size: usize,
    strict: bool,
    active_delimiter: char,
    delimiter_stack: Vec<char>,
    key_lookup: HashMap<SmolStr, usize>,
    null_node: Option<usize>,
    empty_string_node: Option<usize>,
}

impl<'a, 'b> ArenaParser<'a, 'b> {
    fn new(arena: &'b mut ArenaView<'a>, options: &DecodeOptions) -> Self {
        let Indent::Spaces(indent_size) = options.indent;
        Self {
            arena,
            indent_size,
            strict: options.strict,
            active_delimiter: ',',
            delimiter_stack: Vec::new(),
            key_lookup: HashMap::new(),
            null_node: None,
            empty_string_node: None,
        }
    }

    fn parse_document(&mut self) -> Result<usize> {
        let scan = scan_lines(self.arena.input, self.indent_size, self.strict)?;
        self.reserve_from_scan(&scan);
        if scan.non_blank == 0 {
            return Ok(self.push_object(&[]));
        }

        let first_non_blank_idx = scan
            .lines
            .iter()
            .position(|line| !line.is_blank)
            .unwrap_or(0);
        let first_line = &scan.lines[first_non_blank_idx];
        let first_content = trim_ascii(self.line_content(first_line));
        if first_content.starts_with('[') {
            if let Some(header) = self.parse_array_header(first_content)? {
                if header.key.is_none() {
                    if first_line.indent != 0 {
                        return Err(Error::decode("unexpected indentation"));
                    }
                    let parsed =
                        self.parse_array_from_header(&header, &scan, first_non_blank_idx + 1, 0)?;
                    self.ensure_no_trailing_content(&scan, parsed.next_idx)?;
                    return Ok(parsed.node_id);
                }
            }
        }

        if scan.non_blank == 1 && first_line.indent == 0 {
            return self.decode_single_line(first_content);
        }

        if scan.non_blank == 1 && self.strict && first_line.indent != 0 {
            return Err(Error::decode("unexpected indentation"));
        }

        let (node_id, idx) = self.parse_object_block(&scan, 0, 0)?;
        if idx < scan.lines.len() {
            return Err(Error::decode("unexpected trailing content"));
        }
        Ok(node_id)
    }

    fn ensure_no_trailing_content(&self, scan: &ScanResult, start_idx: usize) -> Result<()> {
        if scan.lines[start_idx..].iter().any(|line| !line.is_blank) {
            return Err(Error::decode("unexpected trailing content"));
        }
        Ok(())
    }

    fn decode_single_line(&mut self, line: &'a str) -> Result<usize> {
        if let Some(array) = self.parse_array_line(line)? {
            return Ok(array);
        }
        if let (Some(bracket_idx), Some(colon_idx)) = (line.find('['), line.find(':')) {
            if bracket_idx < colon_idx {
                if let Some(header) = self.parse_array_header(line)? {
                    if let Some(key) = header.key.as_ref() {
                        let value = self.build_array_value(&header)?;
                        let key_id = self.intern_key(&key.value);
                        let pairs = vec![Pair { key: key_id, value }];
                        return Ok(self.push_object(&pairs));
                    }
                }
            }
        }

        if let Some((key, value)) = self.split_key_value(line)? {
            let key = self.parse_key_token(trim_ascii(key))?;
            let value_id = if trim_ascii(value).is_empty() {
                self.push_object(&[])
            } else {
                self.parse_value_token(value)?
            };
            let key_id = self.intern_key(&key.value);
            let pairs = vec![Pair {
                key: key_id,
                value: value_id,
            }];
            return Ok(self.push_object(&pairs));
        }

        if self.strict {
            self.parse_array_header(line)?;
        }
        if self.strict && line.is_ascii() && !line.starts_with('"') && contains_whitespace(line) {
            return Err(Error::decode("unquoted primitive contains whitespace"));
        }
        self.parse_value_token(line)
    }

    fn parse_array_line(&mut self, line: &'a str) -> Result<Option<usize>> {
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

    fn build_array_value(&mut self, header: &HeaderLine<'a>) -> Result<usize> {
        let items = match header.inline {
            Some(inline) => self.parse_inline_array(inline, header.delimiter, header.len)?,
            None => Vec::new(),
        };
        if header.inline.is_none() && header.len > 0 {
            return Err(Error::decode("array payload not implemented"));
        }
        if self.strict && header.len != items.len() {
            return Err(Error::decode("array length mismatch"));
        }
        Ok(self.push_array(&items))
    }

    fn parse_inline_array(
        &mut self,
        inline: &'a str,
        delimiter: char,
        expected_len: usize,
    ) -> Result<Vec<usize>> {
        let tokens = self.split_delimited_with_capacity(inline, delimiter, expected_len)?;
        let mut values = Vec::with_capacity(expected_len.max(tokens.len()));
        for token in tokens {
            if token.is_empty() {
                values.push(self.empty_string_node());
            } else {
                values.push(self.parse_value_token_trimmed(token)?);
            }
        }
        Ok(values)
    }

    fn parse_array_from_header(
        &mut self,
        header: &HeaderLine<'a>,
        scan: &ScanResult,
        idx: usize,
        base_level: usize,
    ) -> Result<ParsedArray> {
        self.push_delimiter(header.delimiter);
        if header.inline.is_none() {
            self.arena.children.reserve(header.len);
        }
        let result = (|| {
            if let Some(inline) = header.inline {
                let items = self.parse_inline_array(inline, header.delimiter, header.len)?;
                if self.strict && items.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    node_id: self.push_array(&items),
                    next_idx: idx,
                    deindent_next: false,
                });
            }

            if let Some(fields) = header.fields.as_ref() {
                let (rows, next_idx, deindent_next) = self.parse_tabular_block(
                    scan,
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
                    node_id: self.push_array(&rows),
                    next_idx,
                    deindent_next,
                });
            }

            if header.len == 0 {
                return Ok(ParsedArray {
                    node_id: self.push_array(&[]),
                    next_idx: idx,
                    deindent_next: false,
                });
            }

            let (items, next_idx) = self.parse_list_block(scan, idx, base_level + 1, header.len)?;
            if self.strict && items.len() != header.len {
                return Err(Error::decode("array length mismatch"));
            }
            Ok(ParsedArray {
                node_id: self.push_array(&items),
                next_idx,
                deindent_next: false,
            })
        })();
        self.pop_delimiter();
        result
    }

    fn parse_tabular_block(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        base_level: usize,
        fields: &[KeyToken],
        delimiter: char,
        expected_len: usize,
    ) -> Result<(Vec<usize>, usize, bool)> {
        let mut rows = Vec::with_capacity(expected_len);
        let mut tokens = TokenBuf::with_capacity(fields.len());
        let mut value_ids: SmallVec<[usize; 16]> = SmallVec::with_capacity(fields.len());
        let mut field_key_ids = Vec::with_capacity(fields.len());
        for field in fields {
            field_key_ids.push(self.intern_key(&field.value));
        }
        let (unique_keys, field_slots) = build_field_slots(&field_key_ids);
        let null_id = self.null_node();
        let row_template: Vec<Pair> = unique_keys
            .iter()
            .map(|&key| Pair { key, value: null_id })
            .collect();
        self.arena
            .pairs
            .reserve(expected_len.saturating_mul(unique_keys.len()));
        self.arena.children.reserve(expected_len);
        self.arena.nodes.reserve(expected_len);
        let mut row_level = None;
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_blank {
                if !self.strict {
                    idx += 1;
                    continue;
                }
                let mut peek = idx + 1;
                while peek < scan.lines.len() && scan.lines[peek].is_blank {
                    peek += 1;
                }
                if peek >= scan.lines.len() || scan.lines[peek].level <= base_level {
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
            let mut row_content = trim_ascii(self.line_content(line));
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
            value_ids.clear();
            for token in tokens.iter() {
                let value_id = if token.is_empty() {
                    self.empty_string_node()
                } else {
                    self.parse_value_token_trimmed(token)?
                };
                value_ids.push(value_id);
            }
            let row_pair_start = self.arena.pairs.len();
            let row_pairs_len = row_template.len();
            self.arena
                .pairs
                .resize(row_pair_start + row_pairs_len, Pair { key: 0, value: null_id });
            {
                let row_slice =
                    &mut self.arena.pairs[row_pair_start..row_pair_start + row_pairs_len];
                row_slice.copy_from_slice(&row_template);
                for (index, value_id) in value_ids.iter().enumerate() {
                    let slot = field_slots[index];
                    row_slice[slot].value = *value_id;
                }
            }
            let row_node = self.push_node(NodeKind::Object, NodeData::None);
            self.arena.nodes[row_node].first_child = row_pair_start;
            self.arena.nodes[row_node].child_len = row_pairs_len;
            rows.push(row_node);
            idx += 1;
        }
        Ok((rows, idx, false))
    }

    fn parse_list_block(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        item_level: usize,
        expected_len: usize,
    ) -> Result<(Vec<usize>, usize)> {
        let mut items = Vec::with_capacity(expected_len);
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_blank {
                if !self.strict {
                    idx += 1;
                    continue;
                }
                let mut peek = idx + 1;
                while peek < scan.lines.len() && scan.lines[peek].is_blank {
                    peek += 1;
                }
                if peek >= scan.lines.len() || scan.lines[peek].level < item_level {
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
            let content = trim_ascii(self.line_content(line));
            if !content.starts_with('-') {
                return Err(Error::decode("expected list item"));
            }
            let item_content = content[1..].trim_start();
            let (item, next_idx) = self.parse_list_item(item_content, scan, idx + 1, item_level)?;
            items.push(item);
            idx = next_idx;
        }
        Ok((items, idx))
    }

    fn parse_list_item(
        &mut self,
        item_content: &'a str,
        scan: &ScanResult,
        idx: usize,
        item_level: usize,
    ) -> Result<(usize, usize)> {
        if item_content.is_empty() {
            return Ok((self.push_object(&[]), idx));
        }

        if let Some(header) = self.parse_array_header(item_content)? {
            if header.key.is_none() {
                let parsed = self.parse_array_from_header(&header, scan, idx, item_level)?;
                return Ok((parsed.node_id, parsed.next_idx));
            }
            let key = header
                .key
                .clone()
                .ok_or_else(|| Error::decode("array header missing key in object context"))?;
            let array_base_level = if header.fields.is_some() {
                if self.strict {
                    item_level + 1
                } else {
                    item_level
                }
            } else {
                item_level + 1
            };
            let parsed = self.parse_array_from_header(&header, scan, idx, array_base_level)?;
            let mut pairs = Vec::new();
            let mut pair_index = HashMap::new();
            let key_id = self.intern_key(&key.value);
            insert_pair(&mut pairs, &mut pair_index, key_id, parsed.node_id);
            let next_idx = self.parse_object_block_into(
                scan,
                parsed.next_idx,
                item_level + 1,
                &mut pairs,
                &mut pair_index,
            )?;
            let obj_node = self.push_object(&pairs);
            return Ok((obj_node, next_idx));
        }

        if self.split_key_value(item_content)?.is_some() {
            return self.parse_object_item_from_line(item_content, scan, idx, item_level);
        }

        let value = self.parse_value_token_trimmed(item_content)?;
        Ok((value, idx))
    }

    fn parse_object_item_from_line(
        &mut self,
        first_content: &'a str,
        scan: &ScanResult,
        mut idx: usize,
        item_level: usize,
    ) -> Result<(usize, usize)> {
        let base_level = item_level + 1;
        let mut pairs = Vec::new();
        let mut pair_index = HashMap::new();
        if let Some((key, value)) = self.split_key_value(first_content)? {
            let key = self.parse_key_token(trim_ascii(key))?;
            let key_id = self.intern_key(&key.value);
            if trim_ascii(value).is_empty() {
                let (nested, next_idx) = self.parse_object_block(scan, idx, base_level + 1)?;
                insert_pair(&mut pairs, &mut pair_index, key_id, nested);
                idx = next_idx;
            } else {
                let value_id = self.parse_value_token(value)?;
                insert_pair(&mut pairs, &mut pair_index, key_id, value_id);
            }
        }
        let next_idx =
            self.parse_object_block_into(scan, idx, base_level, &mut pairs, &mut pair_index)?;
        let obj_node = self.push_object(&pairs);
        Ok((obj_node, next_idx))
    }

    fn parse_object_block(
        &mut self,
        scan: &ScanResult,
        idx: usize,
        base_level: usize,
    ) -> Result<(usize, usize)> {
        let mut pairs = Vec::new();
        let mut pair_index = HashMap::new();
        let next_idx =
            self.parse_object_block_into(scan, idx, base_level, &mut pairs, &mut pair_index)?;
        let obj_node = self.push_object(&pairs);
        Ok((obj_node, next_idx))
    }

    fn parse_object_block_into(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        base_level: usize,
        pairs: &mut Vec<Pair>,
        pair_index: &mut HashMap<usize, usize>,
    ) -> Result<usize> {
        let mut override_level: Option<usize> = None;
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
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
            let content = trim_ascii(self.line_content(line));

            if let Some(header) = self.parse_array_header(content)? {
                let key = header
                    .key
                    .as_ref()
                    .ok_or_else(|| Error::decode("array header missing key in object context"))?;
                let parsed = self.parse_array_from_header(&header, scan, idx + 1, base_level)?;
                let key_id = self.intern_key(&key.value);
                insert_pair(pairs, pair_index, key_id, parsed.node_id);
                if parsed.deindent_next {
                    override_level = Some(base_level);
                }
                idx = parsed.next_idx;
                continue;
            }

            if let Some((key, value)) = self.split_key_value(content)? {
                let key = self.parse_key_token(trim_ascii(key))?;
                let key_id = self.intern_key(&key.value);
                if trim_ascii(value).is_empty() {
                    let (nested, next_idx) =
                        self.parse_object_block(scan, idx + 1, base_level + 1)?;
                    insert_pair(pairs, pair_index, key_id, nested);
                    idx = next_idx;
                } else {
                    let value_id = self.parse_value_token(value)?;
                    insert_pair(pairs, pair_index, key_id, value_id);
                    idx += 1;
                }
                continue;
            }

            if self.strict {
                return Err(Error::decode("bare key not allowed in strict mode"));
            }
            let key = self.parse_key_token(content)?;
            let key_id = self.intern_key(&key.value);
            let null_id = self.null_node();
            insert_pair(pairs, pair_index, key_id, null_id);
            idx += 1;
        }
        Ok(idx)
    }

    fn parse_value_token(&mut self, token: &str) -> Result<usize> {
        let token = trim_ascii(token);
        self.parse_value_token_trimmed(token)
    }

    fn parse_value_token_trimmed(&mut self, token: &str) -> Result<usize> {
        if token.is_empty() {
            return Err(Error::decode("empty value"));
        }
        if token.starts_with('"') {
            let string_ref = self.parse_quoted_ref(token)?;
            return Ok(self.push_string(string_ref));
        }
        match token {
            "null" => return Ok(self.null_node()),
            "true" => return Ok(self.push_bool(true)),
            "false" => return Ok(self.push_bool(false)),
            _ => {}
        }
        if parse_number_token(token).is_some() {
            let span = self.span_for(token);
            return Ok(self.push_number(span));
        }
        let span = self.span_for(token);
        Ok(self.push_string(StringRef::Span(span)))
    }

    fn parse_key_token(&self, token: &str) -> Result<KeyToken> {
        let token = trim_ascii(token);
        if token.starts_with('"') {
            let value_ref = self.parse_quoted_ref(token)?;
            let value = match value_ref {
                StringRef::Span(span) => {
                    let slice = self
                        .arena
                        .input
                        .get(span.start..span.end)
                        .ok_or_else(|| Error::decode("invalid string span"))?;
                    SmolStr::new(slice)
                }
                StringRef::Owned(value) => SmolStr::new(value.as_str()),
            };
            Ok(KeyToken {
                value,
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
            })
        }
    }

    fn parse_quoted_ref(&self, token: &str) -> Result<StringRef> {
        let token = trim_ascii(token);
        if token.len() < 2 || !token.starts_with('"') || !token.ends_with('"') {
            return Err(Error::decode("unterminated string"));
        }
        let inner = &token[1..token.len() - 1];
        let bytes = inner.as_bytes();
        if memchr(b'\\', bytes).is_none() {
            let span = self.span_for(inner);
            return Ok(StringRef::Span(span));
        }
        let mut out = String::with_capacity(inner.len());
        let mut idx = 0;
        while idx < bytes.len() {
            let Some(offset) = memchr2(b'\\', b'"', &bytes[idx..]) else {
                out.push_str(&inner[idx..]);
                break;
            };
            let pos = idx + offset;
            match bytes[pos] {
                b'\\' => {
                    out.push_str(&inner[idx..pos]);
                    let next_idx = pos + 1;
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
                    idx = pos + 2;
                }
                _ => return Err(Error::decode("unterminated string")),
            }
        }
        Ok(StringRef::Owned(out))
    }

    fn split_key_value<'c>(&self, line: &'c str) -> Result<Option<(&'c str, &'c str)>> {
        if line.is_ascii() {
            let bytes = line.as_bytes();
            if memchr(b'"', bytes).is_none() && memchr(b'\\', bytes).is_none() {
                if let Some(idx) = memchr(b':', bytes) {
                    return Ok(Some((&line[..idx], &line[idx + 1..])));
                }
                return Ok(None);
            }
        }
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

    fn parse_array_header(&self, line: &'a str) -> Result<Option<HeaderLine<'a>>> {
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
            Some(inline)
        };

        Ok(Some(HeaderLine {
            key,
            len,
            delimiter,
            fields,
            inline,
        }))
    }

    fn split_delimited<'c>(&self, input: &'c str, delimiter: char) -> Result<TokenBuf<'c>> {
        self.split_delimited_with_capacity(input, delimiter, 0)
    }

    fn split_delimited_with_capacity<'c>(
        &self,
        input: &'c str,
        delimiter: char,
        expected_len: usize,
    ) -> Result<TokenBuf<'c>> {
        let mut tokens = if expected_len > 0 {
            TokenBuf::with_capacity(expected_len)
        } else {
            TokenBuf::new()
        };
        self.split_delimited_into(input, delimiter, &mut tokens)?;
        Ok(tokens)
    }

    fn split_delimited_into<'c>(
        &self,
        input: &'c str,
        delimiter: char,
        tokens: &mut TokenBuf<'c>,
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

    fn line_content(&self, line: &ScanLine) -> &'a str {
        &self.arena.input[line.start..line.end]
    }

    fn reserve_from_scan(&mut self, scan: &ScanResult) {
        let estimated_nodes = scan.non_blank.saturating_mul(2).max(4);
        self.arena.nodes.reserve(estimated_nodes);
        self.arena.pairs.reserve(scan.non_blank);
        self.arena.children.reserve(scan.non_blank);
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

    fn span_for(&self, slice: &str) -> Span {
        let base = self.arena.input.as_ptr() as usize;
        let start = slice.as_ptr() as usize - base;
        Span {
            start,
            end: start + slice.len(),
        }
    }

    fn intern_key(&mut self, key: &SmolStr) -> usize {
        if let Some(&id) = self.key_lookup.get(key.as_str()) {
            return id;
        }
        let id = self.arena.keys.len();
        self.arena.keys.push(key.clone());
        self.key_lookup.insert(key.clone(), id);
        id
    }

    fn push_node(&mut self, kind: NodeKind, data: NodeData) -> usize {
        let index = self.arena.nodes.len();
        self.arena.nodes.push(Node {
            kind,
            data,
            first_child: 0,
            child_len: 0,
        });
        index
    }

    fn push_array(&mut self, children: &[usize]) -> usize {
        let node_index = self.push_node(NodeKind::Array, NodeData::None);
        let start = self.arena.children.len();
        self.arena.children.extend_from_slice(children);
        self.arena.nodes[node_index].first_child = start;
        self.arena.nodes[node_index].child_len = children.len();
        node_index
    }

    fn push_object(&mut self, pairs: &[Pair]) -> usize {
        let node_index = self.push_node(NodeKind::Object, NodeData::None);
        let start = self.arena.pairs.len();
        self.arena.pairs.extend_from_slice(pairs);
        self.arena.nodes[node_index].first_child = start;
        self.arena.nodes[node_index].child_len = pairs.len();
        node_index
    }

    fn push_string(&mut self, string_ref: StringRef) -> usize {
        let index = self.arena.strings.len();
        self.arena.strings.push(string_ref);
        self.push_node(NodeKind::String, NodeData::String(index))
    }

    fn push_number(&mut self, span: Span) -> usize {
        let index = self.arena.numbers.len();
        self.arena.numbers.push(span);
        self.push_node(NodeKind::Number, NodeData::Number(index))
    }

    fn push_bool(&mut self, value: bool) -> usize {
        self.push_node(NodeKind::Bool, NodeData::Bool(value))
    }

    fn null_node(&mut self) -> usize {
        if let Some(id) = self.null_node {
            return id;
        }
        let id = self.push_node(NodeKind::Null, NodeData::None);
        self.null_node = Some(id);
        id
    }

    fn empty_string_node(&mut self) -> usize {
        if let Some(id) = self.empty_string_node {
            return id;
        }
        let id = self.push_string(StringRef::Owned(String::new()));
        self.empty_string_node = Some(id);
        id
    }
}

#[derive(Clone)]
struct KeyToken {
    value: SmolStr,
}

struct HeaderLine<'a> {
    key: Option<KeyToken>,
    len: usize,
    delimiter: char,
    fields: Option<Vec<KeyToken>>,
    inline: Option<&'a str>,
}

struct ParsedArray {
    node_id: usize,
    next_idx: usize,
    deindent_next: bool,
}

fn insert_pair(
    pairs: &mut Vec<Pair>,
    pair_index: &mut HashMap<usize, usize>,
    key: usize,
    value: usize,
) {
    if let Some(&idx) = pair_index.get(&key) {
        pairs[idx].value = value;
        return;
    }
    let idx = pairs.len();
    pairs.push(Pair { key, value });
    pair_index.insert(key, idx);
}

fn build_field_slots(field_key_ids: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut unique_keys = Vec::new();
    let mut field_slots = Vec::with_capacity(field_key_ids.len());
    let mut slot_index: HashMap<usize, usize> = HashMap::new();
    for &key_id in field_key_ids {
        let slot = match slot_index.get(&key_id) {
            Some(&slot) => slot,
            None => {
                let slot = unique_keys.len();
                unique_keys.push(key_id);
                slot_index.insert(key_id, slot);
                slot
            }
        };
        field_slots.push(slot);
    }
    (unique_keys, field_slots)
}
