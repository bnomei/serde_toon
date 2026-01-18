use std::sync::Arc;

use crate::{
    constants::DEFAULT_INDENT,
    types::{Delimiter, ToonError, ToonResult},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Dash,
    Newline,
    String(String, bool),
    Number(f64),
    Integer(i64),
    Bool(bool),
    Null,
    Delimiter(Delimiter),
    Eof,
}

pub struct Scanner {
    input: Arc<str>,
    position: usize,
    line: usize,
    column: usize,
    active_delimiter: Option<Delimiter>,
    last_line_indent: usize,
    cached_indent: Option<CachedIndent>,
    coerce_types: bool,
    indent_width: usize,
    allow_tab_indent: bool,
    strict_mode: bool,
}

#[derive(Clone, Copy, Debug)]
struct CachedIndent {
    position: usize,
    indent: usize,
    chars: usize,
}

impl Scanner {
    #[allow(dead_code)]
    pub(crate) fn new(input: &str) -> Self {
        Self::from_shared_input(Arc::from(input))
    }

    pub fn from_shared_input(input: Arc<str>) -> Self {
        Self {
            input,
            position: 0,
            line: 1,
            column: 1,
            active_delimiter: None,
            last_line_indent: 0,
            cached_indent: None,
            coerce_types: true,
            indent_width: DEFAULT_INDENT,
            allow_tab_indent: false,
            strict_mode: true,
        }
    }

    pub fn set_active_delimiter(&mut self, delimiter: Option<Delimiter>) {
        self.active_delimiter = delimiter;
    }

    pub fn set_coerce_types(&mut self, coerce_types: bool) {
        self.coerce_types = coerce_types;
    }

    pub fn configure_indentation(&mut self, strict: bool, indent_width: usize) {
        self.allow_tab_indent = !strict;
        self.strict_mode = strict;
        self.indent_width = indent_width.max(1);
    }

