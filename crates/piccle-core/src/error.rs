use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Parse,
    Schema,
    Semantic,
    Unsupported,
    Internal,
    ResourceRejected,
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse => write!(f, "parse"),
            Self::Schema => write!(f, "schema"),
            Self::Semantic => write!(f, "semantic"),
            Self::Unsupported => write!(f, "unsupported"),
            Self::Internal => write!(f, "internal"),
            Self::ResourceRejected => write!(f, "resource-rejected"),
        }
    }
}

pub type PiccleResult<T> = Result<T, PiccleError>;

#[derive(Debug, thiserror::Error)]
pub enum PiccleError {
    #[error("resource rejected: {limit} exceeded ({reason})")]
    ResourceRejected { limit: &'static str, reason: &'static str },

    #[error("malformed JSON at {path}: {code}")]
    Malformed { code: &'static str, path: String },

    #[error("schema-invalid at {path}: {code} — {msg}")]
    SchemaInvalid { code: &'static str, path: String, msg: String },

    #[error("semantically invalid at {path}: {code} — {msg}")]
    SemanticInvalid { code: &'static str, path: String, msg: String },

    #[error("unsupported by this engine: {limit} exceeded")]
    Unsupported { limit: &'static str, actual: String, max: String },

    #[error("internal engine error: {0}")]
    Internal(String),
}

impl PiccleError {
    pub fn stage(&self) -> Stage {
        match self {
            Self::ResourceRejected { .. } => Stage::ResourceRejected,
            Self::Malformed { .. } => Stage::Parse,
            Self::SchemaInvalid { .. } => Stage::Schema,
            Self::SemanticInvalid { .. } => Stage::Semantic,
            Self::Unsupported { .. } => Stage::Unsupported,
            Self::Internal(_) => Stage::Internal,
        }
    }
}
