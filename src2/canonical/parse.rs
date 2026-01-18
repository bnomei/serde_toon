//! Stage-2 parse for canonical input.

use super::{
    arena::{Arena, ArenaView, Node, NodeData, NodeKind, Pair, Span, StringRef},
    scan::{LineKind, ScanResult},
    validate::{
        is_canonical_number, is_canonical_unquoted_key, is_canonical_unquoted_string,
        is_numeric_like,
    },
};

use crate::parallel::decode::{block_spans_from, map_spans_parallel, BlockSpan};
use serde_json::Value;
use std::sync::OnceLock;

const PARALLEL_MIN_FIELDS: usize = 64;
const PARALLEL_MIN_ITEMS: usize = 256;
const PARALLEL_MIN_LINES: usize = 1024;
const PARALLEL_MIN_AVG_LINES: usize = 32;
const DOC_DELIM: u8 = b',';
const TABULAR_MIN_ROWS: usize = 2;

fn should_parallelize_spans(total_lines: usize, spans_len: usize, min_spans: usize) -> bool {
    if spans_len < min_spans {
        return false;
    }
    if total_lines < PARALLEL_MIN_LINES {
        return false;
    }
    let avg_lines = total_lines / spans_len;
    avg_lines >= PARALLEL_MIN_AVG_LINES
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayForm {
    Inline,
    List,
    Tabular,
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok().and_then(|value| value.parse().ok())
}

fn min_tabular_rows() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| env_usize("TOON_TABULAR_MIN_ROWS").unwrap_or(TABULAR_MIN_ROWS))
}

pub fn parse(_scan: &ScanResult) -> Arena {
    Arena::new()
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }
}

pub fn parse_view<'a>(_input: &'a str, _scan: &ScanResult) -> Result<ArenaView<'a>, ParseError> {
    let mut arena = ArenaView::new(_input);

    if _scan.lines.is_empty() {
        arena.nodes.push(Node {
            kind: NodeKind::Object,
            first_child: arena.pairs.len(),
            child_len: 0,
            data: NodeData::None,
        });
        return Ok(arena);
    }

    let mut root_index = 0;
    while root_index < _scan.lines.len() && _scan.lines[root_index].kind == LineKind::Blank {
        root_index += 1;
    }
    if root_index >= _scan.lines.len() {
        arena.nodes.push(Node {
            kind: NodeKind::Object,
            first_child: arena.pairs.len(),
            child_len: 0,
            data: NodeData::None,
        });
        return Ok(arena);
    }
    let root_line = &_scan.lines[root_index];
    let content_start = root_line.start + root_line.indent;
    let total_lines = _scan.lines.len();
    if matches!(
        root_line.kind,
        LineKind::ArrayItem | LineKind::EmptyObjectItem
    ) {
        return Err(ParseError::new(1, 1, "root array must use header"));
    }

    if root_line.kind == LineKind::KeyValue {
        if _input.as_bytes().get(content_start) == Some(&b'[') {
            if let Some((key_span, value_span)) =
                split_key_value(_input, content_start, root_line.end, root_index, root_line.start)?
            {
                if let Some((key_name, header)) =
                    parse_array_header(_input, key_span, root_index, root_line.start)?
                {
                    if key_name.start == key_name.end {
                        let root_form = if !header.fields.is_empty() {
                            ArrayForm::Tabular
                        } else if value_span.start != value_span.end {
                            ArrayForm::Inline
                        } else {
                            ArrayForm::List
                        };
                        let (root_index, next_i) = parse_array_value(
                            &mut arena,
                            _input,
                            _scan,
                            root_index + 1,
                            root_line.indent,
                            value_span,
                            header,
                            root_index,
                        )?;
                        let mut end_i = next_i;
                        while end_i < total_lines && _scan.lines[end_i].kind == LineKind::Blank {
                            end_i += 1;
                        }
                        if end_i != total_lines {
                            return Err(ParseError::new(
                                end_i + 1,
                                1,
                                "multiple root values not allowed",
                            ));
                        }
                        let expected_form = canonical_array_form_arena(&arena, root_index);
                        enforce_root_array_form(expected_form, root_form, 0)?;
                        return Ok(arena);
                    }
                }
            }
        }

        if split_key_value(_input, content_start, root_line.end, root_index, root_line.start)?
            .is_none()
        {
            let span = trim_line_span(_input, root_line.start, root_line.end);
            let node =
                parse_primitive_node(&mut arena, _input, span, root_index, root_line.start, DOC_DELIM)?;
            arena.nodes.push(node);
            let mut end_i = root_index + 1;
            while end_i < total_lines && _scan.lines[end_i].kind == LineKind::Blank {
                end_i += 1;
            }
            if end_i != total_lines {
                return Err(ParseError::new(
                    end_i + 1,
                    1,
                    "multiple root values not allowed",
                ));
            }
            return Ok(arena);
        }
    }

    let base_indent = _scan.lines[root_index].indent;
    let expected_array_len = None;
    let (_root_index, next_i) = parse_block(
        &mut arena,
        _input,
        _scan,
        root_index,
        base_indent,
        expected_array_len,
    )?;
    let mut end_i = next_i;
    while end_i < total_lines && _scan.lines[end_i].kind == LineKind::Blank {
        end_i += 1;
    }
    if end_i != total_lines {
        return Err(ParseError::new(
            end_i + 1,
            1,
            "multiple root values not allowed",
        ));
    }

    Ok(arena)
}

fn parse_array_item_block<'a>(
    input: &'a str,
    scan: &ScanResult,
    span: &BlockSpan,
) -> Result<(ArenaView<'a>, usize), ParseError> {
    let mut arena = ArenaView::new(input);
    let local_scan = ScanResult {
        lines: scan.lines[span.start..span.end].to_vec(),
    };
    if local_scan.lines.is_empty() {
        return Err(ParseError::new(span.start + 1, 1, "empty array item span"));
    }

    let (root_index, next_i) = parse_array_item(&mut arena, input, &local_scan, 0, span.indent)
        .map_err(|mut err| {
            err.line += span.start;
            err
        })?;
    if next_i != local_scan.lines.len() {
        return Err(ParseError::new(
            span.start + next_i + 1,
            1,
            "array item did not consume full span",
        ));
    }
    Ok((arena, root_index))
}

fn parse_object_field_block<'a>(
    input: &'a str,
    scan: &ScanResult,
    span: &BlockSpan,
) -> Result<(ArenaView<'a>, Pair), ParseError> {
    let mut arena = ArenaView::new(input);
    let local_scan = ScanResult {
        lines: scan.lines[span.start..span.end].to_vec(),
    };
    if local_scan.lines.is_empty() {
        return Err(ParseError::new(
            span.start + 1,
            1,
            "empty object field span",
        ));
    }

    let line = &local_scan.lines[0];
    if line.kind != LineKind::KeyValue {
        return Err(ParseError::new(span.start + 1, 1, "expected object field"));
    }

    let content_start = line.start + line.indent;
    let (key_span, value_span) =
        split_key_value(input, content_start, line.end, span.start, line.start)?
            .ok_or_else(|| ParseError::new(span.start + 1, 1, "invalid object field"))?;
    let (key_index, value_index, next_i) = parse_field_from_spans(
        &mut arena,
        input,
        &local_scan,
        1,
        span.indent,
        key_span,
        value_span,
        0,
    )
    .map_err(|mut err| {
        err.line += span.start;
        err
    })?;
    if next_i != local_scan.lines.len() {
        return Err(ParseError::new(
            span.start + next_i + 1,
            1,
            "object field did not consume full span",
        ));
    }

    Ok((
        arena,
        Pair {
            key: key_index,
            value: value_index,
        },
    ))
}

fn merge_arena<'a>(target: &mut ArenaView<'a>, local: &mut ArenaView<'a>) {
    let node_offset = target.nodes.len();
    let child_offset = target.children.len();
    let pair_offset = target.pairs.len();
    let string_offset = target.strings.len();
    let number_offset = target.numbers.len();

    target.strings.extend(local.strings.drain(..));
    target.numbers.extend(local.numbers.drain(..));

    for mut node in local.nodes.drain(..) {
        if node.child_len > 0 {
            match node.kind {
                NodeKind::Array => node.first_child += child_offset,
                NodeKind::Object => node.first_child += pair_offset,
                _ => {}
            }
        }
        match node.data {
            NodeData::String(index) => node.data = NodeData::String(index + string_offset),
            NodeData::Number(index) => node.data = NodeData::Number(index + number_offset),
            _ => {}
        }
        target.nodes.push(node);
    }

    for child in local.children.drain(..) {
        target.children.push(child + node_offset);
    }
    for pair in local.pairs.drain(..) {
        target.pairs.push(Pair {
            key: pair.key + string_offset,
            value: pair.value + node_offset,
        });
    }
}

fn merge_arena_with_root<'a>(
    target: &mut ArenaView<'a>,
    local: &mut ArenaView<'a>,
    root_index: usize,
) -> usize {
    let node_offset = target.nodes.len();
    merge_arena(target, local);
    node_offset + root_index
}

fn merge_arena_pair<'a>(target: &mut ArenaView<'a>, local: &mut ArenaView<'a>, pair: Pair) -> Pair {
    let node_offset = target.nodes.len();
    let string_offset = target.strings.len();
    merge_arena(target, local);
    Pair {
        key: pair.key + string_offset,
        value: pair.value + node_offset,
    }
}

