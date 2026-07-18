#![forbid(unsafe_code)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod curve;
pub mod error;
pub mod model;
pub mod schedule;

pub use error::{PiccleError, PiccleResult, Stage};

pub mod prelude {
    pub use crate::curve::Curve;
    pub use crate::error::PiccleError;
    pub use crate::model::*;
    pub use crate::schedule::*;
}
