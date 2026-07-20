//! Deterministic binary64 DSP primitives used by the Piccle renderer.
//!
//! The crate implements the specification-defined oscillators, seeded noise,
//! filters, and canonical reverb. It does not parse documents or allocate
//! render plans.

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod denormal;

pub mod filter;
pub mod measure;
pub mod noise;
pub mod oscillator;
pub mod reverb;