fn parse_block<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    expected_array_len: Option<usize>,
) -> Result<(usize, usize), ParseError> {
    if start >= scan.lines.len() {
        return Ok((usize::MAX, start));
    }

    let mut i = start;
    let mut blank_index = None;
    while i < scan.lines.len() && scan.lines[i].kind == LineKind::Blank {
        if blank_index.is_none() {
            blank_index = Some(i);
        }
        i += 1;
    }
    if i >= scan.lines.len() {
        return Ok((usize::MAX, i));
    }
    if blank_index.is_some()
        && matches!(
            scan.lines[i].kind,
            LineKind::ArrayItem | LineKind::EmptyObjectItem
        )
    {
        return Err(ParseError::new(
            blank_index.unwrap() + 1,
            1,
            "blank line not allowed inside array",
        ));
    }

    match scan.lines[i].kind {
        LineKind::ArrayItem | LineKind::EmptyObjectItem => {
            parse_array_block(arena, input, scan, i, base_indent, expected_array_len)
        }
        LineKind::KeyValue => parse_object_block(arena, input, scan, i, base_indent, None),
        LineKind::Blank => Ok((usize::MAX, i)),
    }
}

fn parse_array_block<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    expected_len: Option<usize>,
) -> Result<(usize, usize), ParseError> {
    let node_index = arena.nodes.len();
    arena.nodes.push(Node {
        kind: NodeKind::Array,
        first_child: 0,
        child_len: 0,
        data: NodeData::None,
    });

    let inferred_len = expected_len.unwrap_or_else(|| count_array_items(scan, start, base_indent));
    let mut local_children = Vec::with_capacity(inferred_len);
    if inferred_len > 0 {
        arena.children.reserve(inferred_len);
        arena.nodes.reserve(inferred_len);
    }
    let mut i = start;
    while i < scan.lines.len() {
        let line = &scan.lines[i];
        if line.kind == LineKind::Blank {
            if blank_line_inside_array(scan, i, base_indent) {
                return Err(ParseError::new(i + 1, 1, "blank line not allowed inside array"));
            }
            break;
        }
        if line.indent != base_indent {
            break;
        }
        if !matches!(line.kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
            break;
        }

        let (child_index, next_i) = parse_array_item(arena, input, scan, i, base_indent)?;
        local_children.push(child_index);
        i = next_i;
    }

    let child_start = arena.children.len();
    let child_len = local_children.len();
    arena.children.extend(local_children);
    arena.nodes[node_index].first_child = child_start;
    arena.nodes[node_index].child_len = child_len;
    Ok((node_index, i))
}

fn count_array_items(scan: &ScanResult, start: usize, base_indent: usize) -> usize {
    let mut i = start;
    let mut count = 0;
    while i < scan.lines.len() {
        let line = &scan.lines[i];
        if line.indent != base_indent {
            break;
        }
        if !matches!(line.kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
            break;
        }
        count += 1;
        i += 1;
        while i < scan.lines.len() && scan.lines[i].indent > base_indent {
            i += 1;
        }
    }
    count
}

fn blank_line_inside_array(scan: &ScanResult, index: usize, list_indent: usize) -> bool {
    let mut i = index + 1;
    while i < scan.lines.len() && scan.lines[i].kind == LineKind::Blank {
        i += 1;
    }
    if i >= scan.lines.len() {
        return false;
    }
    scan.lines[i].indent >= list_indent
}

fn count_object_fields(scan: &ScanResult, start: usize, base_indent: usize) -> usize {
    let mut i = start;
    let mut count = 0;
    while i < scan.lines.len() {
        let line = &scan.lines[i];
        if line.kind == LineKind::Blank {
            i += 1;
            continue;
        }
        if line.indent != base_indent {
            break;
        }
        if line.kind != LineKind::KeyValue {
            break;
        }
        count += 1;
        i += 1;
        while i < scan.lines.len() && scan.lines[i].indent > base_indent {
            i += 1;
        }
    }
    count
}

fn parse_object_block<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    inline_field: Option<(Span, Span)>,
) -> Result<(usize, usize), ParseError> {
    let node_index = arena.nodes.len();
    arena.nodes.push(Node {
        kind: NodeKind::Object,
        first_child: 0,
        child_len: 0,
        data: NodeData::None,
    });

    let expected_fields =
        (inline_field.is_some() as usize) + count_object_fields(scan, start, base_indent);
    let mut local_pairs = Vec::with_capacity(expected_fields);
    if expected_fields > 0 {
        arena.pairs.reserve(expected_fields);
        arena.nodes.reserve(expected_fields);
    }
    let mut i = start;
    if let Some((key_span, value_span)) = inline_field {
        let header_line = start.saturating_sub(1);
        let (key_index, value_index, next_i) = parse_field_from_spans(
            arena,
            input,
            scan,
            start,
            base_indent,
            key_span,
            value_span,
            header_line,
        )?;
        local_pairs.push(Pair {
            key: key_index,
            value: value_index,
        });
        i = next_i;
    }

    let mut did_parallel = false;
    if i < scan.lines.len() {
        let mut has_blank = false;
        let mut check_i = i;
        while check_i < scan.lines.len() {
            let line = &scan.lines[check_i];
            if line.kind == LineKind::Blank {
                has_blank = true;
                check_i += 1;
                continue;
            }
            if line.indent != base_indent {
                break;
            }
            if line.kind != LineKind::KeyValue {
                break;
            }
            check_i += 1;
            while check_i < scan.lines.len() && scan.lines[check_i].indent > base_indent {
                check_i += 1;
            }
        }
        if !has_blank {
            let spans = block_spans_from(i, scan.lines.len(), base_indent, |idx| {
                scan.lines[idx].indent
            });
            let covered = spans.last().map(|span| span.end).unwrap_or(i);
            local_pairs.reserve(spans.len());
            if covered > i
                && should_parallelize_spans(covered - i, spans.len(), PARALLEL_MIN_FIELDS)
            {
                for span in &spans {
                    if scan.lines[span.start].kind != LineKind::KeyValue {
                        return Err(ParseError::new(span.start + 1, 1, "expected object field"));
                    }
                }
                let results =
                    map_spans_parallel(&spans, |span| parse_object_field_block(input, scan, span));
                for result in results {
                    let (mut local, pair) = result?;
                    let mapped = merge_arena_pair(arena, &mut local, pair);
                    local_pairs.push(mapped);
                }
                i = covered;
                did_parallel = true;
            }
        }
    }

    if !did_parallel {
        while i < scan.lines.len() {
            let line = &scan.lines[i];
            if line.kind == LineKind::Blank {
                i += 1;
                continue;
            }
            if line.indent != base_indent {
                break;
            }
            if line.kind != LineKind::KeyValue {
                break;
            }
            let content_start = line.start + line.indent;
            let (key_span, value_span) =
                split_key_value(input, content_start, line.end, i, line.start)?
                    .ok_or_else(|| ParseError::new(i + 1, 1, "missing ':' in object field"))?;
            let (key_index, value_index, next_i) = parse_field_from_spans(
                arena,
                input,
                scan,
                i + 1,
                base_indent,
                key_span,
                value_span,
                i,
            )?;
            local_pairs.push(Pair {
                key: key_index,
                value: value_index,
            });
            i = next_i;
        }
    }

    let pair_start = arena.pairs.len();
    let pair_len = local_pairs.len();
    arena.pairs.extend(local_pairs);
    arena.nodes[node_index].first_child = pair_start;
    arena.nodes[node_index].child_len = pair_len;
    Ok((node_index, i))
}

fn parse_value_or_nested<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    value_span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
) -> Result<(usize, usize), ParseError> {
    if value_span.start == value_span.end {
        return parse_nested_or_empty(arena, input, scan, next_index, base_indent + 2);
    }
    let node = parse_primitive_node(arena, input, value_span, line_index, line_start, delimiter)?;
    let value_index = arena.nodes.len();
    arena.nodes.push(node);
    Ok((value_index, next_index))
}

fn parse_nested_or_empty<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
) -> Result<(usize, usize), ParseError> {
    if start >= scan.lines.len() {
        return Ok(empty_object(arena, start));
    }
    let next_line = &scan.lines[start];
    if next_line.indent != base_indent {
        return Ok(empty_object(arena, start));
    }
    parse_block(arena, input, scan, start, base_indent, None)
}

fn empty_object<'a>(arena: &mut ArenaView<'a>, next_index: usize) -> (usize, usize) {
    let index = arena.nodes.len();
    arena.nodes.push(Node {
        kind: NodeKind::Object,
        first_child: arena.pairs.len(),
        child_len: 0,
        data: NodeData::None,
    });
    (index, next_index)
}

