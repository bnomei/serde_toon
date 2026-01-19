use memchr::memchr_iter;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub struct ScanLine {
    pub indent: usize,
    pub level: usize,
    pub start: usize,
    pub end: usize,
    pub is_blank: bool,
}

#[derive(Debug)]
pub struct ScanResult {
    pub lines: Vec<ScanLine>,
    pub non_blank: usize,
}

pub fn scan_lines(input: &str, indent_size: usize, strict: bool) -> Result<ScanResult> {
    if indent_size == 0 {
        return Err(Error::decode("indent size must be greater than zero"));
    }
    let bytes = input.as_bytes();
    let mut lines = Vec::new();
    let mut non_blank = 0;
    let mut start = 0;
    for idx in memchr_iter(b'\n', bytes) {
        let mut end = idx;
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        let line = build_line(bytes, start, end, indent_size, strict)?;
        if !line.is_blank {
            non_blank += 1;
        }
        lines.push(line);
        start = idx + 1;
    }

    let mut end = bytes.len();
    if end > start && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    let line = build_line(bytes, start, end, indent_size, strict)?;
    if !line.is_blank {
        non_blank += 1;
    }
    lines.push(line);

    Ok(ScanResult { lines, non_blank })
}

fn build_line(
    bytes: &[u8],
    start: usize,
    end: usize,
    indent_size: usize,
    strict: bool,
) -> Result<ScanLine> {
    if start >= end {
        return Ok(ScanLine {
            indent: 0,
            level: 0,
            start,
            end,
            is_blank: true,
        });
    }
    let mut only_whitespace = true;
    for &byte in &bytes[start..end] {
        if !byte.is_ascii_whitespace() {
            only_whitespace = false;
            break;
        }
    }
    if only_whitespace {
        return Ok(ScanLine {
            indent: 0,
            level: 0,
            start,
            end,
            is_blank: true,
        });
    }
    let mut indent_columns: usize = 0;
    let mut indent_chars: usize = 0;
    for &byte in &bytes[start..end] {
        match byte {
            b' ' => {
                indent_columns += 1;
                indent_chars += 1;
            }
            b'\t' => {
                if strict {
                    return Err(Error::decode("tabs not allowed in indentation"));
                }
                indent_columns = indent_columns.saturating_add(indent_size);
                indent_chars += 1;
            }
            _ => break,
        }
    }
    if strict && !indent_columns.is_multiple_of(indent_size) {
        return Err(Error::decode("invalid indentation"));
    }
    let level = indent_columns / indent_size;
    let content_start = start + indent_chars;
    Ok(ScanLine {
        indent: indent_columns,
        level,
        start: content_start,
        end,
        is_blank: false,
    })
}
