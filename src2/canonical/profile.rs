//! Canonical profile rules and defaults.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalDelimiter {
    Comma,
    Tab,
    Pipe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProfile {
    pub indent_spaces: usize,
    pub delimiter: CanonicalDelimiter,
}

impl Default for CanonicalProfile {
    fn default() -> Self {
        Self {
            indent_spaces: 2,
            delimiter: CanonicalDelimiter::Comma,
        }
    }
}

impl CanonicalDelimiter {
    pub fn as_char(self) -> char {
        match self {
            CanonicalDelimiter::Comma => ',',
            CanonicalDelimiter::Tab => '\t',
            CanonicalDelimiter::Pipe => '|',
        }
    }

    pub fn as_byte(self) -> u8 {
        self.as_char() as u8
    }
}