fn split_key_value<'a>(
    input: &'a str,
    start: usize,
    end: usize,
    line_index: usize,
    line_start: usize,
) -> Result<Option<(Span, Span)>, ParseError> {
    let bytes = input.as_bytes();
    let mut i = start;
    let mut in_quotes = false;
    while i < end {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b'\\' if in_quotes => {
                i += 2;
                continue;
            }
            b':' if !in_quotes => {
                if i == start {
                    return Err(ParseError::new(
                        line_index + 1,
                        i.saturating_sub(line_start) + 1,
                        "empty key",
                    ));
                }
                if start < end && bytes[start] == b' ' {
                    return Err(ParseError::new(
                        line_index + 1,
                        start.saturating_sub(line_start) + 1,
                        "unexpected whitespace before key",
                    ));
                }
                if bytes[i - 1] == b' ' {
                    return Err(ParseError::new(
                        line_index + 1,
                        i.saturating_sub(line_start),
                        "space before ':' is not canonical",
                    ));
                }
                if i + 1 < end {
                    if bytes[i + 1] != b' ' {
                        return Err(ParseError::new(
                            line_index + 1,
                            i.saturating_sub(line_start) + 2,
                            "expected single space after ':'",
                        ));
                    }
                    if i + 2 >= end {
                        return Err(ParseError::new(
                            line_index + 1,
                            i.saturating_sub(line_start) + 2,
                            "missing value after ':'",
                        ));
                    }
                    if bytes[i + 2] == b' ' {
                        return Err(ParseError::new(
                            line_index + 1,
                            i.saturating_sub(line_start) + 3,
                            "multiple spaces after ':'",
                        ));
                    }
                }
                let key_span = trim_line_span(input, start, i);
                let value_span = trim_line_span(input, i + 1, end);
                return Ok(Some((key_span, value_span)));
            }
            _ => {}
        }
        i += 1;
    }
    Ok(None)
}

fn reject_list_item_tabular_header(
    input: &str,
    scan: &ScanResult,
    line_index: usize,
) -> Result<(), ParseError> {
    let line = &scan.lines[line_index];
    let content_start = line.start + line.indent;
    let Some((key_span, _)) =
        split_key_value(input, content_start, line.end, line_index, line.start)?
    else {
        return Ok(());
    };
    if let Some((_key_name, header)) = parse_array_header(input, key_span, line_index, line.start)?
    {
        if !header.fields.is_empty() {
            let column = key_span.start.saturating_sub(line.start) + 1;
            return Err(ParseError::new(
                line_index + 1,
                column,
                "tabular array header must be on hyphen line",
            ));
        }
    }
    Ok(())
}

fn parse_array_item<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    index: usize,
    base_indent: usize,
) -> Result<(usize, usize), ParseError> {
    let line = &scan.lines[index];
    if line.kind == LineKind::EmptyObjectItem {
        if let Some(next_line) = scan.lines.get(index + 1) {
            if next_line.indent == base_indent + 2 && next_line.kind == LineKind::KeyValue {
                reject_list_item_tabular_header(input, scan, index + 1)?;
            }
        }
        return parse_nested_or_empty(arena, input, scan, index + 1, base_indent + 2);
    }
    let item_start = line.start + line.indent + 2;
    if let Some((key_span, value_span)) =
        split_key_value(input, item_start, line.end, index, line.start)?
    {
        return parse_object_block(
            arena,
            input,
            scan,
            index + 1,
            base_indent + 2,
            Some((key_span, value_span)),
        );
    }

    let value_span = trim_line_span(input, item_start, line.end);
    if value_span.start == value_span.end {
        if let Some(next_line) = scan.lines.get(index + 1) {
            if next_line.indent == base_indent + 2 && next_line.kind == LineKind::KeyValue {
                reject_list_item_tabular_header(input, scan, index + 1)?;
            }
        }
        return parse_nested_or_empty(arena, input, scan, index + 1, base_indent + 2);
    }

    let node = parse_primitive_node(arena, input, value_span, index, line.start, DOC_DELIM)?;
    let value_index = arena.nodes.len();
    arena.nodes.push(node);
    Ok((value_index, index + 1))
}

fn parse_field_from_spans<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    key_span: Span,
    value_span: Span,
    header_line: usize,
) -> Result<(usize, usize, usize), ParseError> {
    let key_line_start = scan
        .lines
        .get(header_line)
        .map(|line| line.start)
        .unwrap_or(key_span.start);
    let key_quoted = input.as_bytes().get(key_span.start) == Some(&b'"');
    if key_quoted {
        let bytes = input.as_bytes();
        let mut i = key_span.start + 1;
        let mut has_escape = false;
        while i < key_span.end {
            match bytes[i] {
                b'\\' => {
                    has_escape = true;
                    i += 2;
                }
                b'"' => break,
                _ => i += 1,
            }
        }
        if i >= key_span.end || bytes.get(i) != Some(&b'"') {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "unterminated quoted key",
            ));
        }
        let key_name_span = Span {
            start: key_span.start + 1,
            end: i,
        };
        let key_name = input
            .get(key_name_span.start..key_name_span.end)
            .unwrap_or("");
        if !has_escape && is_canonical_unquoted_key(key_name) {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "quoted key must be unquoted",
            ));
        }
    } else {
        let key_slice = input.get(key_span.start..key_span.end).unwrap_or("");
        let key_name = key_slice.split('[').next().unwrap_or(key_slice);
        if !is_canonical_unquoted_key(key_name) {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "unquoted key must be canonical",
            ));
        }
    }

    if let Some((key_name, header)) =
        parse_array_header(input, key_span, header_line, key_line_start)?
    {
        let key_index = push_string_from_span(arena, input, key_name);
        let (value_index, next_i) = parse_array_value(
            arena,
            input,
            scan,
            next_index,
            base_indent,
            value_span,
            header,
            header_line,
        )?;
        return Ok((key_index, value_index, next_i));
    }

    let key_index = push_string_from_span(arena, input, key_span);
    let line_start = scan
        .lines
        .get(header_line)
        .map(|line| line.start)
        .unwrap_or(value_span.start);
    let (value_index, next_i) = parse_value_or_nested(
        arena,
        input,
        scan,
        next_index,
        base_indent,
        value_span,
        header_line,
        line_start,
        DOC_DELIM,
    )?;
    Ok((key_index, value_index, next_i))
}

struct ArrayHeader {
    len: Option<usize>,
    fields: Vec<FieldSpan>,
    delimiter: u8,
}

#[derive(Clone, Copy)]
struct FieldSpan {
    span: Span,
    quoted: bool,
}

fn reject_unnecessary_quoted_header_field(
    input: &str,
    span: Span,
    line_index: usize,
    line_start: usize,
) -> Result<(), ParseError> {
    let raw = input.get(span.start..span.end).unwrap_or("");
    if !raw.as_bytes().contains(&b'\\') && is_canonical_unquoted_key(raw) {
        let column = span.start.saturating_sub(line_start) + 1;
        return Err(ParseError::new(
            line_index + 1,
            column,
            "quoted key must be unquoted",
        ));
    }
    Ok(())
}

