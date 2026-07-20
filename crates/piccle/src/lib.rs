//! The Piccle engine — a portable, secure, deterministic implementation of
//! the Piccle micro-audio format.
//!
//! [`prepare`] is the only way to reach the render path: it runs the full
//! untrusted-input pipeline of piccle-spec/docs/11-engine-safety.md (parser
//! limits → malformed → schema → semantic → engine limits) and compiles the
//! immutable [`RenderPlan`] that [`Renderer`] consumes. Low-level crates are
//! implementation details; applications should depend on this crate.
//!
//! # Example
//!
//! ```no_run
//! let bytes = std::fs::read("tap.piccle.json")?;
//! let plan = piccle::prepare(&bytes)?;
//! let mut renderer = piccle::Renderer::new(&plan);
//! let mut output = [0.0_f32; 512 * 2];
//!
//! while !renderer.is_finished() {
//!     let frames = renderer.render_into(&mut output)?;
//!     let interleaved_stereo = &output[..frames * 2];
//!     // Forward `interleaved_stereo` to the host audio API.
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![warn(clippy::missing_errors_doc)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use piccle_core::error::{PiccleError, PiccleResult, Stage};
use piccle_core::model::Document;
pub use piccle_core::schedule::CANONICAL_SAMPLE_RATE;

/// An immutable plan produced by the complete [`prepare`] security boundary.
///
/// Its low-level representation is intentionally private, so users of this
/// umbrella crate cannot construct a plan from an unvalidated document.
#[derive(Debug, Clone)]
pub struct RenderPlan {
    inner: piccle_render::plan::RenderPlan,
}

impl RenderPlan {
    /// Render sample rate in hertz.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    /// Absolute frame at which the dry mix ends.
    #[must_use]
    pub fn dry_end_frame(&self) -> u64 {
        self.inner.dry_end_frame()
    }

    /// Total output length in stereo frames.
    #[must_use]
    pub fn output_frames(&self) -> u64 {
        self.inner.output_frames()
    }
}

/// Allocation-free streaming renderer over a validated [`RenderPlan`].
#[derive(Debug)]
pub struct Renderer<'a> {
    inner: piccle_render::renderer::Renderer<'a>,
}

impl<'a> Renderer<'a> {
    /// Creates a zero-state renderer for `plan`.
    #[must_use]
    pub fn new(plan: &'a RenderPlan) -> Self {
        Self { inner: piccle_render::renderer::Renderer::new(&plan.inner) }
    }

    /// Renders the complete plan into allocated interleaved stereo samples.
    ///
    /// # Errors
    ///
    /// Returns [`PiccleError::Unsupported`] when the convenience allocation
    /// exceeds [`MAX_RENDER_TO_VEC_BYTES`], or [`PiccleError::Internal`] if
    /// allocation fails or rendering produces a non-finite sample.
    pub fn render_to_vec(plan: &RenderPlan) -> PiccleResult<Vec<f32>> {
        piccle_render::renderer::Renderer::render_to_vec(&plan.inner)
    }

    /// Absolute frame that will be emitted next.
    #[must_use]
    pub fn frame_cursor(&self) -> u64 {
        self.inner.frame_cursor()
    }

    /// Whether every output frame has been emitted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }

    /// Restores all renderer state to its initial value.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// Renders as many interleaved stereo frames as fit in `output`.
    ///
    /// # Errors
    ///
    /// Returns an internal error if rendering produces a non-finite sample.
    pub fn render_into(&mut self, output: &mut [f32]) -> PiccleResult<usize> {
        self.inner.render_into(output)
    }
}

/// Maximum document duration this engine renders (10 minutes).
pub const MAX_DURATION_MS: u64 = 600_000;

/// Maximum number of layers per document.
pub const MAX_LAYERS: usize = 128;

/// Maximum number of serial filters per layer.
pub const MAX_FILTERS_PER_LAYER: usize = 16;

/// Maximum number of entries in any single contour.
pub const MAX_CONTOUR_ENTRIES: usize = 1024;

/// Maximum declared reverb tail (60 seconds).
///
/// A nonzero wet path is also subject to [`MAX_REVERB_TAIL_FRAMES`], so the
/// effective millisecond ceiling is lower above the canonical sample rate.
pub const MAX_TAIL_MS: u64 = 60_000;

/// Maximum reverb-tail frames prepared for a nonzero wet path.
///
/// This preserves the 60-second canonical-profile ceiling while preventing a
/// high-rate profile from multiplying calibration scratch and CPU cost. A
/// zero-amount reverb does not construct wet state and is exempt.
pub const MAX_REVERB_TAIL_FRAMES: u64 = MAX_TAIL_MS * CANONICAL_SAMPLE_RATE as u64 / 1_000;

/// Highest render sample rate supported by this engine.
///
/// This engine-defined ceiling bounds frame-derived allocations before plan
/// construction and includes standard high-resolution application profiles.
pub const MAX_SAMPLE_RATE: u32 = 192_000;

