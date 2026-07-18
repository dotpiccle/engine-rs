#![forbid(unsafe_code)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod plan;
pub mod renderer;

pub use plan::RenderPlan;
pub use renderer::Renderer;