fn parse_array_header(
    input: &str,
    key_span: Span,
    line_index: usize,
    line_start: usize,
) -> Result<Option<(Span, ArrayHeader)>, ParseError> {
    let bytes = input.as_bytes();
    let mut i = key_span.start;
    let mut key_end = key_span.end;
    let mut header_start = None;

    if i < key_span.end && bytes[i] == b'"' {
        i += 1;
        while i < key_span.end {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    key_end = i + 1;
                    header_start = if key_end < key_span.end && bytes[key_end] == b'[' {
                        Some(key_end)
                    } else {
                        None
                    };
                    break;
                }
                _ => i += 1,
            }
        }
    } else {
        while i < key_span.end {
            if bytes[i] == b'[' {
                header_start = Some(i);
                key_end = i;
                break;
            }
            i += 1;
        }
    }

    let header_start = match header_start {
        Some(start) => start,
        None => return Ok(None),
    };

    let mut j = header_start + 1;
    if j >= key_span.end {
        return Err(ParseError::new(
            line_index + 1,
            header_start.saturating_sub(line_start) + 1,
            "unterminated array header",
        ));
    }

    let mut len_value: usize = 0;
    let mut saw_digit = false;
    while j < key_span.end {
        let b = bytes[j];
        if b.is_ascii_digit() {
            saw_digit = true;
            len_value = len_value
                .saturating_mul(10)
                .saturating_add((b - b'0') as usize);
            j += 1;
        } else {
            break;
        }
    }

    if !saw_digit {
        return Err(ParseError::new(
            line_index + 1,
            j.saturating_sub(line_start) + 1,
            "array header must include length",
        ));
    }

    let mut delimiter = b',';
    if j >= key_span.end {
        return Err(ParseError::new(
            line_index + 1,
            j.saturating_sub(line_start) + 1,
            "unterminated array header",
        ));
    }

    match bytes[j] {
        b']' => {}
        b',' | b'|' | b'\t' => {
            delimiter = bytes[j];
            j += 1;
            if j >= key_span.end || bytes[j] != b']' {
                return Err(ParseError::new(
                    line_index + 1,
                    j.saturating_sub(line_start) + 1,
                    "invalid delimiter marker in array header",
                ));
            }
        }
        _ => {
            return Err(ParseError::new(
                line_index + 1,
                j.saturating_sub(line_start) + 1,
                "invalid array header syntax",
            ));
        }
    }

    let bracket_end = j;
    let mut fields = Vec::new();
    let mut k = bracket_end + 1;
    if k < key_span.end {
        if bytes[k] != b'{' {
            return Err(ParseError::new(
                line_index + 1,
                k.saturating_sub(line_start) + 1,
                "unexpected characters after array header",
            ));
        }
        k += 1;
        let mut brace_end = None;
        let mut in_quotes = false;
        while k < key_span.end {
            match bytes[k] {
                b'"' => in_quotes = !in_quotes,
                b'\\' if in_quotes => {
                    k += 2;
                    continue;
                }
                b'}' if !in_quotes => {
                    brace_end = Some(k);
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        let brace_end = brace_end.ok_or_else(|| {
            ParseError::new(
                line_index + 1,
                k.saturating_sub(line_start) + 1,
                "unterminated tabular header",
            )
        })?;

        fields = split_header_fields(
            input,
            bracket_end + 2,
            brace_end,
            delimiter,
            line_index,
            line_start,
        )?;

        if brace_end + 1 != key_span.end {
            return Err(ParseError::new(
                line_index + 1,
                (brace_end + 1).saturating_sub(line_start) + 1,
                "unexpected characters after tabular header",
            ));
        }
    } else if bracket_end + 1 != key_span.end {
        return Err(ParseError::new(
            line_index + 1,
            (bracket_end + 1).saturating_sub(line_start) + 1,
            "unexpected characters after array header",
        ));
    }

    let key_name = if bytes[key_span.start] == b'"' && key_end > key_span.start + 1 {
        Span {
            start: key_span.start + 1,
            end: key_end - 1,
        }
    } else {
        Span {
            start: key_span.start,
            end: key_end,
        }
    };

    Ok(Some((
        key_name,
        ArrayHeader {
            len: Some(len_value),
            fields,
            delimiter,
        },
    )))
}

fn split_header_fields(
    input: &str,
    start: usize,
    end: usize,
    delimiter: u8,
    line_index: usize,
    line_start: usize,
) -> Result<Vec<FieldSpan>, ParseError> {
    let bytes = input.as_bytes();
    let mut fields = Vec::new();
    let mut token_start = start;
    let mut i = start;
    let mut in_quotes = false;
    let mut quoted = false;
    let mut quote_start = None;
    let mut quote_end = None;

    while i < end {
        match bytes[i] {
            b'"' => {
                if !in_quotes {
                    if i != token_start {
                        return Err(ParseError::new(
                            line_index + 1,
                            i.saturating_sub(line_start) + 1,
                            "tabular header fields must not contain whitespace",
                        ));
                    }
                    in_quotes = true;
                    quoted = true;
                    quote_start = Some(i);
                } else {
                    in_quotes = false;
                    quote_end = Some(i);
                }
            }
            b'\\' if in_quotes => {
                i += 2;
                continue;
            }
            b' ' | b'\t' if !in_quotes && bytes[i] != delimiter => {
                return Err(ParseError::new(
                    line_index + 1,
                    i.saturating_sub(line_start) + 1,
                    "tabular header fields must not contain whitespace",
                ));
            }
            b if !in_quotes && (b == b',' || b == b'|' || b == b'\t') && b != delimiter => {
                return Err(ParseError::new(
                    line_index + 1,
                    i.saturating_sub(line_start) + 1,
                    "delimiter mismatch in tabular header",
                ));
            }
            b if !in_quotes && b == delimiter => {
                if i == token_start {
                    return Err(ParseError::new(
                        line_index + 1,
                        i.saturating_sub(line_start) + 1,
                        "empty tabular header field",
                    ));
                }
                let field = finish_header_field(
                    token_start,
                    i,
                    quoted,
                    quote_start,
                    quote_end,
                    line_index,
                    line_start,
                )?;
                fields.push(field);
                token_start = i + 1;
                quoted = false;
                quote_start = None;
                quote_end = None;
            }
            _ => {}
        }
        i += 1;
    }

    if in_quotes {
        return Err(ParseError::new(
            line_index + 1,
            end.saturating_sub(line_start) + 1,
            "unterminated quoted tabular field",
        ));
    }
    if token_start == end {
        return Err(ParseError::new(
            line_index + 1,
            end.saturating_sub(line_start) + 1,
            "empty tabular header field",
        ));
    }
    let field = finish_header_field(
        token_start,
        end,
        quoted,
        quote_start,
        quote_end,
        line_index,
        line_start,
    )?;
    fields.push(field);

    Ok(fields)
}

fn finish_header_field(
    start: usize,
    end: usize,
    quoted: bool,
    quote_start: Option<usize>,
    quote_end: Option<usize>,
    line_index: usize,
    line_start: usize,
) -> Result<FieldSpan, ParseError> {
    if quoted {
        let quote_start = quote_start.ok_or_else(|| {
            ParseError::new(
                line_index + 1,
                start.saturating_sub(line_start) + 1,
                "invalid quoted tabular field",
            )
        })?;
        let quote_end = quote_end.ok_or_else(|| {
            ParseError::new(
                line_index + 1,
                end.saturating_sub(line_start) + 1,
                "unterminated quoted tabular field",
            )
        })?;
        if quote_start != start || quote_end + 1 != end {
            return Err(ParseError::new(
                line_index + 1,
                start.saturating_sub(line_start) + 1,
                "unexpected characters around quoted tabular field",
            ));
        }
        return Ok(FieldSpan {
            span: Span {
                start: quote_start + 1,
                end: quote_end,
            },
            quoted: true,
        });
    }

    Ok(FieldSpan {
        span: Span { start, end },
        quoted: false,
    })
}

fn parse_array_value<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    value_span: Span,
    header: ArrayHeader,
    header_line: usize,
) -> Result<(usize, usize), ParseError> {
    let node_index = arena.nodes.len();
    arena.nodes.push(Node {
        kind: NodeKind::Array,
        first_child: 0,
        child_len: 0,
        data: NodeData::None,
    });
    let expected_len = header.len.unwrap_or(0);
    let mut local_children = Vec::with_capacity(expected_len);
    if expected_len > 0 {
        arena.children.reserve(expected_len);
    }

    if value_span.start != value_span.end {
        let line_start = scan
            .lines
            .get(header_line)
            .map(|line| line.start)
            .unwrap_or(value_span.start);
        if !header.fields.is_empty() {
            return Err(ParseError::new(
                header_line + 1,
                value_span.start.saturating_sub(line_start) + 1,
                "tabular arrays must use row form",
            ));
        }
        if expected_len > 0 {
            arena.nodes.reserve(expected_len);
        }
        for_each_inline_value(
            input,
            value_span,
            header_line,
            line_start,
            header.delimiter,
            |item| {
                let child = parse_primitive_node(
                    arena,
                    input,
                    item,
                    header_line,
                    line_start,
                    header.delimiter,
                )?;
                let child_index = arena.nodes.len();
                arena.nodes.push(child);
                local_children.push(child_index);
                Ok(())
            },
        )?;
        let child_start = arena.children.len();
        let child_len = local_children.len();
        arena.children.extend(local_children);
        arena.nodes[node_index].first_child = child_start;
        arena.nodes[node_index].child_len = child_len;
        if let Some(expected) = header.len {
            if expected != child_len {
                return Err(ParseError::new(
                    header_line + 1,
                    1,
                    "array length does not match header",
                ));
            }
        }
        return Ok((node_index, next_index));
    }

    let list_indent = base_indent + 2;

    if next_index < scan.lines.len() {
        let next_line = &scan.lines[next_index];
        if next_line.kind == LineKind::Blank
            && blank_line_inside_array(scan, next_index, list_indent)
        {
            return Err(ParseError::new(
                next_index + 1,
                1,
                "blank line not allowed inside array",
            ));
        }
        if next_line.indent == list_indent
            && matches!(
                next_line.kind,
                LineKind::ArrayItem | LineKind::EmptyObjectItem
            )
        {
            let spans = block_spans_from(next_index, scan.lines.len(), list_indent, |idx| {
                scan.lines[idx].indent
            });
            let covered = spans.last().map(|span| span.end).unwrap_or(next_index);
            local_children.reserve(spans.len());
            if covered > next_index
                && should_parallelize_spans(covered - next_index, spans.len(), PARALLEL_MIN_ITEMS)
            {
                for span in &spans {
                    let kind = scan.lines[span.start].kind;
                    if !matches!(kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
                        return Err(ParseError::new(span.start + 1, 1, "expected array item"));
                    }
                }
                let results =
                    map_spans_parallel(&spans, |span| parse_array_item_block(input, scan, span));
                for result in results {
                    let (mut local, root_index) = result?;
                    let mapped = merge_arena_with_root(arena, &mut local, root_index);
                    local_children.push(mapped);
                }
                let child_start = arena.children.len();
                let child_len = local_children.len();
                arena.children.extend(local_children);
                arena.nodes[node_index].first_child = child_start;
                arena.nodes[node_index].child_len = child_len;
                if let Some(expected) = header.len {
                    if expected != child_len {
                        return Err(ParseError::new(
                            header_line + 1,
                            1,
                            "array length does not match header",
                        ));
                    }
                }
                return Ok((node_index, covered));
            }

            let mut i = next_index;
            while i < scan.lines.len() {
                let line = &scan.lines[i];
                if line.kind == LineKind::Blank {
                    if blank_line_inside_array(scan, i, list_indent) {
                        return Err(ParseError::new(
                            i + 1,
                            1,
                            "blank line not allowed inside array",
                        ));
                    }
                    break;
                }
                if line.indent != list_indent {
                    break;
                }
                if !matches!(line.kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
                    break;
                }
                let (child_index, next_i) = parse_array_item(arena, input, scan, i, list_indent)?;
                local_children.push(child_index);
                i = next_i;
            }
            let child_start = arena.children.len();
            let child_len = local_children.len();
            arena.children.extend(local_children);
            arena.nodes[node_index].first_child = child_start;
            arena.nodes[node_index].child_len = child_len;
            if let Some(expected) = header.len {
                if expected != child_len {
                    return Err(ParseError::new(
                        header_line + 1,
                        1,
                        "array length does not match header",
                    ));
                }
            }
            return Ok((node_index, i));
        }
    }

    if !header.fields.is_empty() {
        let header_line_start = scan
            .lines
            .get(header_line)
            .map(|line| line.start)
            .unwrap_or(value_span.start);
        let mut field_indices = Vec::with_capacity(header.fields.len());
        for field in header.fields.iter() {
            if field.quoted {
                reject_unnecessary_quoted_header_field(
                    input,
                    field.span,
                    header_line,
                    header_line_start,
                )?;
                let index = parse_quoted_string(
                    arena,
                    input,
                    field.span,
                    header_line,
                    header_line_start,
                    header.delimiter,
                    true,
                )?;
                field_indices.push(index);
            } else {
                let field_name = input.get(field.span.start..field.span.end).unwrap_or("");
                if !is_canonical_unquoted_key(field_name) {
                    return Err(ParseError::new(
                        header_line + 1,
                        field.span.start.saturating_sub(header_line_start) + 1,
                        "tabular field name must be canonical",
                    ));
                }
                field_indices.push(push_string_from_span(arena, input, field.span));
            }
        }
        let field_count = field_indices.len();
        let prealloc_rows = header.len.unwrap_or(0);
        let prealloc_pairs_start = arena.pairs.len();
        if prealloc_rows > 0 && field_count > 0 {
            arena.pairs.resize(
                prealloc_pairs_start + prealloc_rows * field_count,
                Pair { key: 0, value: 0 },
            );
        }
        if expected_len > 0 {
            let per_row = field_count.saturating_add(1);
            arena.nodes.reserve(expected_len.saturating_mul(per_row));
            arena
                .pairs
                .reserve(expected_len.saturating_mul(field_count));
        }
        let mut i = next_index;
        let mut row_count = 0;
        while i < scan.lines.len() {
            let line = &scan.lines[i];
            if line.kind == LineKind::Blank {
                if blank_line_inside_array(scan, i, list_indent) {
                    return Err(ParseError::new(
                        i + 1,
                        1,
                        "blank line not allowed inside array",
                    ));
                }
                break;
            }
            if line.indent != list_indent {
                break;
            }
            let row_span = trim_line_span(input, line.start, line.end);
            let use_prealloc = prealloc_rows > 0 && row_count < prealloc_rows;
            let pair_start = if use_prealloc {
                prealloc_pairs_start + row_count * field_count
            } else {
                arena.pairs.len()
            };
            let obj_index = arena.nodes.len();
            arena.nodes.push(Node {
                kind: NodeKind::Object,
                first_child: pair_start,
                child_len: field_count,
                data: NodeData::None,
            });
            let mut col_index = 0;
            for_each_inline_value(
                input,
                row_span,
                i,
                line.start,
                header.delimiter,
                |value_span| {
                    if col_index >= field_count {
                        return Err(ParseError::new(
                            i + 1,
                            1,
                            "tabular row field count mismatch",
                        ));
                    }
                    let value_node = parse_primitive_node(
                        arena,
                        input,
                        value_span,
                        i,
                        line.start,
                        header.delimiter,
                    )?;
                    let value_index = arena.nodes.len();
                    arena.nodes.push(value_node);
                    let pair = Pair {
                        key: field_indices[col_index],
                        value: value_index,
                    };
                    if use_prealloc {
                        arena.pairs[pair_start + col_index] = pair;
                    } else {
                        arena.pairs.push(pair);
                    }
                    col_index += 1;
                    Ok(())
                },
            )?;
            if col_index != field_count {
                return Err(ParseError::new(
                    i + 1,
                    1,
                    "tabular row field count mismatch",
                ));
            }
            local_children.push(obj_index);
            row_count += 1;
            i += 1;
        }
        let child_start = arena.children.len();
        let child_len = local_children.len();
        arena.children.extend(local_children);
        arena.nodes[node_index].first_child = child_start;
        arena.nodes[node_index].child_len = child_len;
        if let Some(expected) = header.len {
            if expected != child_len {
                return Err(ParseError::new(
                    header_line + 1,
                    1,
                    "array length does not match header",
                ));
            }
        }
        return Ok((node_index, i));
    }

    let child_start = arena.children.len();
    let child_len = local_children.len();
    arena.children.extend(local_children);
    arena.nodes[node_index].first_child = child_start;
    arena.nodes[node_index].child_len = child_len;
    if let Some(expected) = header.len {
        if expected != child_len {
            return Err(ParseError::new(
                header_line + 1,
                1,
                "array length does not match header",
            ));
        }
    }
    Ok((node_index, next_index))
}

fn for_each_inline_value<F>(
    input: &str,
    span: Span,
    _line_index: usize,
    _line_start: usize,
    delimiter: u8,
    mut on_value: F,
) -> Result<usize, ParseError>
where
    F: FnMut(Span) -> Result<(), ParseError>,
{
    let bytes = input.as_bytes();
    let mut count = 0;
    let mut in_quotes = false;
    let mut i = span.start;
    let mut field_start = span.start;
    while i < span.end {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b'\\' if in_quotes => {
                i += 2;
                continue;
            }
            b if !in_quotes && b == delimiter => {
                let value = trim_line_span(input, field_start, i);
                on_value(value)?;
                count += 1;
                i += 1;
                field_start = i;
                while field_start < span.end && bytes[field_start].is_ascii_whitespace() {
                    field_start += 1;
                }
                i = field_start;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if field_start <= span.end {
        let value = trim_line_span(input, field_start, span.end);
        on_value(value)?;
        count += 1;
    }
    Ok(count)
}

fn parse_primitive_node<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
) -> Result<Node, ParseError> {
    let raw = input.get(span.start..span.end).unwrap_or("");
    if raw.is_empty() {
        let index = push_string_span(arena, span);
        return Ok(Node {
            kind: NodeKind::String,
            first_child: 0,
            child_len: 0,
            data: NodeData::String(index),
        });
    }
    if raw == "null" {
        return Ok(Node {
            kind: NodeKind::Null,
            first_child: 0,
            child_len: 0,
            data: NodeData::None,
        });
    }
    if raw == "true" {
        return Ok(Node {
            kind: NodeKind::Bool,
            first_child: 0,
            child_len: 0,
            data: NodeData::Bool(true),
        });
    }
    if raw == "false" {
        return Ok(Node {
            kind: NodeKind::Bool,
            first_child: 0,
            child_len: 0,
            data: NodeData::Bool(false),
        });
    }

    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        let inner_span = Span {
            start: span.start + 1,
            end: span.end - 1,
        };
        let index = parse_quoted_string(
            arena, input, inner_span, line_index, line_start, delimiter, false,
        )?;
        return Ok(Node {
            kind: NodeKind::String,
            first_child: 0,
            child_len: 0,
            data: NodeData::String(index),
        });
    }

    if is_numeric_like(raw) && has_forbidden_leading_zero(raw) {
        let index = push_string_span(arena, span);
        return Ok(Node {
            kind: NodeKind::String,
            first_child: 0,
            child_len: 0,
            data: NodeData::String(index),
        });
    }

    if is_canonical_number(raw) || is_numeric_like(raw) {
        let column = span.start.saturating_sub(line_start) + 1;
        parse_number_value(raw, line_index, column)?;
        let index = push_number_span(arena, span);
        return Ok(Node {
            kind: NodeKind::Number,
            first_child: 0,
            child_len: 0,
            data: NodeData::Number(index),
        });
    }

    if !is_canonical_unquoted_string(raw, delimiter as char) {
        let column = span.start.saturating_sub(line_start) + 1;
        return Err(ParseError::new(
            line_index + 1,
            column,
            "unquoted string must be quoted",
        ));
    }

    let index = push_string_from_span(arena, input, span);
    Ok(Node {
        kind: NodeKind::String,
        first_child: 0,
        child_len: 0,
        data: NodeData::String(index),
    })
}

fn parse_quoted_string<'a>(
    arena: &mut ArenaView<'a>,
    input: &'a str,
    span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
    allow_unquoted: bool,
) -> Result<usize, ParseError> {
    let bytes = input.as_bytes();
    let mut i = span.start;
    let mut last = span.start;
    let mut out = String::new();
    let mut has_escape = false;

    while i < span.end {
        match bytes[i] {
            b'\\' => {
                if i + 1 >= span.end {
                    let column = i.saturating_sub(line_start) + 1;
                    return Err(ParseError::new(
                        line_index + 1,
                        column,
                        "unterminated escape sequence",
                    ));
                }
                let esc = bytes[i + 1];
                let ch = match esc {
                    b'\\' => '\\',
                    b'"' => '"',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => {
                        let column = i.saturating_sub(line_start) + 1;
                        return Err(ParseError::new(
                            line_index + 1,
                            column,
                            "invalid escape sequence",
                        ));
                    }
                };
                if !has_escape {
                    out.push_str(input.get(last..i).unwrap_or(""));
                } else if last < i {
                    out.push_str(input.get(last..i).unwrap_or(""));
                }
                out.push(ch);
                has_escape = true;
                i += 2;
                last = i;
            }
            b'"' => {
                let column = i.saturating_sub(line_start) + 1;
                return Err(ParseError::new(
                    line_index + 1,
                    column,
                    "unescaped quote in string",
                ));
            }
            _ => i += 1,
        }
    }

    if has_escape {
        if last < span.end {
            out.push_str(input.get(last..span.end).unwrap_or(""));
        }
        let index = arena.strings.len();
        arena.strings.push(StringRef::Owned(out));
        return Ok(index);
    }

    let raw = input.get(span.start..span.end).unwrap_or("");
    if !allow_unquoted && is_canonical_unquoted_string(raw, delimiter as char) {
        let column = span.start.saturating_sub(line_start) + 1;
        return Err(ParseError::new(
            line_index + 1,
            column,
            "string must be unquoted",
        ));
    }
    Ok(push_string_span(arena, span))
}

