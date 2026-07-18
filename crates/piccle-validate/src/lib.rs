#![forbid(unsafe_code)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod json;
pub mod schema;
pub mod semantic;

pub use json::JsonParser;
pub use schema::SchemaValidator;
pub use semantic::SemanticValidator;
