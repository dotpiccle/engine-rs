//! Validation pipeline for untrusted Piccle documents.
//!
//! Implements the pre-render security boundary of
//! piccle-spec/docs/11-engine-safety.md §Untrusted input: parser resource
//! limits → malformed JSON checks → JSON Schema validation → semantic
//! validation → default resolution into the immutable document model.

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod json;
mod resolve;
mod schema;
mod semantic;

use piccle_core::error::PiccleResult;
use piccle_core::model::Document;

/// The validation entry point. Stateless and cheap to construct.
#[derive(Debug, Default, Clone, Copy)]
pub struct Validator;

impl Validator {
    /// Creates a validator.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Runs parser limits, parse, schema, and semantic stages without
    /// resolving the model. This is the fuzz-target surface: it must never
    /// panic, hang, or allocate unboundedly on arbitrary input.
    ///
    /// # Errors
    ///
    /// Any validation-stage `PiccleError`.
    pub fn check(bytes: &[u8]) -> PiccleResult<()> {
        let value = json::parse(bytes)?;
        schema::validate_document(&value)?;
        semantic::validate_semantics(&value)?;
        Ok(())
    }

    /// Runs the full pipeline and resolves the typed document model with
    /// all defaults materialized.
    ///
    /// # Errors
    ///
    /// Any validation-stage `PiccleError`, or `Internal` when resolution is
    /// reached without a valid document.
    pub fn validate(bytes: &[u8]) -> PiccleResult<Document> {
        let value = json::parse(bytes)?;
        schema::validate_document(&value)?;
        semantic::validate_semantics(&value)?;
        resolve::resolve_document(&value)
    }
}

/// Convenience free function: full pipeline through resolution.
///
/// # Errors
///
/// See [`Validator::validate`].
pub fn validate(bytes: &[u8]) -> PiccleResult<Document> {
    Validator::validate(bytes)
}
