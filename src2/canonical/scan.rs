//! Preflight and structural scan for canonical input.

use super::profile::CanonicalProfile;
use memchr::memchr_iter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    KeyValue,
    ArrayItem,
    EmptyObjectItem,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanLine {
    pub indent: usize,
    pub kind: LineKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub struct ScanResult {
    pub lines: Vec<ScanLine>,
}

#[derive(Debug)]
pub struct ScanError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl ScanError {
    fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }
}

pub fn preflight_scan(input: &str, profile: CanonicalProfile) -> Result<ScanResult, ScanError> {
    if input.is_empty() {
        return Ok(ScanResult { lines: Vec::new() });
    }

    let mut lines = Vec::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut line_start = 0;
    let mut line_no = 1;
    let mut prev_indent = 0;
    let mut last_key_by_indent: std::collections::BTreeMap<usize, Vec<u8>> =
        std::collections::BTreeMap::new();

    for line_end in memchr_iter(b'\n', bytes) {
        process_line(
            bytes,
            line_start,
            line_end,
            line_no,
            profile,
            &mut prev_indent,
            &mut last_key_by_indent,
            &mut lines,
        )?;
        line_start = line_end + 1;
        line_no += 1;
    }

    if line_start < len {
        process_line(
            bytes,
            line_start,
            len,
            line_no,
            profile,
            &mut prev_indent,
            &mut last_key_by_indent,
            &mut lines,
        )?;
    }

    Ok(ScanResult { lines })
}

fn process_line(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    line_no: usize,
    profile: CanonicalProfile,
    prev_indent: &mut usize,
    last_key_by_indent: &mut std::collections::BTreeMap<usize, Vec<u8>>,
    lines: &mut Vec<ScanLine>,
) -> Result<(), ScanError> {
    if line_end == line_start {
        lines.push(ScanLine {
            indent: 0,
            kind: LineKind::Blank,
            start: line_start,
            end: line_end,
        });
        return Ok(());
    }

    let mut only_spaces = line_start;
    while only_spaces < line_end && bytes[only_spaces] == b' ' {
        only_spaces += 1;
    }
    if only_spaces == line_end {
        lines.push(ScanLine {
            indent: line_end - line_start,
            kind: LineKind::Blank,
            start: line_start,
            end: line_end,
        });
        return Ok(());
    }

    reject_noncanonical_delims(bytes, line_start, line_end, line_no)?;

    let mut indent = 0;
    while line_start + indent < line_end && bytes[line_start + indent] == b' ' {
        indent += 1;
    }

    if line_start + indent < line_end && bytes[line_start + indent] == b'\t' {
        return Err(ScanError::new(
            line_no,
            indent + 1,
            "tab indentation not allowed",
        ));
    }

    if indent == line_end - line_start {
        return Err(ScanError::new(line_no, 1, "line cannot be whitespace-only"));
    }

    if profile.indent_spaces > 0 && indent % profile.indent_spaces != 0 {
        return Err(ScanError::new(
            line_no,
            1,
            "indentation must be a multiple of canonical indent",
        ));
    }

    let content_start = line_start + indent;
    let kind = if bytes[content_start] == b'-' {
        if content_start + 1 == line_end {
            LineKind::EmptyObjectItem
        } else if bytes[content_start + 1] == b' ' {
            LineKind::ArrayItem
        } else {
            LineKind::KeyValue
        }
    } else {
        LineKind::KeyValue
    };

    if indent < *prev_indent {
        drop_keys_from(last_key_by_indent, indent + 1);
    }

    if matches!(kind, LineKind::ArrayItem | LineKind::EmptyObjectItem) {
        drop_keys_from(last_key_by_indent, indent + 2);
    }

    if let Some((key_indent, key_bytes)) = extract_key(
        bytes,
        content_start,
        line_end,
        line_start,
        indent,
        kind,
        line_no,
    )? {
        last_key_by_indent.insert(key_indent, key_bytes);
    }

    lines.push(ScanLine {
        indent,
        kind,
        start: line_start,
        end: line_end,
    });

    *prev_indent = indent;
    Ok(())
}

