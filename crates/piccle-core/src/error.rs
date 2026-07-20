//! Typed engine errors with stable spec-defined codes.
//!
//! Spec: piccle-spec/docs/14-conformance.md (validation stages) and
//! piccle-spec/test-vectors/invalid-expectations.json (canonical codes).
//! Error code strings are part of the public API and MUST NOT be renamed
//! without a SemVer-major bump.

use std::borrow::Cow;
use std::fmt;

/// Validation stage at which an error was raised.
///
/// The `Display` strings match the spec's machine-readable stage names in
/// piccle-spec/test-vectors/invalid-expectations.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// JSON parse stage (malformed input).
    Parse,
    /// JSON Schema validation stage.
    Schema,
    /// Semantic validation stage.
    Semantic,
    /// Engine limit rejection after successful validation.
    Unsupported,
    /// Internal engine fault (e.g. non-finite DSP state).
    Internal,
    /// Parser resource-limit rejection (input too large or too deep).
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

/// Result alias for engine operations.
pub type PiccleResult<T> = Result<T, PiccleError>;

/// Engine error with a stable code, JSON path, and message.
///
/// Shape: `{ stage, code, path, msg }` per piccle-spec/docs/14-conformance.md.
#[derive(Debug, thiserror::Error)]
pub enum PiccleError {
    /// Parser resource limit exceeded (input size or nesting depth).
    #[error("resource rejected: {limit} exceeded ({reason})")]
    ResourceRejected {
        /// Name of the exceeded limit.
        limit: &'static str,
        /// Human-readable reason.
        reason: &'static str,
    },

    /// Malformed JSON (parse stage). Parse-stage errors always use path `$`.
    #[error("malformed JSON at {path}: {code}")]
    Malformed {
        /// Stable code (`json.malformed`, `json.duplicate_member`,
        /// `json.non_finite_number`, `json.number_out_of_range`).
        code: &'static str,
        /// JSON path (always `$` at the parse stage).
        path: String,
    },

    /// Document does not satisfy the v1 JSON Schema.
    #[error("schema-invalid at {path}: {code} — {msg}")]
    SchemaInvalid {
        /// Stable code (`schema.<keyword>`).
        code: &'static str,
        /// JSON path of the offending member.
        path: String,
        /// Human-readable detail.
        msg: String,
    },

    /// Document is schema-valid but semantically inconsistent.
    #[error("semantically invalid at {path}: {code} — {msg}")]
    SemanticInvalid {
        /// Stable code (`semantic.<reason>`).
        code: &'static str,
        /// JSON path of the offending member.
        path: String,
        /// Human-readable detail.
        msg: String,
    },

    /// Valid document that exceeds a published engine limit.
    #[error("unsupported by this engine: {limit} exceeded (actual {actual}, max {max})")]
    Unsupported {
        /// Name of the exceeded limit.
        limit: &'static str,
        /// Observed value.
        actual: String,
        /// Published maximum.
        max: String,
    },

    /// Internal engine fault (never a validation outcome).
    #[error("internal engine error: {0}")]
    Internal(Cow<'static, str>),
}

impl PiccleError {
    /// Builds a parse-stage error. The path is always `$` per spec.
    #[must_use]
    pub fn malformed(code: &'static str) -> Self {
        Self::Malformed { code, path: "$".to_owned() }
    }

    /// Builds a schema-stage error.
    #[must_use]
    pub const fn schema(code: &'static str, path: String, msg: String) -> Self {
        Self::SchemaInvalid { code, path, msg }
    }

    /// Builds a semantic-stage error.
    #[must_use]
    pub const fn semantic(code: &'static str, path: String, msg: String) -> Self {
        Self::SemanticInvalid { code, path, msg }
    }

    /// Builds an internal engine error.
    ///
    /// Static messages remain allocation-free, including when constructed
    /// from the render loop. Owned messages are available for preparation
    /// diagnostics that need dynamic context.
    pub fn internal(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::Internal(msg.into())
    }

