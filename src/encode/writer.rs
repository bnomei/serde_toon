use crate::{
    types::{Delimiter, EncodeOptions, Number, ToonResult},
    utils::{
        number::write_canonical_number_into,
        string::{escape_string_into, is_valid_unquoted_key, needs_quoting},
        QuotingContext,
    },
};

pub(crate) struct Writer {
    buffer: Vec<u8>,
    pub(crate) options: EncodeOptions,
    active_delimiters: Vec<Delimiter>,
    indent_unit: String,
    indent_cache: Vec<String>,
}

impl Writer {
    pub fn new(options: EncodeOptions) -> Self {
        let indent_unit = " ".repeat(options.indent.get_spaces());
        Self {
            buffer: Vec::new(),
            active_delimiters: vec![options.delimiter],
            options,
            indent_unit,
            indent_cache: vec![String::new()],
        }
    }

    pub fn finish(self) -> String {
        String::from_utf8(self.buffer).expect("writer output must be valid UTF-8")
    }

    /// Finish writing and return the output as raw bytes.
    ///
    /// # Examples
    /// ```ignore
    /// use serde_toon::EncodeOptions;
    /// use serde_toon::encode::writer::Writer;
    ///
    /// let mut writer = Writer::new(EncodeOptions::default());
    /// writer.write_str("a: 1").unwrap();
    /// let bytes = writer.finish_bytes();
    /// assert!(!bytes.is_empty());
    /// ```ignore
    pub fn finish_bytes(self) -> Vec<u8> {
        self.buffer
    }

    pub fn write_str(&mut self, s: &str) -> ToonResult<()> {
        self.buffer.extend_from_slice(s.as_bytes());
        Ok(())
    }

    pub fn write_char(&mut self, ch: char) -> ToonResult<()> {
        if ch.is_ascii() {
            self.buffer.push(ch as u8);
            return Ok(());
        }

        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        self.buffer.extend_from_slice(encoded.as_bytes());
        Ok(())
    }

    pub fn write_newline(&mut self) -> ToonResult<()> {
        self.buffer.push(b'\n');
        Ok(())
    }

    pub fn write_indent(&mut self, depth: usize) -> ToonResult<()> {
        if depth == 0 || self.indent_unit.is_empty() {
            return Ok(());
        }
        if depth >= self.indent_cache.len() {
            self.extend_indent_cache(depth);
        }
        self.buffer
            .extend_from_slice(self.indent_cache[depth].as_bytes());
        Ok(())
    }

    pub fn write_delimiter(&mut self) -> ToonResult<()> {
        self.buffer.push(self.options.delimiter.as_char() as u8);
        Ok(())
    }

    pub fn write_key(&mut self, key: &str) -> ToonResult<()> {
        if is_valid_unquoted_key(key) {
            self.write_str(key)
        } else {
            self.write_quoted_string(key)
        }
    }

    pub fn write_array_header(
        &mut self,
        key: Option<&str>,
        length: usize,
        fields: Option<&[&str]>,
        depth: usize,
    ) -> ToonResult<()> {
        if let Some(k) = key {
            if depth > 0 {
                self.write_indent(depth)?;
            }
            self.write_key(k)?;
        }

        self.write_char('[')?;
        self.write_usize(length)?;

        // Only write delimiter in header if it's not comma (comma is default/implied)
        if self.options.delimiter != Delimiter::Comma {
            self.write_delimiter()?;
        }

        self.write_char(']')?;

        // Write field list for tabular arrays: {field1,field2}
        if let Some(field_list) = fields {
            self.write_char('{')?;
            for (i, field) in field_list.iter().enumerate() {
                if i > 0 {
                    self.write_delimiter()?;
                }
                self.write_key(field)?;
            }
            self.write_char('}')?;
        }

        self.write_char(':')
    }

    pub fn write_empty_array_with_key(
        &mut self,
        key: Option<&str>,
        depth: usize,
    ) -> ToonResult<()> {
        if let Some(k) = key {
            if depth > 0 {
                self.write_indent(depth)?;
            }
            self.write_key(k)?;
        }
        self.write_char('[')?;
        self.write_usize(0)?;

        if self.options.delimiter != Delimiter::Comma {
            self.write_delimiter()?;
        }

        self.write_char(']')?;
        self.write_char(':')
    }

    pub fn needs_quoting(&self, s: &str, context: QuotingContext) -> bool {
        // Use active delimiter for array values, document delimiter for object values
        let delim_char = match context {
            QuotingContext::ObjectValue => self.get_document_delimiter_char(),
            QuotingContext::ArrayValue => self.get_active_delimiter_char(),
        };
        needs_quoting(s, delim_char)
    }

    pub fn write_quoted_string(&mut self, s: &str) -> ToonResult<()> {
        self.buffer.push(b'"');
        escape_string_into(&mut self.buffer, s);
        self.buffer.push(b'"');
        Ok(())
    }

    pub fn write_canonical_number(&mut self, n: &Number) -> ToonResult<()> {
        write_canonical_number_into(n, &mut self.buffer);
        Ok(())
    }