fn drop_keys_from(map: &mut std::collections::BTreeMap<usize, Vec<u8>>, from_indent: usize) {
    let _ = map.split_off(&from_indent);
}

fn extract_key(
    bytes: &[u8],
    content_start: usize,
    line_end: usize,
    line_start: usize,
    indent: usize,
    kind: LineKind,
    line_no: usize,
) -> Result<Option<(usize, Vec<u8>)>, ScanError> {
    let (key_start, key_indent) = match kind {
        LineKind::ArrayItem => (content_start + 2, indent + 2),
        LineKind::EmptyObjectItem | LineKind::Blank => return Ok(None),
        LineKind::KeyValue => (content_start, indent),
    };

    if matches!(kind, LineKind::ArrayItem) && bytes[key_start] == b'[' {
        return Ok(None);
    }

    if key_start >= line_end {
        return Ok(None);
    }

    if kind == LineKind::KeyValue && bytes[key_start] == b'[' {
        if line_no == 1 && indent == 0 {
            return Ok(None);
        }
    }

    let key_column = key_start - line_start + 1;
    let (key_bytes, key_end) = if bytes[key_start] == b'"' {
        parse_quoted_key(bytes, key_start, line_end, line_no, key_column)?
    } else {
        parse_unquoted_key(bytes, key_start, line_end, line_no, key_column)?
    };

    if key_end >= line_end {
        return Ok(None);
    }

    let mut i = key_end;
    while i < line_end && bytes[i] == b' ' {
        i += 1;
    }

    if i < line_end && (bytes[i] == b':' || bytes[i] == b'[') {
        Ok(Some((key_indent, key_bytes)))
    } else {
        Ok(None)
    }
}

fn parse_unquoted_key(
    bytes: &[u8],
    start: usize,
    line_end: usize,
    line_no: usize,
    column: usize,
) -> Result<(Vec<u8>, usize), ScanError> {
    let mut i = start;
    while i < line_end {
        match bytes[i] {
            b':' | b'[' => break,
            b' ' => break,
            _ => i += 1,
        }
    }

    if i == start {
        return Err(ScanError::new(line_no, column, "empty key"));
    }

    Ok((bytes[start..i].to_vec(), i))
}

fn parse_quoted_key(
    bytes: &[u8],
    start: usize,
    line_end: usize,
    line_no: usize,
    column: usize,
) -> Result<(Vec<u8>, usize), ScanError> {
    let mut out = Vec::new();
    let mut i = start + 1;
    while i < line_end {
        match bytes[i] {
            b'"' => return Ok((out, i + 1)),
            b'\\' => {
                if i + 1 >= line_end {
                    let escape_column = column + (i - start);
                    return Err(ScanError::new(
                        line_no,
                        escape_column,
                        "unterminated escape",
                    ));
                }
                let esc = bytes[i + 1];
                match esc {
                    b'\\' | b'"' => out.push(esc),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    _ => {
                        let escape_column = column + (i - start);
                        return Err(ScanError::new(
                            line_no,
                            escape_column,
                            "invalid escape sequence",
                        ));
                    }
                }
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    let end_column = column + (line_end - start);
    Err(ScanError::new(
        line_no,
        end_column,
        "unterminated quoted key",
    ))
}

fn reject_noncanonical_delims(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    line_no: usize,
) -> Result<(), ScanError> {
    let mut in_quotes = false;
    let mut i = line_start;
    while i < line_end {
        match bytes[i] {
            b'"' => {
                in_quotes = !in_quotes;
                i += 1;
            }
            b'\\' if in_quotes => {
                if i + 1 >= line_end {
                    let column = i - line_start + 1;
                    return Err(ScanError::new(
                        line_no,
                        column,
                        "unterminated escape sequence",
                    ));
                }
                i += 2;
            }
            b'|' if !in_quotes => {
                i += 1;
            }
            b'\t' => {
                i += 1;
            }
            b'\r' => {
                let column = i - line_start + 1;
                return Err(ScanError::new(line_no, column, "CR not allowed"));
            }
            _ => i += 1,
        }
    }

    Ok(())
}
