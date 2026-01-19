use std::error::Error as StdError;

use thiserror::Error as ThisError;

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

#[derive(Debug, ThisError)]
#[error("{message}")]
pub struct Error {
    pub kind: ErrorKind,
    pub stage: ErrorStage,
    pub message: String,
    pub location: Option<Location>,
    #[source]
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl Error {
    pub fn not_implemented(context: &'static str) -> Self {
        Self::new(
            ErrorKind::NotImplemented,
            ErrorStage::Unknown,
            format!("not implemented: {context}"),
        )
    }

    pub fn encode(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Encode, ErrorStage::Encode, message)
    }

    pub fn encode_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_source(ErrorKind::Encode, ErrorStage::Encode, message, source)
    }

    pub fn decode(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Decode, ErrorStage::Decode, message)
    }

    pub fn decode_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_source(ErrorKind::Decode, ErrorStage::Decode, message, source)
    }

    pub fn serialize(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Serialize, ErrorStage::Encode, message)
    }

    pub fn serialize_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_source(ErrorKind::Serialize, ErrorStage::Encode, message, source)
    }

    pub fn deserialize(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Deserialize, ErrorStage::Decode, message)
    }

    pub fn deserialize_with_source(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::new_with_source(ErrorKind::Deserialize, ErrorStage::Decode, message, source)
    }

    pub fn with_stage(mut self, stage: ErrorStage) -> Self {
        self.stage = stage;
        self
    }

    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    fn new(kind: ErrorKind, stage: ErrorStage, message: impl Into<String>) -> Self {
        Self {
            kind,
            stage,
            message: message.into(),
            location: None,
            source: None,
        }
    }

    fn new_with_source(
        kind: ErrorKind,
        stage: ErrorStage,
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            stage,
            message: message.into(),
            location: None,
            source: Some(Box::new(source)),
        }
    }
}
