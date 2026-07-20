//! Core document model, errors, curve primitives, and frame schedule for the
//! Piccle engine.
//!
//! This crate is platform-neutral and carries no JSON or DSP dependencies.

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod curve;
pub mod error;
pub mod model;
pub mod schedule;

pub use error::{PiccleError, PiccleResult, Stage};

/// Common imports for engine crates.
pub mod prelude {
    pub use crate::curve::Curve;
    pub use crate::error::{PiccleError, PiccleResult, Stage};
    pub use crate::model::*;
    pub use crate::schedule::{CANONICAL_SAMPLE_RATE, frame_at, render_frequency_max};
}