fn trim_line_span(input: &str, start: usize, end: usize) -> Span {
    let bytes = input.as_bytes();
    let mut s = start;
    let mut e = end;
    while s < e && bytes[s] == b' ' {
        s += 1;
    }
    while e > s && bytes[e - 1] == b' ' {
        e -= 1;
    }
    Span { start: s, end: e }
}

fn push_string_span<'a>(arena: &mut ArenaView<'a>, span: Span) -> usize {
    let index = arena.strings.len();
    arena.strings.push(StringRef::Span(span));
    index
}

fn push_number_span<'a>(arena: &mut ArenaView<'a>, span: Span) -> usize {
    let index = arena.numbers.len();
    arena.numbers.push(span);
    index
}

fn push_string_from_span<'a>(arena: &mut ArenaView<'a>, input: &'a str, span: Span) -> usize {
    if span.start >= span.end {
        return push_string_span(arena, span);
    }
    if !input.as_bytes()[span.start..span.end].contains(&b'\\') {
        return push_string_span(arena, span);
    }

    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut i = span.start;
    while i < span.end {
        match bytes[i] {
            b'\\' => {
                if i + 1 >= span.end {
                    break;
                }
                match bytes[i + 1] {
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    other => out.push(other as char),
                }
                i += 2;
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }

    let index = arena.strings.len();
    arena.strings.push(StringRef::Owned(out));
    index
}

pub fn parse_value_view(input: &str, scan: &ScanResult) -> Result<Value, ParseError> {
    if scan.lines.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }

    let mut root_index = 0;
    while root_index < scan.lines.len() && scan.lines[root_index].kind == LineKind::Blank {
        root_index += 1;
    }
    if root_index >= scan.lines.len() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let root_line = &scan.lines[root_index];
    let content_start = root_line.start + root_line.indent;
    let total_lines = scan.lines.len();
    if matches!(
        root_line.kind,
        LineKind::ArrayItem | LineKind::EmptyObjectItem
    ) {
        return Err(ParseError::new(1, 1, "root array must use header"));
    }

    if root_line.kind == LineKind::KeyValue {
        if input.as_bytes().get(content_start) == Some(&b'[') {
            if let Some((key_span, value_span)) =
                split_key_value(input, content_start, root_line.end, root_index, root_line.start)?
            {
                if let Some((key_name, header)) =
                    parse_array_header(input, key_span, root_index, root_line.start)?
                {
                    if key_name.start == key_name.end {
                        let root_form = if !header.fields.is_empty() {
                            ArrayForm::Tabular
                        } else if value_span.start != value_span.end {
                            ArrayForm::Inline
                        } else {
                            ArrayForm::List
                        };
                        let (value, next_i) = parse_array_value_value(
                            input,
                            scan,
                            root_index + 1,
                            root_line.indent,
                            value_span,
                            header,
                            root_index,
                        )?;
                        let mut end_i = next_i;
                        while end_i < total_lines && scan.lines[end_i].kind == LineKind::Blank {
                            end_i += 1;
                        }
                        if end_i != total_lines {
                            return Err(ParseError::new(
                                end_i + 1,
                                1,
                                "multiple root values not allowed",
                            ));
                        }
                        let expected_form = canonical_array_form_value(&value);
                        enforce_root_array_form(expected_form, root_form, 0)?;
                        return Ok(value);
                    }
                }
            }
        }

        if split_key_value(input, content_start, root_line.end, root_index, root_line.start)?
            .is_none()
        {
            let span = trim_line_span(input, root_line.start, root_line.end);
            let value = parse_primitive_value(input, span, root_index, root_line.start, DOC_DELIM)?;
            let mut end_i = root_index + 1;
            while end_i < total_lines && scan.lines[end_i].kind == LineKind::Blank {
                end_i += 1;
            }
            if end_i != total_lines {
                return Err(ParseError::new(
                    end_i + 1,
                    1,
                    "multiple root values not allowed",
                ));
            }
            return Ok(value);
        }
    }

    let base_indent = scan.lines[root_index].indent;
    let expected_array_len = None;
    let (value, next_i) = parse_value_block(input, scan, root_index, base_indent, expected_array_len)?;
    let mut end_i = next_i;
    while end_i < total_lines && scan.lines[end_i].kind == LineKind::Blank {
        end_i += 1;
    }
    if end_i != total_lines {
        return Err(ParseError::new(
            end_i + 1,
            1,
            "multiple root values not allowed",
        ));
    }
    Ok(value)
}

