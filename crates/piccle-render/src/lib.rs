//! Immutable render plans and the streaming Piccle render loop.
//!
//! Document parsing and validation deliberately live outside this crate. Most
//! applications should use the preparation API from the `piccle` crate.

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod plan;
pub mod renderer;

pub use plan::RenderPlan;
pub use renderer::Renderer;
