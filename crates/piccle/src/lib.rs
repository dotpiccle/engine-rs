#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use piccle_core as core;
pub use piccle_dsp as dsp;
pub use piccle_render as render;
pub use piccle_validate as validate;

pub mod prelude {
    pub use piccle_core::prelude::*;
    pub use piccle_render::{RenderPlan, Renderer};
}