fn parse_value_block(
    input: &str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    expected_array_len: Option<usize>,
) -> Result<(Value, usize), ParseError> {
    if start >= scan.lines.len() {
        return Ok((Value::Null, start));
    }

    let mut i = start;
    let mut blank_index = None;
    while i < scan.lines.len() && scan.lines[i].kind == LineKind::Blank {
        if blank_index.is_none() {
            blank_index = Some(i);
        }
        i += 1;
    }
    if i >= scan.lines.len() {
        return Ok((Value::Null, i));
    }
    if blank_index.is_some()
        && matches!(
            scan.lines[i].kind,
            LineKind::ArrayItem | LineKind::EmptyObjectItem
        )
    {
        return Err(ParseError::new(
            blank_index.unwrap() + 1,
            1,
            "blank line not allowed inside array",
        ));
    }

    match scan.lines[i].kind {
        LineKind::ArrayItem | LineKind::EmptyObjectItem => {
            parse_array_value_block(input, scan, i, base_indent, expected_array_len)
        }
        LineKind::KeyValue => parse_object_value_block(input, scan, i, base_indent, None),
        LineKind::Blank => Ok((Value::Null, i)),
    }
}

fn parse_array_value_block(
    input: &str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    expected_len: Option<usize>,
) -> Result<(Value, usize), ParseError> {
    let inferred_len = expected_len.unwrap_or_else(|| count_array_items(scan, start, base_indent));
    let mut items = Vec::with_capacity(inferred_len);
    let mut i = start;
    while i < scan.lines.len() {
        let line = &scan.lines[i];
        if line.kind == LineKind::Blank {
            if blank_line_inside_array(scan, i, base_indent) {
                return Err(ParseError::new(i + 1, 1, "blank line not allowed inside array"));
            }
            break;
        }
        if line.indent != base_indent {
            break;
        }
        if !matches!(line.kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
            break;
        }
        let (value, next_i) = parse_array_item_value(input, scan, i, base_indent)?;
        items.push(value);
        i = next_i;
    }
    Ok((Value::Array(items), i))
}

fn parse_array_item_value(
    input: &str,
    scan: &ScanResult,
    index: usize,
    base_indent: usize,
) -> Result<(Value, usize), ParseError> {
    let line = &scan.lines[index];
    if line.kind == LineKind::EmptyObjectItem {
        return parse_nested_or_empty_value(input, scan, index + 1, base_indent + 2);
    }
    let item_start = line.start + line.indent + 2;
    if input.as_bytes().get(item_start) == Some(&b'[') {
        if let Some((key_span, value_span)) =
            split_key_value(input, item_start, line.end, index, line.start)?
        {
            if let Some((key_name, header)) =
                parse_array_header(input, key_span, index, line.start)?
            {
                if key_name.start == key_name.end {
                    let (value, next_i) = parse_array_value_value(
                        input,
                        scan,
                        index + 1,
                        base_indent,
                        value_span,
                        header,
                        index,
                    )?;
                    return Ok((value, next_i));
                }
            }
        }
    }
    if let Some((key_span, value_span)) =
        split_key_value(input, item_start, line.end, index, line.start)?
    {
        return parse_object_value_block(
            input,
            scan,
            index + 1,
            base_indent + 2,
            Some((key_span, value_span)),
        );
    }

    let value_span = trim_line_span(input, item_start, line.end);
    if value_span.start == value_span.end {
        if let Some(next_line) = scan.lines.get(index + 1) {
            if next_line.indent == base_indent + 2 && next_line.kind == LineKind::KeyValue {
                reject_list_item_tabular_header(input, scan, index + 1)?;
            }
        }
        return parse_nested_or_empty_value(input, scan, index + 1, base_indent + 2);
    }

    let value = parse_primitive_value(input, value_span, index, line.start, DOC_DELIM)?;
    Ok((value, index + 1))
}