    pub fn current_position(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    pub fn peek(&self) -> Option<char> {
        let bytes = self.input.as_bytes();
        match bytes.get(self.position) {
            Some(&byte) if byte.is_ascii() => Some(byte as char),
            Some(_) => self.input[self.position..].chars().next(),
            None => None,
        }
    }

    pub fn count_leading_spaces(&mut self) -> usize {
        self.peek_indent()
    }


    pub fn advance(&mut self) -> Option<char> {
        let bytes = self.input.as_bytes();
        match bytes.get(self.position) {
            Some(&byte) if byte.is_ascii() => {
                self.position += 1;
                if byte == b'\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                Some(byte as char)
            }
            Some(_) => {
                let ch = self.input[self.position..].chars().next()?;
                self.position += ch.len_utf8();
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                Some(ch)
            }
            None => None,
        }
    }

    pub fn skip_whitespace(&mut self) -> usize {
        let mut skipped = 0;
        while let Some(ch) = self.peek() {
            if ch == ' ' {
                self.advance();
                skipped += 1;
            } else {
                break;
            }
        }
        skipped
    }

    fn count_indent_from_with_chars(&self, idx: &mut usize) -> (usize, usize) {
        let mut count = 0;
        let mut chars = 0;
        let bytes = self.input.as_bytes();
        while *idx < bytes.len() {
            match bytes[*idx] {
                b' ' => {
                    count += 1;
                    chars += 1;
                    *idx += 1;
                }
                b'\t' if self.allow_tab_indent => {
                    count += self.indent_width;
                    chars += 1;
                    *idx += 1;
                }
                _ => break,
            }
        }
        (count, chars)
    }

    fn peek_indent(&mut self) -> usize {
        if let Some(cached) = self.cached_indent {
            if cached.position == self.position {
                return cached.indent;
            }
        }

        let mut idx = self.position;
        let (indent, chars) = self.count_indent_from_with_chars(&mut idx);
        self.cached_indent = Some(CachedIndent {
            position: self.position,
            indent,
            chars,
        });
        indent
    }

    pub fn scan_token(&mut self) -> ToonResult<Token> {
        if self.column == 1 {
            let mut indent_consumed = false;
            if let Some(cached) = self.cached_indent.take() {
                if cached.position == self.position {
                    self.position += cached.chars;
                    self.column += cached.chars;
                    if !self.allow_tab_indent && matches!(self.peek(), Some('\t')) {
                        let (line, col) = self.current_position();
                        return Err(ToonError::parse_error(
                            line,
                            col,
                            "Tabs are not allowed in indentation",
                        ));
                    }
                    self.last_line_indent = cached.indent;
                    indent_consumed = true;
                } else {
                    self.cached_indent = Some(cached);
                }
            }

            if !indent_consumed {
                let mut count = 0;
                while let Some(ch) = self.peek() {
                    match ch {
                        ' ' => {
                            count += 1;
                            self.advance();
                        }
                        '\t' => {
                            if !self.allow_tab_indent {
                                let (line, col) = self.current_position();
                                return Err(ToonError::parse_error(
                                    line,
                                    col + count,
                                    "Tabs are not allowed in indentation",
                                ));
                            }
                            count += self.indent_width;
                            self.advance();
                        }
                        _ => break,
                    }
                }
                self.last_line_indent = count;
            }
        }

        self.skip_whitespace();

        match self.peek() {
            None => Ok(Token::Eof),
            Some('\n') => {
                self.advance();
                Ok(Token::Newline)
            }
            Some('[') => {
                self.advance();
                Ok(Token::LeftBracket)
            }
            Some(']') => {
                self.advance();
                Ok(Token::RightBracket)
            }
            Some('{') => {
                self.advance();
                Ok(Token::LeftBrace)
            }
            Some('}') => {
                self.advance();
                Ok(Token::RightBrace)
            }
            Some(':') => {
                self.advance();
                Ok(Token::Colon)
            }
            Some('-') => {
                self.advance();
                if let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        let start = self.position.saturating_sub(1);
                        let range = self.scan_number_range(start);
                        if self.should_merge_number_as_string() {
                            return self.merge_number_with_unquoted_suffix_range(range);
                        }
                        return self.parse_number(&self.input[range]);
                    }
                }
                Ok(Token::Dash)
            }
            Some(',') => {
                // Delimiter only when active, otherwise part of unquoted string
                if matches!(self.active_delimiter, Some(Delimiter::Comma)) {
                    self.advance();
                    Ok(Token::Delimiter(Delimiter::Comma))
                } else {
                    self.scan_unquoted_string()
                }
            }
            Some('|') => {
                if matches!(self.active_delimiter, Some(Delimiter::Pipe)) {
                    self.advance();
                    Ok(Token::Delimiter(Delimiter::Pipe))
                } else {
                    self.scan_unquoted_string()
                }
            }
            Some('\t') => {
                if matches!(self.active_delimiter, Some(Delimiter::Tab)) {
                    self.advance();
                    Ok(Token::Delimiter(Delimiter::Tab))
                } else {
                    self.scan_unquoted_string()
                }
            }
            Some('"') => self.scan_quoted_string(),
            Some(ch) if ch.is_ascii_digit() => {
                let start = self.position;
                let range = self.scan_number_range(start);
                if self.should_merge_number_as_string() {
                    return self.merge_number_with_unquoted_suffix_range(range);
                }
                self.parse_number(&self.input[range])
            }
            Some(_) => self.scan_unquoted_string(),
        }
    }

    fn scan_quoted_string(&mut self) -> ToonResult<Token> {
        self.advance();

        let mut value = String::new();
        let mut escaped = false;

        while let Some(ch) = self.advance() {
            if escaped {
                match ch {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    _ => {
                        let (line, col) = self.current_position();
                        return Err(ToonError::parse_error(
                            line,
                            col - 1,
                            format!("Invalid escape sequence: \\{ch}"),
                        ));
                    }
                }
                escaped = false;
            } else if ch == '\n' || ch == '\r' {
                let (line, col) = self.current_position();
                return Err(ToonError::parse_error(
                    line,
                    col,
                    "Unescaped newline in string",
                ));
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok(Token::String(value, true));
            } else {
                value.push(ch);
            }
        }

        Err(ToonError::UnexpectedEof)
    }

    fn scan_unquoted_string(&mut self) -> ToonResult<Token> {
        let bytes = self.input.as_bytes();
        let start = self.position;
        let mut idx = start;
        let mut has_non_ascii = false;

        while idx < bytes.len() {
            let byte = bytes[idx];
            if byte < 0x80 {
                if self.is_unquoted_terminator_byte(byte) {
                    break;
                }
            } else {
                has_non_ascii = true;
            }
            idx += 1;
        }

        let slice = &self.input[start..idx];
        self.position = idx;
        if has_non_ascii {
            self.column += slice.chars().count();
        } else {
            self.column += idx - start;
        }

        let mut value = slice.to_string();

        // Single-char delimiters kept as-is, others trimmed
        if !(value.len() == 1 && (value == "," || value == "|" || value == "\t")) {
            let trimmed_len = value.trim_end().len();
            value.truncate(trimmed_len);
        }

        if !self.coerce_types {
            return Ok(Token::String(value, false));
        }

        match value.as_str() {
            "null" => Ok(Token::Null),
            "true" => Ok(Token::Bool(true)),
            "false" => Ok(Token::Bool(false)),
            _ => Ok(Token::String(value, false)),
        }
    }

    fn is_unquoted_terminator(&self, ch: char) -> bool {
        if matches!(ch, '\n' | ' ' | ':' | '[' | ']' | '{' | '}') {
            return true;
        }

        if let Some(active) = self.active_delimiter {
            return matches!(
                (active, ch),
                (Delimiter::Comma, ',') | (Delimiter::Pipe, '|') | (Delimiter::Tab, '\t')
            );
        }

        false
    }

    fn is_unquoted_terminator_byte(&self, byte: u8) -> bool {
        if matches!(byte, b'\n' | b' ' | b':' | b'[' | b']' | b'{' | b'}') {
            return true;
        }

        if let Some(active) = self.active_delimiter {
            return matches!(
                (active, byte),
                (Delimiter::Comma, b',') | (Delimiter::Pipe, b'|') | (Delimiter::Tab, b'\t')
            );
        }

        false
    }

    fn should_merge_number_as_string(&self) -> bool {
        match self.peek() {
            Some(ch) => !self.is_unquoted_terminator(ch),
            None => false,
        }
    }

    fn merge_number_with_unquoted_suffix_range(
        &mut self,
        prefix_range: std::ops::Range<usize>,
    ) -> ToonResult<Token> {
        let mut prefix = self.input[prefix_range].to_string();
        let rest = match self.scan_unquoted_string()? {
            Token::String(value, _) => value,
            token => return Ok(token),
        };

        prefix.push_str(&rest);
        Ok(Token::String(prefix, false))
    }

    pub fn get_last_line_indent(&self) -> usize {
        self.last_line_indent
    }

    fn scan_number_range(&mut self, start: usize) -> std::ops::Range<usize> {
        let bytes = self.input.as_bytes();
        let begin = self.position;
        let mut idx = begin;

        while idx < bytes.len() {
            match bytes[idx] {
                b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-' => idx += 1,
                _ => break,
            }
        }

        self.position = idx;
        self.column += idx - begin;
        start..idx
    }

    fn parse_number(&self, s: &str) -> ToonResult<Token> {
        if !self.coerce_types {
            return Ok(Token::String(s.to_string(), false));
        }

        // Number followed immediately by other chars like "0(f)" should be a string
        if let Some(next_ch) = self.peek() {
            if next_ch != ' '
                && next_ch != '\n'
                && next_ch != ':'
                && next_ch != '['
                && next_ch != ']'
                && next_ch != '{'
                && next_ch != '}'
                && !matches!(
                    (self.active_delimiter, next_ch),
                    (Some(Delimiter::Comma), ',')
                        | (Some(Delimiter::Pipe), '|')
                        | (Some(Delimiter::Tab), '\t')
                )
            {
                return Ok(Token::String(s.to_string(), false));
            }
        }

        // Leading zeros like "05" are strings, but "0", "0.5", "-0" are numbers
        if s.starts_with('0') && s.len() > 1 {
            if let Some(second_char) = s.chars().nth(1) {
                if second_char.is_ascii_digit() {
                    return Ok(Token::String(s.to_string(), false));
                }
            }
        }
        if s.starts_with("-0") && s.len() > 2 {
            if let Some(third_char) = s.chars().nth(2) {
                if third_char.is_ascii_digit() {
                    return Ok(Token::String(s.to_string(), false));
                }
            }
        }

        if s.contains('.') || s.contains('e') || s.contains('E') {
            if let Ok(f) = s.parse::<f64>() {
                Ok(Token::Number(f))
            } else {
                Ok(Token::String(s.to_string(), false))
            }
        } else if let Ok(i) = s.parse::<i64>() {
            Ok(Token::Integer(i))
        } else {
            Ok(Token::String(s.to_string(), false))
        }
    }

    pub fn read_rest_of_line_with_space_count(&mut self) -> (String, usize, bool) {
        let bytes = self.input.as_bytes();
        let start = self.position;
        let mut idx = start;
        let mut leading_space = 0usize;

        while idx < bytes.len() && bytes[idx] == b' ' {
            leading_space += 1;
            idx += 1;
        }

        let content_start = idx;
        let mut has_non_ascii = false;
        while idx < bytes.len() {
            let byte = bytes[idx];
            if byte == b'\n' {
                break;
            }
            if byte >= 0x80 {
                has_non_ascii = true;
            }
            idx += 1;
        }

        let slice = &self.input[content_start..idx];
        let mut result = slice.to_string();
        self.position = idx;
        if has_non_ascii {
            self.column += leading_space + slice.chars().count();
        } else {
            self.column += idx - start;
        }

        let trimmed_len = result.trim_end().len();
        let had_trailing_space = trimmed_len != result.len();
        result.truncate(trimmed_len);
        (result, leading_space, had_trailing_space)
    }

    pub fn parse_value_string(&self, s: &str) -> ToonResult<Token> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Ok(Token::String(String::new(), false));
        }

        if trimmed.starts_with('"') {
            let mut value = String::new();
            let mut escaped = false;

            let mut chars = trimmed.char_indices();
            chars.next();

            for (idx, ch) in chars {
                if escaped {
                    match ch {
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        _ => {
                            return Err(ToonError::parse_error(
                                self.line,
                                self.column,
                                format!("Invalid escape sequence: \\{ch}"),
                            ));
                        }
                    }
                    escaped = false;
                    continue;
                }

                if ch == '\\' {
                    escaped = true;
                    continue;
                }

                if ch == '"' {
                    if idx + ch.len_utf8() != trimmed.len() {
                        return Err(ToonError::parse_error(
                            self.line,
                            self.column,
                            "Unexpected characters after closing quote",
                        ));
                    }
                    return Ok(Token::String(value, true));
                }

                value.push(ch);
            }

            return Err(ToonError::parse_error(
                self.line,
                self.column,
                "Unterminated string: missing closing quote",
            ));
        }

        if !self.coerce_types {
            return Ok(Token::String(trimmed.to_string(), false));
        }

        match trimmed {
            "true" => return Ok(Token::Bool(true)),
            "false" => return Ok(Token::Bool(false)),
            "null" => return Ok(Token::Null),
            _ => {}
        }

        if trimmed.starts_with('-') || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            // Leading zeros like "05" or "-05" are strings
            if trimmed.starts_with('0') && trimmed.len() > 1 {
                if let Some(second_char) = trimmed.chars().nth(1) {
                    if second_char.is_ascii_digit() {
                        return Ok(Token::String(trimmed.to_string(), false));
                    }
                }
            }
            if trimmed.starts_with("-0") && trimmed.len() > 2 {
                if let Some(third_char) = trimmed.chars().nth(2) {
                    if third_char.is_ascii_digit() {
                        return Ok(Token::String(trimmed.to_string(), false));
                    }
                }
            }

            if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
                if let Ok(f) = trimmed.parse::<f64>() {
                    let normalized = if f == -0.0 { 0.0 } else { f };
                    return Ok(Token::Number(normalized));
                }
            } else if let Ok(i) = trimmed.parse::<i64>() {
                return Ok(Token::Integer(i));
            }
        }

        Ok(Token::String(trimmed.to_string(), false))
    }

}

