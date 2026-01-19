use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotImplemented,
    Encode,
    Decode,
    Serialize,
    Deserialize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorStage {
    Encode,
    Decode,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub stage: ErrorStage,
    pub message: String,
    pub location: Option<Location>,
}

impl Error {
    pub fn not_implemented(context: &'static str) -> Self {
        Self {
            kind: ErrorKind::NotImplemented,
            stage: ErrorStage::Unknown,
            message: format!("not implemented: {context}"),
            location: None,
        }
    }

    pub fn encode(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Encode,
            stage: ErrorStage::Encode,
            message: message.into(),
            location: None,
        }
    }

    pub fn decode(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Decode,
            stage: ErrorStage::Decode,
            message: message.into(),
            location: None,
        }
    }

    pub fn serialize(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Serialize,
            stage: ErrorStage::Encode,
            message: message.into(),
            location: None,
        }
    }

    pub fn deserialize(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Deserialize,
            stage: ErrorStage::Decode,
            message: message.into(),
            location: None,
        }
    }

    pub fn with_stage(mut self, stage: ErrorStage) -> Self {
        self.stage = stage;
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}