fn parse_object_value_block(
    input: &str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
    inline_field: Option<(Span, Span)>,
) -> Result<(Value, usize), ParseError> {
    let expected_fields =
        (inline_field.is_some() as usize) + count_object_fields(scan, start, base_indent);
    let mut map = serde_json::Map::with_capacity(expected_fields);
    let mut i = start;
    if let Some((key_span, value_span)) = inline_field {
        let header_line = start.saturating_sub(1);
        let (key, value, next_i) = parse_field_from_spans_value(
            input,
            scan,
            start,
            base_indent,
            key_span,
            value_span,
            header_line,
        )?;
        map.insert(key, value);
        i = next_i;
    }

    while i < scan.lines.len() {
        let line = &scan.lines[i];
        if line.kind == LineKind::Blank {
            i += 1;
            continue;
        }
        if line.indent != base_indent {
            break;
        }
        if line.kind != LineKind::KeyValue {
            break;
        }
        let content_start = line.start + line.indent;
        let (key_span, value_span) =
            split_key_value(input, content_start, line.end, i, line.start)?
                .ok_or_else(|| ParseError::new(i + 1, 1, "missing ':' in object field"))?;
        let (key, value, next_i) = parse_field_from_spans_value(
            input,
            scan,
            i + 1,
            base_indent,
            key_span,
            value_span,
            i,
        )?;
        map.insert(key, value);
        i = next_i;
    }

    Ok((Value::Object(map), i))
}

fn parse_field_from_spans_value(
    input: &str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    key_span: Span,
    value_span: Span,
    header_line: usize,
) -> Result<(String, Value, usize), ParseError> {
    let key_line_start = scan
        .lines
        .get(header_line)
        .map(|line| line.start)
        .unwrap_or(key_span.start);
    let bytes = input.as_bytes();
    let key_quoted = bytes.get(key_span.start) == Some(&b'"');
    let mut key_name_span = key_span;
    if key_quoted {
        let mut i = key_span.start + 1;
        let mut has_escape = false;
        let mut found_quote = false;
        while i < key_span.end {
            match bytes[i] {
                b'\\' => {
                    has_escape = true;
                    i += 2;
                }
                b'"' => {
                    key_name_span = Span {
                        start: key_span.start + 1,
                        end: i,
                    };
                    found_quote = true;
                    break;
                }
                _ => i += 1,
            }
        }
        if !found_quote {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "unterminated quoted key",
            ));
        }
        let raw = input
            .get(key_name_span.start..key_name_span.end)
            .unwrap_or("");
        if !has_escape && is_canonical_unquoted_key(raw) {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "quoted key must be unquoted",
            ));
        }
    } else {
        let key_slice = input.get(key_span.start..key_span.end).unwrap_or("");
        let key_name = key_slice.split('[').next().unwrap_or(key_slice);
        if !is_canonical_unquoted_key(key_name) {
            return Err(ParseError::new(
                header_line + 1,
                key_span.start.saturating_sub(key_line_start) + 1,
                "unquoted key must be canonical",
            ));
        }
    }

    if let Some((key_name, header)) =
        parse_array_header(input, key_span, header_line, key_line_start)?
    {
        let key = if key_quoted {
            parse_quoted_string_value(
                input,
                key_name,
                header_line,
                key_line_start,
                DOC_DELIM,
                true,
            )?
        } else {
            input
                .get(key_name.start..key_name.end)
                .unwrap_or("")
                .to_string()
        };
        let (value, next_i) = parse_array_value_value(
            input,
            scan,
            next_index,
            base_indent,
            value_span,
            header,
            header_line,
        )?;
        return Ok((key, value, next_i));
    }

    let key = if key_quoted {
        parse_quoted_string_value(
            input,
            key_name_span,
            header_line,
            key_line_start,
            DOC_DELIM,
            true,
        )?
    } else {
        input
            .get(key_name_span.start..key_name_span.end)
            .unwrap_or("")
            .to_string()
    };

    let line_start = scan
        .lines
        .get(header_line)
        .map(|line| line.start)
        .unwrap_or(value_span.start);
    let (value, next_i) = parse_value_or_nested_value(
        input,
        scan,
        next_index,
        base_indent,
        value_span,
        header_line,
        line_start,
        DOC_DELIM,
    )?;
    Ok((key, value, next_i))
}

fn parse_value_or_nested_value(
    input: &str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    value_span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
) -> Result<(Value, usize), ParseError> {
    if value_span.start == value_span.end {
        return parse_nested_or_empty_value(input, scan, next_index, base_indent + 2);
    }
    let value_delimiter = if delimiter == b',' { b'\0' } else { delimiter };
    let value =
        parse_primitive_value(input, value_span, line_index, line_start, value_delimiter)?;
    Ok((value, next_index))
}

fn parse_nested_or_empty_value(
    input: &str,
    scan: &ScanResult,
    start: usize,
    base_indent: usize,
) -> Result<(Value, usize), ParseError> {
    if start >= scan.lines.len() {
        return Ok((Value::Object(serde_json::Map::new()), start));
    }
    let next_line = &scan.lines[start];
    if next_line.indent != base_indent {
        return Ok((Value::Object(serde_json::Map::new()), start));
    }
    parse_value_block(input, scan, start, base_indent, None)
}

fn parse_array_value_value(
    input: &str,
    scan: &ScanResult,
    next_index: usize,
    base_indent: usize,
    value_span: Span,
    header: ArrayHeader,
    header_line: usize,
) -> Result<(Value, usize), ParseError> {
    let expected_len = header.len.unwrap_or(0);
    let mut items = Vec::with_capacity(expected_len);

    if value_span.start != value_span.end {
        let line_start = scan
            .lines
            .get(header_line)
            .map(|line| line.start)
            .unwrap_or(value_span.start);
        if !header.fields.is_empty() {
            return Err(ParseError::new(
                header_line + 1,
                value_span.start.saturating_sub(line_start) + 1,
                "tabular arrays must use row form",
            ));
        }
        for_each_inline_value(
            input,
            value_span,
            header_line,
            line_start,
            header.delimiter,
            |item| {
                let value =
                    parse_primitive_value(input, item, header_line, line_start, header.delimiter)?;
                items.push(value);
                Ok(())
            },
        )?;
        if let Some(expected) = header.len {
            if expected != items.len() {
                return Err(ParseError::new(
                    header_line + 1,
                    1,
                    "array length does not match header",
                ));
            }
        }
        return Ok((Value::Array(items), next_index));
    }

    let list_indent = base_indent + 2;

    if next_index < scan.lines.len() {
        let next_line = &scan.lines[next_index];
        if next_line.kind == LineKind::Blank
            && blank_line_inside_array(scan, next_index, list_indent)
        {
            return Err(ParseError::new(
                next_index + 1,
                1,
                "blank line not allowed inside array",
            ));
        }
        if next_line.indent == list_indent
            && matches!(
                next_line.kind,
                LineKind::ArrayItem | LineKind::EmptyObjectItem
            )
        {
            let mut i = next_index;
            while i < scan.lines.len() {
                let line = &scan.lines[i];
                if line.kind == LineKind::Blank {
                    if blank_line_inside_array(scan, i, list_indent) {
                        return Err(ParseError::new(
                            i + 1,
                            1,
                            "blank line not allowed inside array",
                        ));
                    }
                    break;
                }
                if line.indent != list_indent {
                    break;
                }
                if !matches!(line.kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
                    break;
                }
                let (value, next_i) = parse_array_item_value(input, scan, i, list_indent)?;
                items.push(value);
                i = next_i;
            }
            if let Some(expected) = header.len {
                if expected != items.len() {
                    return Err(ParseError::new(
                        header_line + 1,
                        1,
                        "array length does not match header",
                    ));
                }
            }
            return Ok((Value::Array(items), i));
        }
    }

    if !header.fields.is_empty() {
        let header_line_start = scan
            .lines
            .get(header_line)
            .map(|line| line.start)
            .unwrap_or(value_span.start);
        let mut field_names = Vec::with_capacity(header.fields.len());
        for field in header.fields.iter() {
            if field.quoted {
                reject_unnecessary_quoted_header_field(
                    input,
                    field.span,
                    header_line,
                    header_line_start,
                )?;
                let name = parse_quoted_string_value(
                    input,
                    field.span,
                    header_line,
                    header_line_start,
                    header.delimiter,
                    true,
                )?;
                field_names.push(name);
            } else {
                let field_name = input.get(field.span.start..field.span.end).unwrap_or("");
                if !is_canonical_unquoted_key(field_name) {
                    return Err(ParseError::new(
                        header_line + 1,
                        field.span.start.saturating_sub(header_line_start) + 1,
                        "tabular field name must be canonical",
                    ));
                }
                field_names.push(field_name.to_string());
            }
        }
        let field_count = field_names.len();
        let mut i = next_index;
        while i < scan.lines.len() {
            let line = &scan.lines[i];
            if line.kind == LineKind::Blank {
                if blank_line_inside_array(scan, i, list_indent) {
                    return Err(ParseError::new(
                        i + 1,
                        1,
                        "blank line not allowed inside array",
                    ));
                }
                break;
            }
            if line.indent != list_indent {
                break;
            }
            let row_span = trim_line_span(input, line.start, line.end);
            let mut row_map = serde_json::Map::with_capacity(field_count);
            let mut col_index = 0;
            for_each_inline_value(
                input,
                row_span,
                i,
                line.start,
                header.delimiter,
                |value_span| {
                    if col_index >= field_count {
                        return Err(ParseError::new(
                            i + 1,
                            1,
                            "tabular row field count mismatch",
                        ));
                    }
                    let value =
                        parse_primitive_value(input, value_span, i, line.start, header.delimiter)?;
                    row_map.insert(field_names[col_index].clone(), value);
                    col_index += 1;
                    Ok(())
                },
            )?;
            if col_index != field_count {
                return Err(ParseError::new(
                    i + 1,
                    1,
                    "tabular row field count mismatch",
                ));
            }
            items.push(Value::Object(row_map));
            i += 1;
        }
        if let Some(expected) = header.len {
            if expected != items.len() {
                return Err(ParseError::new(
                    header_line + 1,
                    1,
                    "array length does not match header",
                ));
            }
        }
        return Ok((Value::Array(items), i));
    }

    if let Some(expected) = header.len {
        if expected != items.len() {
            return Err(ParseError::new(
                header_line + 1,
                1,
                "array length does not match header",
            ));
        }
    }
    Ok((Value::Array(items), next_index))
}