#[cfg(test)]
mod tests {
    use core::f64;
    use std::sync::Arc;

    use super::*;

    fn make_scanner(input: &str) -> Scanner {
        Scanner::from_shared_input(Arc::from(input))
    }

    #[rstest::rstest]
    fn test_scan_structural_tokens() {
        let mut scanner = make_scanner("[]{}:-");
        assert_eq!(scanner.scan_token().unwrap(), Token::LeftBracket);
        assert_eq!(scanner.scan_token().unwrap(), Token::RightBracket);
        assert_eq!(scanner.scan_token().unwrap(), Token::LeftBrace);
        assert_eq!(scanner.scan_token().unwrap(), Token::RightBrace);
        assert_eq!(scanner.scan_token().unwrap(), Token::Colon);
        assert_eq!(scanner.scan_token().unwrap(), Token::Dash);
    }

    #[rstest::rstest]
    fn test_scan_numbers() {
        let mut scanner = make_scanner("42 3.141592653589793 -5");
        assert_eq!(scanner.scan_token().unwrap(), Token::Integer(42));
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::Number(f64::consts::PI)
        );
        assert_eq!(scanner.scan_token().unwrap(), Token::Integer(-5));
    }

    #[rstest::rstest]
    fn test_scan_booleans() {
        let mut scanner = make_scanner("true false");
        assert_eq!(scanner.scan_token().unwrap(), Token::Bool(true));
        assert_eq!(scanner.scan_token().unwrap(), Token::Bool(false));
    }

    #[rstest::rstest]
    fn test_scan_null() {
        let mut scanner = make_scanner("null");
        assert_eq!(scanner.scan_token().unwrap(), Token::Null);
    }

    #[rstest::rstest]
    fn test_scan_quoted_string() {
        let mut scanner = make_scanner(r#""hello world""#);
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::String("hello world".to_string(), true)
        );
    }

    #[rstest::rstest]
    fn test_scan_escaped_string() {
        let mut scanner = make_scanner(r#""hello\nworld""#);
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::String("hello\nworld".to_string(), true)
        );
    }

    #[rstest::rstest]
    fn test_scan_unquoted_string() {
        let mut scanner = make_scanner("hello");
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::String("hello".to_string(), false)
        );
    }

    #[rstest::rstest]
    fn test_read_rest_of_line_with_space_count() {
        let mut scanner = make_scanner(" world");
        let (content, leading_space, had_trailing_space) =
            scanner.read_rest_of_line_with_space_count();
        assert_eq!(content, "world");
        assert_eq!(leading_space, 1);
        assert!(!had_trailing_space);

        let mut scanner = make_scanner("world");
        let (content, leading_space, had_trailing_space) =
            scanner.read_rest_of_line_with_space_count();
        assert_eq!(content, "world");
        assert_eq!(leading_space, 0);
        assert!(!had_trailing_space);

        let mut scanner = make_scanner("(hello)");
        let (content, leading_space, had_trailing_space) =
            scanner.read_rest_of_line_with_space_count();
        assert_eq!(content, "(hello)");
        assert_eq!(leading_space, 0);
        assert!(!had_trailing_space);

        let mut scanner = make_scanner("");
        let (content, leading_space, had_trailing_space) =
            scanner.read_rest_of_line_with_space_count();
        assert_eq!(content, "");
        assert_eq!(leading_space, 0);
        assert!(!had_trailing_space);
    }

    #[rstest::rstest]
    fn test_parse_value_string() {
        let scanner = make_scanner("");
        assert_eq!(
            scanner.parse_value_string("hello").unwrap(),
            Token::String("hello".to_string(), false)
        );

        assert_eq!(
            scanner.parse_value_string("(hello)").unwrap(),
            Token::String("(hello)".to_string(), false)
        );

        assert_eq!(
            scanner
                .parse_value_string("Mostly Functions (3 of 3)")
                .unwrap(),
            Token::String("Mostly Functions (3 of 3)".to_string(), false)
        );
        assert_eq!(
            scanner.parse_value_string("0(f)").unwrap(),
            Token::String("0(f)".to_string(), false)
        );

        assert_eq!(
            scanner.parse_value_string("42").unwrap(),
            Token::Integer(42)
        );

        assert_eq!(
            scanner.parse_value_string("true").unwrap(),
            Token::Bool(true)
        );
        assert_eq!(
            scanner.parse_value_string("false").unwrap(),
            Token::Bool(false)
        );
        assert_eq!(scanner.parse_value_string("null").unwrap(), Token::Null);

        assert_eq!(
            scanner.parse_value_string(r#""hello world""#).unwrap(),
            Token::String("hello world".to_string(), true)
        );
    }

    #[rstest::rstest]
    fn test_number_followed_by_parenthesis() {
        let mut scanner = make_scanner("0(f)");
        let start = scanner.position;
        let range = scanner.scan_number_range(start);
        let token = scanner.parse_number(&scanner.input[range]).unwrap();

        assert_eq!(token, Token::String("0".to_string(), false));
    }

    #[rstest::rstest]
    fn test_tabs_in_indentation_rejected() {
        let mut scanner = Scanner::new("\tkey: value");
        let err = scanner.scan_token().unwrap_err();
        assert!(err
            .to_string()
            .contains("Tabs are not allowed in indentation"));
    }

    #[rstest::rstest]
    fn test_scan_quoted_string_invalid_escape() {
        let mut scanner = Scanner::new(r#""bad\x""#);
        let err = scanner.scan_token().unwrap_err();
        assert!(err.to_string().contains("Invalid escape sequence"));
    }

    #[rstest::rstest]
    fn test_scan_quoted_string_unterminated() {
        let mut scanner = Scanner::new("\"unterminated");
        let err = scanner.scan_token().unwrap_err();
        assert!(matches!(err, ToonError::UnexpectedEof));
    }

    #[rstest::rstest]
    fn test_parse_value_string_invalid_escape() {
        let scanner = Scanner::new("");
        let err = scanner.parse_value_string(r#""bad\x""#).unwrap_err();
        assert!(err.to_string().contains("Invalid escape sequence"));
    }

    #[rstest::rstest]
    fn test_parse_value_string_unexpected_trailing_chars() {
        let scanner = Scanner::new("");
        let err = scanner
            .parse_value_string(r#""hello" trailing"#)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("Unexpected characters after closing quote"));
    }

    #[rstest::rstest]
    fn test_parse_value_string_unterminated() {
        let scanner = Scanner::new("");
        let err = scanner.parse_value_string(r#""missing"#).unwrap_err();
        assert!(err.to_string().contains("Unterminated string"));
    }

    #[rstest::rstest]
    fn test_scan_number_leading_zero_string() {
        let mut scanner = Scanner::new("05");
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::String("05".to_string(), false)
        );
    }

    #[rstest::rstest]
    fn test_scan_number_trailing_char_string() {
        let mut scanner = Scanner::new("1x");
        assert_eq!(
            scanner.scan_token().unwrap(),
            Token::String("1x".to_string(), false)
        );
    }
}
