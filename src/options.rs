#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delimiter {
    #[default]
    Comma,
    Tab,
    Pipe,
}

impl Delimiter {
    pub fn as_char(self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Pipe => '|',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Spaces(usize),
}

impl Indent {
    pub fn spaces(count: usize) -> Self {
        Indent::Spaces(count)
    }
}

impl Default for Indent {
    fn default() -> Self {
        Indent::Spaces(2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyFolding {
    #[default]
    Off,
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpandPaths {
    #[default]
    Off,
    Safe,
}

#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    pub indent: Indent,
    pub delimiter: Delimiter,
    pub key_folding: KeyFolding,
    pub flatten_depth: Option<usize>,
}

impl EncodeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_indent(mut self, indent: Indent) -> Self {
        self.indent = indent;
        self
    }

    pub fn with_delimiter(mut self, delimiter: Delimiter) -> Self {
        self.delimiter = delimiter;
        self
    }

    pub fn with_key_folding(mut self, key_folding: KeyFolding) -> Self {
        self.key_folding = key_folding;
        self
    }

    pub fn with_flatten_depth(mut self, flatten_depth: Option<usize>) -> Self {
        self.flatten_depth = flatten_depth;
        self
    }
}

#[derive(Debug, Clone)]
pub struct DecodeOptions {
    pub indent: Indent,
    pub strict: bool,
    pub expand_paths: ExpandPaths,
}

impl DecodeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_indent(mut self, indent: Indent) -> Self {
        self.indent = indent;
        self
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    pub fn with_expand_paths(mut self, expand_paths: ExpandPaths) -> Self {
        self.expand_paths = expand_paths;
        self
    }
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            indent: Indent::default(),
            strict: true,
            expand_paths: ExpandPaths::default(),
        }
    }
}