fn parse_primitive_value(
    input: &str,
    span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
) -> Result<Value, ParseError> {
    let raw = input.get(span.start..span.end).unwrap_or("");
    if raw.is_empty() {
        return Ok(Value::String(String::new()));
    }
    if raw == "null" {
        return Ok(Value::Null);
    }
    if raw == "true" {
        return Ok(Value::Bool(true));
    }
    if raw == "false" {
        return Ok(Value::Bool(false));
    }

    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        let inner_span = Span {
            start: span.start + 1,
            end: span.end - 1,
        };
        let value =
            parse_quoted_string_value(input, inner_span, line_index, line_start, delimiter, true)?;
        return Ok(Value::String(value));
    }

    if is_numeric_like(raw) && has_forbidden_leading_zero(raw) {
        return Ok(Value::String(raw.to_string()));
    }

    if is_canonical_number(raw) || is_numeric_like(raw) {
        let column = span.start.saturating_sub(line_start) + 1;
        let number = parse_number_value(raw, line_index, column)?;
        return Ok(Value::Number(number));
    }

    if !is_canonical_unquoted_string(raw, delimiter as char) {
        let column = span.start.saturating_sub(line_start) + 1;
        return Err(ParseError::new(
            line_index + 1,
            column,
            "unquoted string must be quoted",
        ));
    }

    Ok(Value::String(raw.to_string()))
}

fn has_forbidden_leading_zero(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i = 1;
    }
    if i >= bytes.len() {
        return false;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_len = i.saturating_sub(start);
    int_len > 1 && bytes[start] == b'0'
}

fn parse_number_value(
    raw: &str,
    line_index: usize,
    column: usize,
) -> Result<serde_json::Number, ParseError> {
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(value.into());
    }
    if let Ok(value) = raw.parse::<u64>() {
        return Ok(value.into());
    }
    let value = raw
        .parse::<f64>()
        .map_err(|_| ParseError::new(line_index + 1, column, "invalid canonical number"))?;
    if value.fract() == 0.0 {
        if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
            return Ok((value as i64).into());
        }
        if value >= 0.0 && value <= u64::MAX as f64 {
            return Ok((value as u64).into());
        }
    }
    serde_json::Number::from_f64(value)
        .ok_or_else(|| ParseError::new(line_index + 1, column, "invalid canonical number"))
}

fn parse_quoted_string_value(
    input: &str,
    span: Span,
    line_index: usize,
    line_start: usize,
    delimiter: u8,
    allow_unquoted: bool,
) -> Result<String, ParseError> {
    let bytes = input.as_bytes();
    let mut i = span.start;
    let mut last = span.start;
    let mut out = String::new();
    let mut has_escape = false;

    while i < span.end {
        match bytes[i] {
            b'\\' => {
                if i + 1 >= span.end {
                    let column = i.saturating_sub(line_start) + 1;
                    return Err(ParseError::new(
                        line_index + 1,
                        column,
                        "unterminated escape sequence",
                    ));
                }
                let esc = bytes[i + 1];
                let ch = match esc {
                    b'\\' => '\\',
                    b'"' => '"',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => {
                        let column = i.saturating_sub(line_start) + 1;
                        return Err(ParseError::new(
                            line_index + 1,
                            column,
                            "invalid escape sequence",
                        ));
                    }
                };
                if !has_escape {
                    out.push_str(input.get(last..i).unwrap_or(""));
                } else if last < i {
                    out.push_str(input.get(last..i).unwrap_or(""));
                }
                out.push(ch);
                has_escape = true;
                i += 2;
                last = i;
            }
            b'"' => {
                let column = i.saturating_sub(line_start) + 1;
                return Err(ParseError::new(
                    line_index + 1,
                    column,
                    "unescaped quote in string",
                ));
            }
            _ => i += 1,
        }
    }

    if has_escape {
        if last < span.end {
            out.push_str(input.get(last..span.end).unwrap_or(""));
        }
        return Ok(out);
    }

    let raw = input.get(span.start..span.end).unwrap_or("");
    if !allow_unquoted && is_canonical_unquoted_string(raw, delimiter as char) {
        let column = span.start.saturating_sub(line_start) + 1;
        return Err(ParseError::new(
            line_index + 1,
            column,
            "string must be unquoted",
        ));
    }
    Ok(raw.to_string())
}

fn enforce_root_array_form(
    expected: ArrayForm,
    actual: ArrayForm,
    line_index: usize,
) -> Result<(), ParseError> {
    if expected == actual {
        return Ok(());
    }
    let expected_label = match expected {
        ArrayForm::Inline => "inline",
        ArrayForm::List => "list",
        ArrayForm::Tabular => "tabular",
    };
    Err(ParseError::new(
        line_index + 1,
        1,
        format!("root array must use {expected_label} form"),
    ))
}

fn canonical_array_form_arena(arena: &ArenaView<'_>, node_index: usize) -> ArrayForm {
    let node = match arena.nodes.get(node_index) {
        Some(node) => node,
        None => return ArrayForm::List,
    };
    if node.kind != NodeKind::Array {
        return ArrayForm::List;
    }
    let items = arena.children(node);
    if items.is_empty() {
        return ArrayForm::List;
    }
    if items.iter().all(|idx| {
        matches!(
            arena.nodes.get(*idx).map(|n| n.kind),
            Some(NodeKind::Null | NodeKind::Bool | NodeKind::Number | NodeKind::String)
        )
    }) {
        return ArrayForm::Inline;
    }
    if items.len() >= min_tabular_rows() && is_tabular_candidate_arena(arena, items) {
        return ArrayForm::Tabular;
    }
    ArrayForm::List
}

fn is_tabular_candidate_arena(arena: &ArenaView<'_>, items: &[usize]) -> bool {
    let first_idx = match items.first() {
        Some(idx) => *idx,
        None => return false,
    };
    let first = match arena.nodes.get(first_idx) {
        Some(node) => node,
        None => return false,
    };
    if first.kind != NodeKind::Object {
        return false;
    }
    let pairs = arena.pairs(first);
    if pairs.is_empty() {
        return false;
    }
    let mut keys = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let key = match arena.get_str(pair.key) {
            Some(key) => key,
            None => return false,
        };
        let value = match arena.nodes.get(pair.value) {
            Some(node) => node,
            None => return false,
        };
        if !matches!(
            value.kind,
            NodeKind::Null | NodeKind::Bool | NodeKind::Number | NodeKind::String
        ) {
            return false;
        }
        keys.push(key);
    }
    for item_idx in items.iter().skip(1) {
        let node = match arena.nodes.get(*item_idx) {
            Some(node) => node,
            None => return false,
        };
        if node.kind != NodeKind::Object {
            return false;
        }
        let pairs = arena.pairs(node);
        if pairs.len() != keys.len() {
            return false;
        }
        for (idx, pair) in pairs.iter().enumerate() {
            let key = match arena.get_str(pair.key) {
                Some(key) => key,
                None => return false,
            };
            if key != keys[idx] {
                return false;
            }
            let value = match arena.nodes.get(pair.value) {
                Some(node) => node,
                None => return false,
            };
            if !matches!(
                value.kind,
                NodeKind::Null | NodeKind::Bool | NodeKind::Number | NodeKind::String
            ) {
                return false;
            }
        }
    }
    true
}

fn canonical_array_form_value(value: &Value) -> ArrayForm {
    let items = match value {
        Value::Array(items) => items,
        _ => return ArrayForm::List,
    };
    if items.is_empty() {
        return ArrayForm::List;
    }
    if items.iter().all(|item| {
        matches!(
            item,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
        )
    }) {
        return ArrayForm::Inline;
    }
    if items.len() >= min_tabular_rows() && is_tabular_candidate_value(items) {
        return ArrayForm::Tabular;
    }
    ArrayForm::List
}

fn is_tabular_candidate_value(items: &[Value]) -> bool {
    let first = match items.first() {
        Some(value) => value,
        None => return false,
    };
    let first_obj = match first.as_object() {
        Some(obj) => obj,
        None => return false,
    };
    if first_obj.is_empty() {
        return false;
    }
    let keys: Vec<&str> = first_obj.keys().map(|key| key.as_str()).collect();
    for value in first_obj.values() {
        if !matches!(
            value,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
        ) {
            return false;
        }
    }
    for item in items.iter().skip(1) {
        let obj = match item.as_object() {
            Some(obj) => obj,
            None => return false,
        };
        if obj.len() != keys.len() {
            return false;
        }
        for (idx, (key, value)) in obj.iter().enumerate() {
            if key.as_str() != keys[idx] {
                return false;
            }
            if !matches!(
                value,
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
            ) {
                return false;
            }
        }
    }
    true
}