/// Maximum allocation made by [`Renderer::render_to_vec`] (64 MiB).
///
/// Use chunked [`Renderer::render_into`] for longer output timelines.
pub const MAX_RENDER_TO_VEC_BYTES: u64 = piccle_render::renderer::MAX_RENDER_TO_VEC_BYTES;

/// Validates `bytes` and compiles a canonical-profile (48 kHz) render plan.
///
/// This is the ONLY way to reach the render path. A document that passes
/// format validation but exceeds a published engine limit is reported as
/// [`PiccleError::Unsupported`], never as a validation failure.
///
/// # Errors
///
/// Any validation-stage [`PiccleError`], or `Unsupported` when a valid
/// document exceeds a published engine limit.
pub fn prepare(bytes: &[u8]) -> PiccleResult<RenderPlan> {
    prepare_with_rate(bytes, CANONICAL_SAMPLE_RATE)
}

/// Validates `bytes` and compiles a render plan for the render profile at
/// `sample_rate` (at least 8000 Hz per
/// piccle-spec/docs/11-engine-safety.md §Additional engine render profiles).
///
/// # Errors
///
/// Any validation-stage [`PiccleError`], or `Unsupported` when a valid
/// document, render rate, or rate-dependent reverb preparation exceeds a
/// published engine limit.
pub fn prepare_with_rate(bytes: &[u8], sample_rate: u32) -> PiccleResult<RenderPlan> {
    if sample_rate < MIN_SAMPLE_RATE {
        return Err(PiccleError::Unsupported {
            limit: "min_sample_rate",
            actual: sample_rate.to_string(),
            max: MIN_SAMPLE_RATE.to_string(),
        });
    }
    if sample_rate > MAX_SAMPLE_RATE {
        return Err(PiccleError::Unsupported {
            limit: "max_sample_rate",
            actual: sample_rate.to_string(),
            max: MAX_SAMPLE_RATE.to_string(),
        });
    }
    let document = piccle_validate::Validator::validate(bytes)?;
    check_engine_limits(&document, sample_rate)?;
    Ok(RenderPlan {
        inner: piccle_render::plan::RenderPlan::compile_validated(&document, sample_rate),
    })
}

/// Lowest render sample rate the engine supports (spec floor).
pub const MIN_SAMPLE_RATE: u32 = 8_000;

/// Compares a valid document against the published engine limits.
///
/// Spec: piccle-spec/docs/11-engine-safety.md — resource limits MUST be
/// checked before allocating render resources and MUST NOT be presented as
/// schema or semantic-validation failures.
fn check_engine_limits(document: &Document, sample_rate: u32) -> PiccleResult<()> {
    if document.duration_ms > MAX_DURATION_MS {
        return Err(PiccleError::Unsupported {
            limit: "max_duration_ms",
            actual: document.duration_ms.to_string(),
            max: MAX_DURATION_MS.to_string(),
        });
    }
    if document.layers.len() > MAX_LAYERS {
        return Err(PiccleError::Unsupported {
            limit: "max_layers",
            actual: document.layers.len().to_string(),
            max: MAX_LAYERS.to_string(),
        });
    }
    if let Some(reverb) = &document.reverb {
        if reverb.tail_ms > MAX_TAIL_MS {
            return Err(PiccleError::Unsupported {
                limit: "max_tail_ms",
                actual: reverb.tail_ms.to_string(),
                max: MAX_TAIL_MS.to_string(),
            });
        }
        let tail_frames = piccle_core::schedule::frame_at(reverb.tail_ms, sample_rate);
        if reverb.amount > 0.0 && tail_frames > MAX_REVERB_TAIL_FRAMES {
            return Err(PiccleError::Unsupported {
                limit: "max_reverb_tail_frames",
                actual: tail_frames.to_string(),
                max: MAX_REVERB_TAIL_FRAMES.to_string(),
            });
        }
    }
    for layer in &document.layers {
        if layer.filters.len() > MAX_FILTERS_PER_LAYER {
            return Err(PiccleError::Unsupported {
                limit: "max_filters_per_layer",
                actual: layer.filters.len().to_string(),
                max: MAX_FILTERS_PER_LAYER.to_string(),
            });
        }
        check_contour_len(&layer.volume.levels)?;
        if let piccle_core::model::Source::Tone(tone) = &layer.source {
            check_contour_len(&tone.frequencies)?;
        }
        for filter in &layer.filters {
            check_contour_len(&filter.frequencies)?;
        }
    }
    Ok(())
}

fn check_contour_len(entries: &[piccle_core::model::ContourEntry]) -> PiccleResult<()> {
    if entries.len() > MAX_CONTOUR_ENTRIES {
        return Err(PiccleError::Unsupported {
            limit: "max_contour_entries",
            actual: entries.len().to_string(),
            max: MAX_CONTOUR_ENTRIES.to_string(),
        });
    }
    Ok(())
}
