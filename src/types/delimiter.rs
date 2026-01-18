use std::fmt;

use serde::{Deserialize, Serialize};

/// Delimiter character used to separate array elements.
///
/// # Examples
/// ```
/// use serde_toon::Delimiter;
///
/// let delim = Delimiter::Pipe;
/// assert_eq!(delim.as_char(), '|');
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Delimiter {
    #[default]
    Comma,
    Tab,
    Pipe,
}

impl Delimiter {
    /// Get the character representation of this delimiter.
    ///
    /// # Examples
    /// ```
    /// use serde_toon::Delimiter;
    ///
    /// assert_eq!(Delimiter::Comma.as_char(), ',');
    /// ```
    pub fn as_char(&self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Pipe => '|',
        }
    }

    /// Get the string representation for metadata (empty for comma, char for others).
    ///
    /// # Examples
    /// ```
    /// use serde_toon::Delimiter;
    ///
    /// assert_eq!(Delimiter::Comma.as_metadata_str(), "");
    /// assert_eq!(Delimiter::Tab.as_metadata_str(), "\t");
    /// ```
    pub fn as_metadata_str(&self) -> &'static str {
        match self {
            Delimiter::Comma => "",
            Delimiter::Tab => "\t",
            Delimiter::Pipe => "|",
        }
    }

    /// Parse a delimiter from a character.
    ///
    /// # Examples
    /// ```
    /// use serde_toon::Delimiter;
    ///
    /// assert_eq!(Delimiter::from_char('|'), Some(Delimiter::Pipe));
    /// ```
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            ',' => Some(Delimiter::Comma),
            '\t' => Some(Delimiter::Tab),
            '|' => Some(Delimiter::Pipe),
            _ => None,
        }
    }

    /// Check if the delimiter character appears in the string.
    ///
    /// # Examples
    /// ```
    /// use serde_toon::Delimiter;
    ///
    /// assert!(Delimiter::Comma.contains_in("a,b"));
    /// ```
    pub fn contains_in(&self, s: &str) -> bool {
        s.contains(self.as_char())
    }
}

impl fmt::Display for Delimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_delimiter_conversion() {
        assert_eq!(Delimiter::Comma.as_char(), ',');
        assert_eq!(Delimiter::Tab.as_char(), '\t');
        assert_eq!(Delimiter::Pipe.as_char(), '|');
    }

    #[rstest::rstest]
    fn test_delimiter_from_char() {
        assert_eq!(Delimiter::from_char(','), Some(Delimiter::Comma));
        assert_eq!(Delimiter::from_char('\t'), Some(Delimiter::Tab));
        assert_eq!(Delimiter::from_char('|'), Some(Delimiter::Pipe));
        assert_eq!(Delimiter::from_char('x'), None);
    }

    #[rstest::rstest]
    fn test_delimiter_contains() {
        assert!(Delimiter::Comma.contains_in("a,b,c"));
        assert!(Delimiter::Tab.contains_in("a\tb\tc"));
        assert!(Delimiter::Pipe.contains_in("a|b|c"));
        assert!(!Delimiter::Comma.contains_in("abc"));
    }
}