    pub fn write_usize(&mut self, value: usize) -> ToonResult<()> {
        let mut buf = itoa::Buffer::new();
        self.buffer.extend_from_slice(buf.format(value as u64).as_bytes());
        Ok(())
    }

    pub fn push_active_delimiter(&mut self, delim: Delimiter) {
        self.active_delimiters.push(delim);
    }
    pub fn pop_active_delimiter(&mut self) {
        if self.active_delimiters.len() > 1 {
            self.active_delimiters.pop();
        }
    }
    fn get_active_delimiter_char(&self) -> char {
        self.active_delimiters
            .last()
            .unwrap_or(&self.options.delimiter)
            .as_char()
    }

    fn get_document_delimiter_char(&self) -> char {
        self.options.delimiter.as_char()
    }

    fn extend_indent_cache(&mut self, depth: usize) {
        while self.indent_cache.len() <= depth {
            let next = match self.indent_cache.last() {
                Some(prev) => {
                    let mut s = String::with_capacity(prev.len() + self.indent_unit.len());
                    s.push_str(prev);
                    s.push_str(&self.indent_unit);
                    s
                }
                None => String::new(),
            };
            self.indent_cache.push(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_writer_basic() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_str("hello").unwrap();
        writer.write_str(" ").unwrap();
        writer.write_str("world").unwrap();

        assert_eq!(writer.finish(), "hello world");
    }

    #[rstest::rstest]
    fn test_write_delimiter() {
        let mut opts = EncodeOptions::default();
        let mut writer = Writer::new(opts.clone());

        writer.write_str("a").unwrap();
        writer.write_delimiter().unwrap();
        writer.write_str("b").unwrap();

        assert_eq!(writer.finish(), "a,b");

        opts = opts.with_delimiter(Delimiter::Pipe);
        let mut writer = Writer::new(opts);

        writer.write_str("a").unwrap();
        writer.write_delimiter().unwrap();
        writer.write_str("b").unwrap();

        assert_eq!(writer.finish(), "a|b");
    }

    #[rstest::rstest]
    fn test_write_indent() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_indent(0).unwrap();
        writer.write_str("a").unwrap();
        writer.write_newline().unwrap();

        writer.write_indent(1).unwrap();
        writer.write_str("b").unwrap();
        writer.write_newline().unwrap();

        writer.write_indent(2).unwrap();
        writer.write_str("c").unwrap();

        assert_eq!(writer.finish(), "a\n  b\n    c");
    }

    #[rstest::rstest]
    fn test_write_array_header() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer
            .write_array_header(Some("items"), 3, None, 0)
            .unwrap();
        assert_eq!(writer.finish(), "items[3]:");

        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);
        let fields = vec!["id", "name"];

        writer
            .write_array_header(Some("users"), 2, Some(&fields), 0)
            .unwrap();
        assert_eq!(writer.finish(), "users[2]{id,name}:");
    }

    #[rstest::rstest]
    fn test_write_array_header_with_pipe_delimiter() {
        let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
        let mut writer = Writer::new(opts);

        writer
            .write_array_header(Some("items"), 3, None, 0)
            .unwrap();
        assert_eq!(writer.finish(), "items[3|]:");

        let opts = EncodeOptions::new().with_delimiter(Delimiter::Pipe);
        let mut writer = Writer::new(opts);
        let fields = vec!["id", "name"];

        writer
            .write_array_header(Some("users"), 2, Some(&fields), 0)
            .unwrap();
        assert_eq!(writer.finish(), "users[2|]{id|name}:");
    }

    #[rstest::rstest]
    fn test_write_key_with_special_chars() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_key("normal_key").unwrap();
        assert_eq!(writer.finish(), "normal_key");

        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_key("key:with:colons").unwrap();
        assert_eq!(writer.finish(), "\"key:with:colons\"");
    }

    #[rstest::rstest]
    fn test_write_quoted_string() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_quoted_string("hello world").unwrap();
        assert_eq!(writer.finish(), "\"hello world\"");

        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_quoted_string("say \"hi\"").unwrap();
        assert_eq!(writer.finish(), r#""say \"hi\"""#);
    }

    #[rstest::rstest]
    fn test_needs_quoting() {
        let opts = EncodeOptions::default();
        let writer = Writer::new(opts);
        let ctx = QuotingContext::ObjectValue;

        assert!(!writer.needs_quoting("hello", ctx));
        assert!(writer.needs_quoting("hello,world", ctx));
        assert!(writer.needs_quoting("true", ctx));
        assert!(writer.needs_quoting("false", ctx));
        assert!(writer.needs_quoting("null", ctx));
        assert!(writer.needs_quoting("123", ctx));
        assert!(writer.needs_quoting("", ctx));
        assert!(writer.needs_quoting("hello:world", ctx));
    }

    #[rstest::rstest]
    fn test_write_empty_array() {
        let opts = EncodeOptions::default();
        let mut writer = Writer::new(opts);

        writer.write_empty_array_with_key(Some("items"), 0).unwrap();
        assert_eq!(writer.finish(), "items[0]:");
    }
}
