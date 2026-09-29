use memchr::memchr_iter;

use crate::error::Location;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub struct ScanLine {
    pub raw_start: usize,
    pub indent: usize,
    pub level: usize,
    pub start: usize,
    pub end: usize,
    pub is_blank: bool,
    pub is_comment: bool,
}

#[derive(Debug)]
pub struct ScanResult {
    pub lines: Vec<ScanLine>,
    pub non_blank: usize,
}

pub fn scan_lines(
    input: &str,
    indent_size: usize,
    strict: bool,
    validate: bool,
) -> Result<ScanResult> {
    if indent_size == 0 {
        return Err(Error::decode("indent size must be greater than zero"));
    }
    let bytes = input.as_bytes();
    let mut lines = Vec::new();
    let mut non_blank = 0;
    // A BOM is syntax only at byte zero. Keeping offsets in the original input
    // lets arena spans and diagnostics continue to refer to the caller's text.
    let mut start = if bytes.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    for idx in memchr_iter(b'\n', bytes) {
        let mut end = idx;
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        if validate
            && end > start
            && bytes[start..end].iter().find(|&&byte| byte != b' ') != Some(&b'#')
        {
            let last = bytes[end - 1];
            if last == b' ' {
                return Err(Error::decode("trailing whitespace not allowed"));
            }
        }
        let line_idx = lines.len();
        let line = build_line(bytes, start, end, indent_size, strict).map_err(|err| {
            err.with_location(Location {
                offset: start,
                line: line_idx + 1,
                column: 1,
            })
        })?;
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
    if validate
        && end > start
        && bytes[start..end].iter().find(|&&byte| byte != b' ') != Some(&b'#')
    {
        let last = bytes[end - 1];
        if last == b' ' {
            return Err(Error::decode("trailing whitespace not allowed"));
        }
    }
    let line_idx = lines.len();
    let line = build_line(bytes, start, end, indent_size, strict).map_err(|err| {
        err.with_location(Location {
            offset: start,
            line: line_idx + 1,
            column: 1,
        })
    })?;
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
    let mut end = end;
    while end > start && bytes[end - 1] == b' ' {
        end -= 1;
    }
    if start >= end {
        return Ok(ScanLine {
            raw_start: start,
            indent: 0,
            level: 0,
            start,
            end,
            is_blank: true,
            is_comment: false,
        });
    }
    let mut only_whitespace = true;
    for &byte in &bytes[start..end] {
        if byte != b' ' && byte != b'\t' {
            only_whitespace = false;
            break;
        }
    }
    let spaces = bytes[start..end]
        .iter()
        .take_while(|&&byte| byte == b' ')
        .count();
    let possible_tsv = spaces > 0
        && spaces.is_multiple_of(indent_size)
        && bytes.get(start + spaces) == Some(&b'\t');
    if only_whitespace && !possible_tsv {
        return Ok(ScanLine {
            raw_start: start,
            indent: 0,
            level: 0,
            start,
            end,
            is_blank: true,
            is_comment: false,
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
                // Once a complete space indentation prefix has been consumed,
                // HTAB may be the first (empty) cell of a tabular row.  Keep it
                // in the content; the scope-aware parser decides whether it is
                // a delimiter. A leading tab remains illegal indentation.
                if indent_columns > 0
                    && indent_columns.is_multiple_of(indent_size)
                    && bytes[start..start + indent_chars]
                        .iter()
                        .all(|&byte| byte == b' ')
                {
                    break;
                }
                if strict {
                    return Err(Error::decode("tabs not allowed in indentation"));
                }
                indent_columns = indent_columns.saturating_add(indent_size);
                indent_chars += 1;
            }
            _ => break,
        }
    }
    // Comments are removed before indentation and every structural operation.
    // In particular, oddly-indented comments are valid and do not become blank
    // lines inside an array scope.
    if bytes[start..start + indent_chars]
        .iter()
        .all(|&b| b == b' ')
        && bytes.get(start + indent_chars) == Some(&b'#')
    {
        return Ok(ScanLine {
            raw_start: start,
            indent: 0,
            level: 0,
            start: end,
            end,
            is_blank: true,
            is_comment: true,
        });
    }
    if strict && !indent_columns.is_multiple_of(indent_size) {
        return Err(Error::decode("invalid indentation"));
    }
    let level = indent_columns / indent_size;
    let content_start = start + indent_chars;
    Ok(ScanLine {
        raw_start: start,
        indent: indent_columns,
        level,
        start: content_start,
        end,
        is_blank: false,
        is_comment: false,
    })
}
