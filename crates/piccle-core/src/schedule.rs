//! Frame schedule primitives.
//!
//! Spec: piccle-spec/docs/11-engine-safety.md (frame rule and canonical
//! conformance profile).

/// Canonical conformance sample rate (Hz).
pub const CANONICAL_SAMPLE_RATE: u32 = 48_000;

/// Absolute ceiling for source frequencies in any render profile (Hz).
pub const FREQUENCY_MAX_HZ: f64 = 20_000.0;

/// Absolute floor for pitch and filter frequencies in any render profile (Hz).
pub const FREQUENCY_MIN_HZ: f64 = 20.0;

/// Fraction of the sample rate available to sources in any profile.
pub const NYQUIST_SAFETY_FACTOR: f64 = 0.49;

/// Maps a millisecond timestamp to an absolute frame index.
///
/// Spec: piccle-spec/docs/11-engine-safety.md — the frame rule is evaluated
/// in binary64: `frame(m) = floor(m × r / 1000 + 0.5)`.
///
/// Callers must only pass timestamps within the engine's published limits;
/// validation enforces those limits before any frame conversion happens.
#[must_use]
pub fn frame_at(ms: u64, sample_rate: u32) -> u64 {
    (ms as f64 * f64::from(sample_rate) / 1000.0 + 0.5).floor() as u64
}

/// Highest frequency a source may reach in the active render profile.
///
/// Spec: piccle-spec/docs/11-engine-safety.md —
/// `render_frequency_max = min(20000, 0.49 × rate)`.
#[must_use]
pub fn render_frequency_max(sample_rate: u32) -> f64 {
    FREQUENCY_MAX_HZ.min(NYQUIST_SAFETY_FACTOR * f64::from(sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rule_rounds_half_up_at_millisecond_boundaries() {
        assert_eq!(frame_at(1, 48_000), 48);
    }

    #[test]
    fn frame_rule_preserves_the_zero_origin() {
        assert_eq!(frame_at(0, 48_000), 0);
    }

    #[test]
    fn render_frequency_max_matches_spec_table() {
        assert_eq!(render_frequency_max(8_000), 3_920.0);
    }

    #[test]
    fn render_frequency_max_caps_at_20khz() {
        assert_eq!(render_frequency_max(48_000), 20_000.0);
    }
}
