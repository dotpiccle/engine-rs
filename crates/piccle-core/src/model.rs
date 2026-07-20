//! Resolved Piccle document model with every default materialized.
//!
//! Values in this model have already passed schema and semantic validation;
//! constructing them outside `piccle-validate` is possible but bypasses the
//! security boundary defined by piccle-spec/docs/11-engine-safety.md.

use crate::curve::Curve;

/// Oscillator waveform for a tone source.
///
/// Spec: piccle-spec/schemas/v1.json `$defs/source` (`sine`, `triangle`,
/// `square`, `saw`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Waveform {
    /// Smooth sinusoid (single harmonic).
    Sine,
    /// Warm odd-harmonic waveform with 1/k^2 rolloff.
    Triangle,
    /// Hollow odd-harmonic waveform with 1/k rolloff.
    Square,
    /// Bright all-harmonic waveform with 1/k rolloff.
    Saw,
}

/// Spectral character of deterministic noise.
///
/// Spec: piccle-spec/docs/09-noise-and-determinism.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoiseCharacter {
    /// 400 Hz first-order lowpass character.
    Soft,
    /// Unfiltered uniform noise.
    Neutral,
    /// 2 kHz first-order highpass character.
    Sharp,
}

/// Biquad filter type.
///
/// Spec: piccle-spec/docs/06-filters.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterType {
    /// Keeps lows, attenuates above the cutoff.
    Lowpass,
    /// Keeps highs, attenuates below the cutoff.
    Highpass,
    /// Keeps a focused region around the cutoff.
    Bandpass,
}

/// One entry of a contour (frequency or level target with timing).
///
/// `transition_ms`/`transition_curve` describe the move toward the *next*
/// entry and are ignored on the last entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourEntry {
    /// Target value: Hz for pitch/filter contours, linear amplitude for levels.
    pub target: f64,
    /// Hold time at this entry before transitioning, in milliseconds.
    pub hold_ms: u64,
    /// Transition time toward the next entry, in milliseconds.
    pub transition_ms: u64,
    /// Curve shape of the transition toward the next entry.
    pub transition_curve: Curve,
}

/// Tone (pitched) source.
#[derive(Debug, Clone, PartialEq)]
pub struct ToneSource {
    /// Waveform shape.
    pub wave: Waveform,
    /// Pitch contour in Hz, evaluated from layer time zero.
    pub frequencies: Vec<ContourEntry>,
    /// Detune in cents, applied after contour interpolation and before the
    /// render-profile frequency clamp. Range -1200..=1200.
    pub offset_cents: i32,
}

/// Noise (pitchless) source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSource {
    /// Spectral character.
    pub character: NoiseCharacter,
    /// PCG32 seed (unsigned 32-bit).
    pub seed: u32,
}

/// Raw sound generator of a layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Pitched tone.
    Tone(ToneSource),
    /// Deterministic noise.
    Noise(NoiseSource),
}

/// One serial biquad filter in a layer's filter chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// Filter type, fixed for the layer duration.
    pub filter_type: FilterType,
    /// Cutoff-frequency contour in Hz, evaluated from layer time zero.
    pub frequencies: Vec<ContourEntry>,
    /// Resonance 0..=1 (Q = 0.707 + resonance * 11.293).
    pub resonance: f64,
}

/// A fade stage: duration plus the curve used to interpolate the fade gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeStage {
    /// Fade duration in milliseconds. 0 means no fade stage.
    pub ms: u64,
    /// Curve applied to the fade gain.
    pub curve: Curve,
}

/// Resolved loudness contour for a layer.
///
/// The number shorthand resolves to a constant level with the default
/// 5 ms linear fade-out (piccle-spec/docs/05-layer-volume.md).
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeContour {
    /// Fade-in from silence to the first declared level.
    pub fade_in: FadeStage,
    /// Fade-out from the held level to silence at the layer end.
    pub fade_out: FadeStage,
    /// Level contour; offsets begin after `fade_in.ms`.
    pub levels: Vec<ContourEntry>,
}

impl VolumeContour {
    /// Constant-level shorthand with the spec's default fade stages: no
    /// fade-in, 5 ms linear fade-out (piccle-spec/docs/05-layer-volume.md).
    #[must_use]
    pub fn constant(level: f64) -> Self {
        Self {
            fade_in: FadeStage { ms: 0, curve: Curve::Linear },
            fade_out: FadeStage { ms: 5, curve: Curve::Linear },
            levels: vec![ContourEntry {
                target: level,
                hold_ms: 0,
                transition_ms: 0,
                transition_curve: Curve::Linear,
            }],
        }
    }
}

/// One sound generator with envelope, stereo position, and filter chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Unique identifier (`^[a-z][a-z0-9-]*$`).
    pub id: String,
    /// Start time in milliseconds from the document start.
    pub start_ms: u64,
    /// Play duration in milliseconds.
    pub duration_ms: u64,
    /// Tone or noise generator.
    pub source: Source,
    /// Loudness contour.
    pub volume: VolumeContour,
    /// Stereo position -1 (left) ..= 1 (right).
    pub balance: f64,
    /// Serial filter chain (may be empty).
    pub filters: Vec<Filter>,
}

/// Whole-document reverb applied after the layer mix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reverb {
    /// Wet mix amount 0..=1.
    pub amount: f64,
    /// RT60 target and emitted wet-tail duration in milliseconds.
    pub tail_ms: u64,
    /// Wet-path lowpass corner in Hz (clamped to render bandwidth).
    pub soften_hz: f64,
}

/// Fully resolved Piccle document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Total document duration in milliseconds. When absent in the source
    /// document, computed as the latest layer end (`start_ms + duration_ms`).
    pub duration_ms: u64,
    /// Final master gain 0..=1 (default 1).
    pub master_volume_level: f64,
    /// Optional whole-document reverb.
    pub reverb: Option<Reverb>,
    /// Layers in canonical array order (mix order).
    pub layers: Vec<Layer>,
}
