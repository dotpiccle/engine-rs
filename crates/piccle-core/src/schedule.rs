//! Frame schedule primitives.
//!
//! Spec: piccle-spec/docs/11-engine-safety.md (frame rule and canonical
//! conformance profile).

/// Canonical conformance sample rate (Hz).
pub const CANONICAL_SAMPLE_RATE: u32 = 48_000;

/// Maximum iterations in the echo repeat-count procedure (`2^20`).
pub const ECHO_REPEAT_ITERATION_CAP: u64 = 1_048_576;

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

/// Computes echo `N_total` using the bounded binary64 iterative procedure.
///
/// Returns `None` when the `2^20` iteration cap is reached before the repeat
/// amplitude falls below `0.001`; that document is semantically invalid.
#[must_use]
pub fn echo_repeat_count(feedback: f64) -> Option<u64> {
    if feedback == 0.0 {
        return Some(1);
    }

    let mut repeats = 1_u64;
    let mut amp = feedback;
    let mut iterations = 0_u64;
    while amp >= 0.001 {
        amp *= feedback;
        repeats += 1;
        iterations += 1;
        if iterations >= ECHO_REPEAT_ITERATION_CAP {
            return None;
        }
    }
    Some(repeats + 1)
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

    #[test]
    fn echo_repeat_count_zero_feedback_has_one_repeat() {
        assert_eq!(echo_repeat_count(0.0), Some(1));
    }

    #[test]
    fn echo_repeat_count_matches_canonical_fixture() {
        assert_eq!(echo_repeat_count(0.6), Some(15));
    }

    #[test]
    fn echo_repeat_count_rejects_unbounded_feedback() {
        assert_eq!(echo_repeat_count(0.999_999_999_999_999_9), None);
    }
}
