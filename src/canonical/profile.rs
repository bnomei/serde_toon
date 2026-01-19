#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanonicalDelimiter {
    #[default]
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
            delimiter: CanonicalDelimiter::default(),
        }
    }
}