    /// The validation stage of this error.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        match self {
            Self::ResourceRejected { .. } => Stage::ResourceRejected,
            Self::Malformed { .. } => Stage::Parse,
            Self::SchemaInvalid { .. } => Stage::Schema,
            Self::SemanticInvalid { .. } => Stage::Semantic,
            Self::Unsupported { .. } => Stage::Unsupported,
            Self::Internal(_) => Stage::Internal,
        }
    }

    /// The stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ResourceRejected { .. } => "resource.rejected",
            Self::Malformed { code, .. }
            | Self::SchemaInvalid { code, .. }
            | Self::SemanticInvalid { code, .. } => code,
            Self::Unsupported { .. } => "engine.unsupported",
            Self::Internal(_) => "internal",
        }
    }

    /// The JSON path of the offending member (`$` when not applicable).
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Malformed { path, .. }
            | Self::SchemaInvalid { path, .. }
            | Self::SemanticInvalid { path, .. } => path,
            Self::ResourceRejected { .. } | Self::Unsupported { .. } | Self::Internal(_) => "$",
        }
    }

    /// The human-readable detail message.
    #[must_use]
    pub fn msg(&self) -> String {
        match self {
            Self::ResourceRejected { limit, reason } => {
                format!("resource limit {limit} exceeded: {reason}")
            }
            Self::Malformed { code, .. } => (*code).to_owned(),
            Self::SchemaInvalid { msg, .. } | Self::SemanticInvalid { msg, .. } => msg.clone(),
            Self::Unsupported { limit, actual, max } => {
                format!("engine limit {limit} exceeded: actual {actual}, max {max}")
            }
            Self::Internal(msg) => msg.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_display_names_match_the_conformance_contract() {
        let names = [
            (Stage::Parse, "parse"),
            (Stage::Schema, "schema"),
            (Stage::Semantic, "semantic"),
            (Stage::Unsupported, "unsupported"),
            (Stage::Internal, "internal"),
            (Stage::ResourceRejected, "resource-rejected"),
        ];
        for (stage, name) in names {
            assert_eq!(stage.to_string(), name);
        }
    }

    #[test]
    fn resource_rejected_reports_stage_code_and_path() {
        let error = PiccleError::ResourceRejected { limit: "max_input_bytes", reason: "too big" };
        assert_eq!(
            (error.stage(), error.code(), error.path()),
            (Stage::ResourceRejected, "resource.rejected", "$")
        );
    }

    #[test]
    fn unsupported_reports_stage_code_and_path() {
        let error = PiccleError::Unsupported {
            limit: "max_layers",
            actual: "200".to_string(),
            max: "128".to_string(),
        };
        assert_eq!(
            error.stage().to_string() + "/" + error.code() + "/" + error.path(),
            "unsupported/engine.unsupported/$"
        );
    }

    #[test]
    fn internal_reports_stage_code_and_path() {
        let error = PiccleError::internal("boom");
        assert_eq!((error.stage(), error.code(), error.path()), (Stage::Internal, "internal", "$"));
    }

    #[test]
    fn msg_for_resource_rejected_includes_limit_and_reason() {
        let error = PiccleError::ResourceRejected { limit: "max_input_bytes", reason: "too big" };
        assert_eq!(error.msg(), "resource limit max_input_bytes exceeded: too big");
    }

    #[test]
    fn msg_for_malformed_is_the_code() {
        let error = PiccleError::malformed("json.malformed");
        assert_eq!(error.msg(), "json.malformed");
    }

    #[test]
    fn msg_for_unsupported_includes_actual_and_max() {
        let error = PiccleError::Unsupported {
            limit: "max_layers",
            actual: "200".to_string(),
            max: "128".to_string(),
        };
        assert_eq!(error.msg(), "engine limit max_layers exceeded: actual 200, max 128");
    }

    #[test]
    fn msg_for_schema_and_semantic_and_internal_is_the_detail() {
        let schema =
            PiccleError::schema("schema.type", "$".to_string(), "expected integer".to_string());
        let semantic = PiccleError::semantic(
            "semantic.duplicate_layer_id",
            "$".to_string(),
            "dup".to_string(),
        );
        let internal = PiccleError::internal("bug");
        assert_eq!(
            (schema.msg(), semantic.msg(), internal.msg()),
            ("expected integer".to_string(), "dup".to_string(), "bug".to_string())
        );
    }
}
