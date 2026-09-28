use std::collections::HashMap;

use memchr::{memchr, memchr2, memchr3, memchr_iter};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::arena::{ArenaView, Node, NodeData, NodeKind, Pair, Span, StringRef};
use crate::error::Location;
use crate::{DecodeOptions, Error, Indent, Result};

use super::scan::{scan_lines, ScanLine, ScanResult};
use super::{parse_number_token, trim_ascii};

type TokenBuf<'a> = SmallVec<[&'a str; 16]>;
pub fn parse_into<'a>(arena: &mut ArenaView<'a>, options: &DecodeOptions) -> Result<usize> {
    let mut parser = ArenaParser::new(arena, options, false);
    parser.parse_document()
}

pub fn parse_into_validate<'a>(
    arena: &mut ArenaView<'a>,
    options: &DecodeOptions,
) -> Result<usize> {
    let mut parser = ArenaParser::new(arena, options, true);
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
    validate: bool,
}

impl<'a, 'b> ArenaParser<'a, 'b> {
    fn new(arena: &'b mut ArenaView<'a>, options: &DecodeOptions, validate: bool) -> Self {
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
            validate,
        }
    }

    fn parse_document(&mut self) -> Result<usize> {
        let scan = scan_lines(
            self.arena.input,
            self.indent_size,
            self.strict,
            self.validate,
        )?;
        self.reserve_from_scan(&scan);
        self.reject_blanks_in_header_spans(&scan)?;
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
        if first_content.starts_with('\t') {
            return Err(Error::decode("tabs not allowed in indentation"));
        }
        if first_content == "[]" {
            if self.strict && first_line.indent != 0 {
                return Err(Error::decode("unexpected indentation"));
            }
            self.ensure_no_trailing_content(&scan, first_non_blank_idx + 1)?;
            return Ok(self.push_array(&[]));
        }
        if first_content.starts_with('[') {
            if let Some(header) = self.parse_array_header(first_content)? {
                if header.key.is_none() {
                    if first_line.indent != 0 {
                        return Err(self.attach_location_for_line(
                            &scan,
                            first_non_blank_idx,
                            Error::decode("unexpected indentation"),
                        ));
                    }
                    let parsed = self
                        .parse_array_from_header(&header, &scan, first_non_blank_idx + 1, 0)
                        .map_err(|err| self.attach_location_for_slice(&scan, first_content, err))?;
                    self.ensure_no_trailing_content(&scan, parsed.next_idx)?;
                    return Ok(parsed.node_id);
                }
            }
        }

        if scan.non_blank == 1 && (first_line.indent == 0 || !self.strict) {
            return self
                .decode_single_line(first_content, &scan)
                .map_err(|err| self.attach_location_for_slice(&scan, first_content, err));
        }

        if scan.non_blank == 1 && self.strict && first_line.indent != 0 {
            return Err(self.attach_location_for_line(
                &scan,
                first_non_blank_idx,
                Error::decode("unexpected indentation"),
            ));
        }

        let (node_id, idx) = self.parse_object_block(&scan, 0, 0)?;
        if idx < scan.lines.len() {
            return Err(self.attach_location_for_line(
                &scan,
                idx,
                Error::decode("unexpected trailing content"),
            ));
        }
        Ok(node_id)
    }

    fn ensure_no_trailing_content(&self, scan: &ScanResult, start_idx: usize) -> Result<()> {
        if let Some((idx, _)) = scan
            .lines
            .iter()
            .enumerate()
            .skip(start_idx)
            .find(|(_, line)| !line.is_blank)
        {
            return Err(self.attach_location_for_line(
                scan,
                idx,
                Error::decode("unexpected trailing content"),
            ));
        }
        Ok(())
    }

    fn decode_single_line(&mut self, line: &'a str, scan: &ScanResult) -> Result<usize> {
        if let Some(array) = self
            .parse_array_line(line)
            .map_err(|err| self.attach_location_for_slice(scan, line, err))?
        {
            return Ok(array);
        }
        if let Some(header) = self
            .parse_array_header(line)
            .map_err(|err| self.attach_location_for_slice(scan, line, err))?
        {
            if let Some(key) = header.key.as_ref() {
                let value = self
                    .build_array_value(&header)
                    .map_err(|err| self.attach_location_for_slice(scan, line, err))?;
                let key_id = self.intern_key(&key.value);
                let pairs = vec![Pair { key: key_id, value }];
                return Ok(self.push_object(&pairs));
            }
        }

        if let Some((key, value)) = self
            .split_key_value(line)
            .map_err(|err| self.attach_location_for_slice(scan, line, err))?
        {
            let key = self
                .parse_key_token(trim_ascii(key))
                .map_err(|err| self.attach_location_for_slice(scan, key, err))?;
            let value_id = if trim_ascii(value).is_empty() {
                self.push_object(&[])
            } else {
                let value_trimmed = trim_ascii(value);
                self.parse_value_token(value)
                    .map_err(|err| self.attach_location_for_slice(scan, value_trimmed, err))?
            };
            let key_id = self.intern_key(&key.value);
            let pairs = vec![Pair {
                key: key_id,
                value: value_id,
            }];
            return Ok(self.push_object(&pairs));
        }

        if self.strict {
            self.parse_array_header(line)
                .map_err(|err| self.attach_location_for_slice(scan, line, err))?;
        }
        self.parse_value_token(line)
            .map_err(|err| self.attach_location_for_slice(scan, line, err))
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
        if header.keyed && header.inline.is_none() {
            if self.strict && header.len != 0 {
                return Err(Error::decode("array length mismatch"));
            }
            return Ok(self.push_object(&[]));
        }
        if header.fields.is_some() && header.inline.is_some() {
            return Err(Error::decode(
                "fields-bearing header cannot have inline content",
            ));
        }
        let items = match header.inline {
            Some(inline) => self.parse_inline_array(inline, header.delimiter, header.len)?,
            None => Vec::new(),
        };
        if self.strict && header.inline.is_none() && header.len > 0 {
            return Err(Error::decode("array payload required"));
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
        let mut values = Vec::with_capacity(tokens.len());
        for token in tokens {
            if token.is_empty() {
                values.push(self.empty_string_node());
            } else if token == "[]" {
                let span = self.span_for(token);
                values.push(self.push_string(StringRef::Span(span)));
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
        let result = (|| {
            if header.fields.is_some() && header.inline.is_some() {
                return Err(Error::decode(
                    "fields-bearing header cannot have inline content",
                ));
            }
            if header.keyed {
                let fields = header
                    .fields
                    .as_ref()
                    .ok_or_else(|| Error::decode("keyed header requires fields"))?;
                let (node_id, next_idx) = self.parse_keyed_block(
                    scan,
                    idx,
                    base_level,
                    fields,
                    header.delimiter,
                    header.len,
                )?;
                return Ok(ParsedArray { node_id, next_idx });
            }
            if let Some(inline) = header.inline {
                let items = self.parse_inline_array(inline, header.delimiter, header.len)?;
                if self.strict && items.len() != header.len {
                    return Err(Error::decode("array length mismatch"));
                }
                return Ok(ParsedArray {
                    node_id: self.push_array(&items),
                    next_idx: idx,
                });
            }

            if let Some(fields) = header.fields.as_ref() {
                let (rows, next_idx) = self.parse_tabular_block(
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
                });
            }

            if header.len == 0 && self.strict {
                return Ok(ParsedArray {
                    node_id: self.push_array(&[]),
                    next_idx: idx,
                });
            }

            let (items, next_idx) = self.parse_list_block(scan, idx, base_level + 1, header.len)?;
            if self.strict && header.len > 0 && items.is_empty() {
                return Err(Error::decode("array payload required"));
            }
            if self.strict && items.len() != header.len {
                return Err(Error::decode("array length mismatch"));
            }
            Ok(ParsedArray {
                node_id: self.push_array(&items),
                next_idx,
            })
        })();
        self.pop_delimiter();
        result
    }

    fn parse_keyed_block(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        base_level: usize,
        fields: &[FieldEntry],
        delimiter: char,
        expected_len: usize,
    ) -> Result<(usize, usize)> {
        let leaf_count: usize = fields.iter().map(FieldEntry::leaf_count).sum();
        let mut pairs = Vec::new();
        let mut pair_index = HashMap::new();
        let mut entries = 0;
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_comment {
                idx += 1;
                continue;
            }
            if line.is_blank {
                if !self.strict || entries == 0 {
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
                return Err(self.attach_location_for_line(
                    scan,
                    idx,
                    Error::decode("blank line not allowed in keyed object"),
                ));
            }
            if line.level <= base_level {
                break;
            }
            if line.level != base_level + 1 {
                return Err(self.attach_location_for_line(
                    scan,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(self.line_content(line));
            if content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }
            let Some((key, cells)) = self.split_key_value(content)? else {
                if self.strict {
                    return Err(self.attach_location_for_slice(
                        scan,
                        content,
                        Error::decode("entry row missing ':'"),
                    ));
                }
                idx += 1;
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
                    scan,
                    content,
                    Error::decode("tabular row field count mismatch"),
                ));
            }
            let mut values = SmallVec::<[usize; 16]>::new();
            for token in tokens {
                values.push(if token.is_empty() {
                    self.empty_string_node()
                } else if token == "[]" {
                    let span = self.span_for(token);
                    self.push_string(StringRef::Span(span))
                } else {
                    self.parse_value_token_trimmed(token)?
                });
            }
            let mut cursor = 0;
            let value = self.build_field_object(fields, &values, &mut cursor);
            let key_id = self.intern_key(&key.value);
            insert_pair(&mut pairs, &mut pair_index, key_id, value, self.strict)?;
            entries += 1;
            idx += 1;
        }
        if self.strict && entries != expected_len {
            return Err(Error::decode("array length mismatch"));
        }
        Ok((self.push_object(&pairs), idx))
    }

    fn parse_tabular_block(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        base_level: usize,
        fields: &[FieldEntry],
        delimiter: char,
        _expected_len: usize,
    ) -> Result<(Vec<usize>, usize)> {
        let mut rows = Vec::new();
        let mut tokens = TokenBuf::with_capacity(fields.len());
        let mut value_ids: SmallVec<[usize; 16]> = SmallVec::with_capacity(fields.len());
        let leaf_count: usize = fields.iter().map(FieldEntry::leaf_count).sum();
        let mut row_level = None;
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_comment {
                idx += 1;
                continue;
            }
            if line.is_blank {
                if !self.strict || rows.is_empty() {
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
                return Err(self.attach_location_for_line(
                    scan,
                    idx,
                    Error::decode("blank line not allowed in array"),
                ));
            }
            let level = line.level;
            if row_level.is_none() {
                if level <= base_level {
                    return Ok((rows, idx));
                }
                if self.strict && level != base_level + 1 {
                    return Err(self.attach_location_for_line(
                        scan,
                        idx,
                        Error::decode("unexpected indentation"),
                    ));
                }
                row_level = Some(level);
            }
            let row_level = row_level.unwrap();
            if level < row_level {
                return Ok((rows, idx));
            }
            if level > row_level {
                return Err(self.attach_location_for_line(
                    scan,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let row_content = trim_ascii(self.line_content(line));
            if delimiter != '\t' && row_content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }
            if !self
                .split_tabular_row_into(row_content, delimiter, &mut tokens)
                .map_err(|err| self.attach_location_for_slice(scan, row_content, err))?
            {
                return Ok((rows, idx));
            }
            if tokens.len() != leaf_count && self.strict {
                return Err(self.attach_location_for_slice(
                    scan,
                    row_content,
                    Error::decode("tabular row field count mismatch"),
                ));
            }
            value_ids.clear();
            for token in tokens.iter() {
                let value_id = if token.is_empty() {
                    self.empty_string_node()
                } else if *token == "[]" {
                    let span = self.span_for(token);
                    self.push_string(StringRef::Span(span))
                } else {
                    self.parse_value_token_trimmed(token)
                        .map_err(|err| self.attach_location_for_slice(scan, token, err))?
                };
                value_ids.push(value_id);
            }
            let mut cursor = 0;
            let row_node = self.build_field_object(fields, &value_ids, &mut cursor);
            rows.push(row_node);
            idx += 1;
        }
        Ok((rows, idx))
    }

    fn build_field_object(
        &mut self,
        fields: &[FieldEntry],
        values: &[usize],
        cursor: &mut usize,
    ) -> usize {
        let mut pairs = Vec::with_capacity(fields.len());
        let mut slots = HashMap::new();
        for field in fields {
            let value = if field.children.is_empty() {
                let Some(&value) = values.get(*cursor) else {
                    *cursor += 1;
                    continue;
                };
                *cursor += 1;
                value
            } else {
                self.build_field_object(&field.children, values, cursor)
            };
            let key = self.intern_key(&field.key.value);
            // Duplicate field names are rejected while parsing the header.
            insert_pair(&mut pairs, &mut slots, key, value, false)
                .expect("non-strict insertion cannot fail");
        }
        self.push_object(&pairs)
    }

    fn parse_list_block(
        &mut self,
        scan: &ScanResult,
        mut idx: usize,
        item_level: usize,
        _expected_len: usize,
    ) -> Result<(Vec<usize>, usize)> {
        let mut items = Vec::new();
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_comment {
                idx += 1;
                continue;
            }
            if line.is_blank {
                if !self.strict || items.is_empty() {
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
                return Err(self.attach_location_for_line(
                    scan,
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
                    scan,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(self.line_content(line));
            if content != "-" && !content.starts_with("- ") {
                return Err(self.attach_location_for_slice(
                    scan,
                    content,
                    Error::decode("expected list item"),
                ));
            }
            let item_content = if content == "-" {
                ""
            } else {
                trim_ascii(&content[2..])
            };
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
        if item_content == "[]" {
            return Ok((self.push_array(&[]), idx));
        }

        let header = match self.parse_array_header(item_content) {
            Ok(header) => header,
            Err(err) => {
                return Err(self.attach_location_for_slice(scan, item_content, err));
            }
        };
        if let Some(header) = header {
            if header.key.is_none() {
                if header.fields.is_some() {
                    return Err(self.attach_location_for_slice(
                        scan,
                        item_content,
                        Error::decode("fields-bearing header requires a key in a list item"),
                    ));
                }
                let parsed = self
                    .parse_array_from_header(&header, scan, idx, item_level)
                    .map_err(|err| self.attach_location_for_slice(scan, item_content, err))?;
                return Ok((parsed.node_id, parsed.next_idx));
            }
            let key = header.key.clone().ok_or_else(|| {
                self.attach_location_for_slice(
                    scan,
                    item_content,
                    Error::decode("array header missing key in object context"),
                )
            })?;
            let array_base_level = item_level + 1;
            let parsed = self
                .parse_array_from_header(&header, scan, idx, array_base_level)
                .map_err(|err| self.attach_location_for_slice(scan, item_content, err))?;
            let mut pairs = Vec::new();
            let mut pair_index = HashMap::new();
            let key_id = self.intern_key(&key.value);
            insert_pair(
                &mut pairs,
                &mut pair_index,
                key_id,
                parsed.node_id,
                self.strict,
            )?;
            let next_idx = self
                .parse_object_block_into(
                    scan,
                    parsed.next_idx,
                    item_level + 1,
                    &mut pairs,
                    &mut pair_index,
                    true,
                )
                .map_err(|err| self.attach_location_for_slice(scan, item_content, err))?;
            let obj_node = self.push_object(&pairs);
            return Ok((obj_node, next_idx));
        }

        if self
            .split_key_value(item_content)
            .map_err(|err| self.attach_location_for_slice(scan, item_content, err))?
            .is_some()
        {
            return self
                .parse_object_item_from_line(item_content, scan, idx, item_level)
                .map_err(|err| self.attach_location_for_slice(scan, item_content, err));
        }

        let value = self
            .parse_value_token_trimmed(item_content)
            .map_err(|err| self.attach_location_for_slice(scan, item_content, err))?;
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
        if let Some((key, value)) = self
            .split_key_value(first_content)
            .map_err(|err| self.attach_location_for_slice(scan, first_content, err))?
        {
            let key = self
                .parse_key_token(trim_ascii(key))
                .map_err(|err| self.attach_location_for_slice(scan, key, err))?;
            let key_id = self.intern_key(&key.value);
            if trim_ascii(value).is_empty() {
                let (nested, next_idx) = self
                    .parse_object_block(scan, idx, base_level + 1)
                    .map_err(|err| self.attach_location_for_slice(scan, first_content, err))?;
                insert_pair(&mut pairs, &mut pair_index, key_id, nested, self.strict)?;
                idx = next_idx;
            } else {
                let value_trimmed = trim_ascii(value);
                let value_id = self
                    .parse_value_token(value)
                    .map_err(|err| self.attach_location_for_slice(scan, value_trimmed, err))?;
                insert_pair(&mut pairs, &mut pair_index, key_id, value_id, self.strict)?;
            }
        }
        let next_idx = self
            .parse_object_block_into(scan, idx, base_level, &mut pairs, &mut pair_index, true)
            .map_err(|err| self.attach_location_for_slice(scan, first_content, err))?;
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
        let next_idx = self.parse_object_block_into(
            scan,
            idx,
            base_level,
            &mut pairs,
            &mut pair_index,
            false,
        )?;
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
        reject_internal_blank: bool,
    ) -> Result<usize> {
        while idx < scan.lines.len() {
            let line = &scan.lines[idx];
            if line.is_comment {
                idx += 1;
                continue;
            }
            if line.is_blank {
                if self.strict && reject_internal_blank && !pairs.is_empty() {
                    let mut peek = idx + 1;
                    while peek < scan.lines.len() && scan.lines[peek].is_blank {
                        peek += 1;
                    }
                    if peek < scan.lines.len() && scan.lines[peek].level >= base_level {
                        return Err(self.attach_location_for_line(
                            scan,
                            idx,
                            Error::decode("blank line not allowed inside list item"),
                        ));
                    }
                }
                idx += 1;
                continue;
            }
            let level = line.level;
            if level < base_level {
                break;
            }
            if level > base_level {
                return Err(self.attach_location_for_line(
                    scan,
                    idx,
                    Error::decode("unexpected indentation"),
                ));
            }
            let content = trim_ascii(self.line_content(line));
            if content.starts_with('\t') {
                return Err(Error::decode("tabs not allowed in indentation"));
            }

            let header = match self.parse_array_header(content) {
                Ok(header) => header,
                Err(err) => {
                    return Err(self.attach_location_for_slice(scan, content, err));
                }
            };
            if let Some(header) = header {
                let key = header.key.as_ref().ok_or_else(|| {
                    self.attach_location_for_slice(
                        scan,
                        content,
                        Error::decode("array header missing key in object context"),
                    )
                })?;
                let parsed = self
                    .parse_array_from_header(&header, scan, idx + 1, base_level)
                    .map_err(|err| self.attach_location_for_slice(scan, content, err))?;
                let key_id = self.intern_key(&key.value);
                insert_pair(pairs, pair_index, key_id, parsed.node_id, self.strict)?;
                idx = parsed.next_idx;
                continue;
            }

            if let Some((key, value)) = self
                .split_key_value(content)
                .map_err(|err| self.attach_location_for_slice(scan, content, err))?
            {
                let key = self
                    .parse_key_token(trim_ascii(key))
                    .map_err(|err| self.attach_location_for_slice(scan, key, err))?;
                let key_id = self.intern_key(&key.value);
                if trim_ascii(value).is_empty() {
                    let (nested, next_idx) = self
                        .parse_object_block(scan, idx + 1, base_level + 1)
                        .map_err(|err| self.attach_location_for_slice(scan, content, err))?;
                    insert_pair(pairs, pair_index, key_id, nested, self.strict)?;
                    idx = next_idx;
                } else {
                    let value_trimmed = trim_ascii(value);
                    let value_id = self
                        .parse_value_token(value)
                        .map_err(|err| self.attach_location_for_slice(scan, value_trimmed, err))?;
                    insert_pair(pairs, pair_index, key_id, value_id, self.strict)?;
                    idx += 1;
                }
                continue;
            }

            return Err(self.attach_location_for_slice(
                scan,
                content,
                Error::decode("object field missing ':'"),
            ));
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
        if token == "[]" {
            return Ok(self.push_array(&[]));
        }
        match token {
            "null" => return Ok(self.null_node()),
            "true" => return Ok(self.push_bool(true)),
            "false" => return Ok(self.push_bool(false)),
            _ => {}
        }
        let number = parse_number_token(token);
        if number.is_some() {
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
            Ok(KeyToken { value })
        } else {
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
        if bytes.iter().any(|&byte| byte < 0x20 && byte != b'\t') {
            return Err(Error::decode("literal control character in quoted string"));
        }
        if memchr(b'\\', bytes).is_none() {
            if memchr(b'"', bytes).is_some() {
                return Err(Error::decode("unexpected content after quoted token"));
            }
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
        match self.parse_array_header_inner(line) {
            Ok(header) => Ok(header),
            Err(err)
                if !self.strict
                    && matches!(
                        err.message.as_str(),
                        "array length missing"
                            | "invalid array length"
                            | "invalid array delimiter"
                            | "invalid array header suffix"
                            | "unterminated array header"
                            | "unterminated field list"
                            | "unmatched field brace"
                            | "empty field name"
                            | "invalid field name"
                            | "field delimiter mismatch"
                            | "keyed header requires fields"
                    ) =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    fn parse_array_header_inner(&self, line: &'a str) -> Result<Option<HeaderLine<'a>>> {
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
        if digits_end > 1 && inner.starts_with('0') {
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
            _ => return Err(Error::decode("invalid array delimiter")),
        };

        let mut rest = &line[bracket_end + 1..];
        if rest.starts_with(char::is_whitespace) {
            return Err(Error::decode("invalid array header suffix"));
        }
        let mut fields = None;
        if rest.starts_with('{') {
            let end =
                matching_brace(rest).ok_or_else(|| Error::decode("unterminated field list"))?;
            let field_segment = &rest[1..end];
            if b",|\t".iter().copied().any(|candidate| {
                candidate != delimiter as u8 && first_unquoted(field_segment, candidate).is_some()
            }) {
                return Err(Error::decode("field delimiter mismatch"));
            }
            fields = Some(self.parse_field_entries(field_segment, delimiter)?);
            rest = &rest[end + 1..];
        }
        if keyed && fields.is_none() {
            return Err(Error::decode("keyed header requires fields"));
        }

        let Some(after_colon) = rest.strip_prefix(':') else {
            return Err(Error::decode(if rest.contains(':') {
                "invalid array header suffix"
            } else {
                "array header missing ':'"
            }));
        };
        let inline = trim_ascii(after_colon);
        let inline = if inline.is_empty() {
            None
        } else {
            Some(inline)
        };
        if !self.strict && fields.is_some() && inline.is_some() {
            return Ok(None);
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

    fn parse_field_entries(&self, input: &'a str, delimiter: char) -> Result<Vec<FieldEntry>> {
        let mut result = Vec::new();
        let mut names = HashMap::<SmolStr, ()>::new();
        for token in split_top_level(input, delimiter)? {
            let token = trim_ascii(token);
            if token.is_empty() {
                return Err(Error::decode("empty field name"));
            }
            let brace = first_unquoted(token, b'{');
            let (name, children) = if let Some(start) = brace {
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

    fn split_delimited<'c>(&self, input: &'c str, delimiter: char) -> Result<TokenBuf<'c>> {
        self.split_delimited_with_capacity(input, delimiter, 0)
    }

    fn split_delimited_with_capacity<'c>(
        &self,
        input: &'c str,
        delimiter: char,
        expected_len: usize,
    ) -> Result<TokenBuf<'c>> {
        let _ = expected_len;
        let mut tokens = TokenBuf::new();
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

    fn reject_blanks_in_header_spans(&self, scan: &ScanResult) -> Result<()> {
        if !self.strict {
            return Ok(());
        }
        for (header_idx, line) in scan.lines.iter().enumerate() {
            if line.is_blank {
                continue;
            }
            let content = trim_ascii(self.line_content(line));
            let (candidate, header_level) = if let Some(rest) = content.strip_prefix("- ") {
                // A keyed field carried by a list marker stands one level
                // deeper than the marker for scope purposes (§10).
                (rest, line.level + usize::from(!rest.starts_with('[')))
            } else {
                (content, line.level)
            };
            let Ok(Some(header)) = self.parse_array_header(candidate) else {
                continue;
            };
            if header.inline.is_some() {
                continue;
            }

            let mut started = false;
            for (idx, following) in scan.lines.iter().enumerate().skip(header_idx + 1) {
                if following.is_comment {
                    continue;
                }
                if following.is_blank {
                    if started {
                        let next_content =
                            scan.lines.iter().skip(idx + 1).find(|next| !next.is_blank);
                        if next_content.is_none_or(|next| next.level <= header_level) {
                            break;
                        }
                        return Err(self.attach_location_for_line(
                            scan,
                            idx,
                            Error::decode("blank line not allowed inside array header span"),
                        ));
                    }
                    continue;
                }
                if following.level <= header_level {
                    break;
                }
                started = true;
            }
        }
        Ok(())
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

    fn location_from_offset(&self, scan: &ScanResult, offset: usize) -> Option<Location> {
        for (idx, line) in scan.lines.iter().enumerate() {
            if offset <= line.end {
                let column = offset.saturating_sub(line.raw_start);
                return Some(Location {
                    offset,
                    line: idx + 1,
                    column: column + 1,
                });
            }
        }
        None
    }

    fn attach_location_from_offset(&self, scan: &ScanResult, offset: usize, err: Error) -> Error {
        if err.location.is_some() {
            return err;
        }
        match self.location_from_offset(scan, offset) {
            Some(location) => err.with_location(location),
            None => err,
        }
    }

    fn attach_location_for_line(&self, scan: &ScanResult, line_idx: usize, err: Error) -> Error {
        let offset = scan
            .lines
            .get(line_idx)
            .map(|line| line.raw_start)
            .unwrap_or(0);
        self.attach_location_from_offset(scan, offset, err)
    }

    fn attach_location_for_slice(&self, scan: &ScanResult, slice: &str, err: Error) -> Error {
        let offset = self.span_for(slice).start;
        self.attach_location_from_offset(scan, offset, err)
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
    keyed: bool,
    fields: Option<Vec<FieldEntry>>,
    inline: Option<&'a str>,
}

struct FieldEntry {
    key: KeyToken,
    children: Vec<FieldEntry>,
}

impl FieldEntry {
    fn leaf_count(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            self.children.iter().map(Self::leaf_count).sum()
        }
    }
}

struct ParsedArray {
    node_id: usize,
    next_idx: usize,
}

fn insert_pair(
    pairs: &mut Vec<Pair>,
    pair_index: &mut HashMap<usize, usize>,
    key: usize,
    value: usize,
    reject_duplicate: bool,
) -> Result<()> {
    if let Some(&idx) = pair_index.get(&key) {
        if reject_duplicate {
            return Err(Error::decode("duplicate key"));
        }
        pairs[idx].value = value;
        return Ok(());
    }
    let idx = pairs.len();
    pairs.push(Pair { key, value });
    pair_index.insert(key, idx);
    Ok(())
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
