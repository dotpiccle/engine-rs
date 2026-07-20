//! Direct-form I biquad filter.
//!
//! Spec: piccle-spec/docs/06-filters.md §Biquad definition. Coefficients are
//! recomputed for every frame from the clamped contour value; an unchanged
//! frequency (bit-identical) skips recomputation, which cannot alter output.

use piccle_core::model::FilterType;
use piccle_core::schedule::render_frequency_max;

use crate::denormal::flush_subnormal;

/// Q at `resonance = 0`.
const Q_BASE: f64 = 0.707;
/// Q slope per unit resonance.
const Q_RESONANCE_SLOPE: f64 = 11.293;
/// Lowest permitted cutoff frequency (Hz).
const MIN_CUTOFF_HZ: f64 = 20.0;
/// One serial biquad in a layer's filter chain. State is all zeroes at the
/// layer start.
#[derive(Debug, Clone)]
pub struct Biquad {
    filter_type: FilterType,
    sample_rate: u32,
    q: f64,
    frequency_max: f64,
    /// Bit pattern of the frequency the current coefficients were computed
    /// from; identical bits reproduce identical coefficients.
    coeff_freq_bits: u64,
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// Creates a biquad with zero state. `resonance` maps to
    /// `Q = 0.707 + resonance × 11.293` per spec.
    #[must_use]
    pub fn new(filter_type: FilterType, resonance: f64, sample_rate: u32) -> Self {
        Self {
            filter_type,
            sample_rate,
            q: Q_BASE + resonance * Q_RESONANCE_SLOPE,
            frequency_max: render_frequency_max(sample_rate),
            coeff_freq_bits: u64::MAX,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Resets delay state to zero (layer restart); coefficients persist.
    pub fn reset_state(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// Sets this frame's cutoff frequency (clamped to
    /// `[20, render_frequency_max]`) and recomputes coefficients when the
    /// clamped value changed.
    pub fn set_frequency(&mut self, freq_hz: f64) {
        let f = freq_hz.clamp(MIN_CUTOFF_HZ, self.frequency_max);
        if f.to_bits() == self.coeff_freq_bits {
            return;
        }
        self.coeff_freq_bits = f.to_bits();
        // piccle-spec/docs/06-filters.md: ω = 2π × f / sample_rate,
        // c = cos(ω), α = sin(ω) / (2Q), normalized by a0 = 1 + α.
        let omega = 2.0 * std::f64::consts::PI * f / f64::from(self.sample_rate);
        let c = omega.cos();
        let alpha = omega.sin() / (2.0 * self.q);
        let inv_a0 = 1.0 / (1.0 + alpha);
        let (b0, b1, b2) = match self.filter_type {
            FilterType::Lowpass => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0),
            FilterType::Highpass => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0),
            FilterType::Bandpass => (alpha, 0.0, -alpha),
        };
        self.b0 = b0 * inv_a0;
        self.b1 = b1 * inv_a0;
        self.b2 = b2 * inv_a0;
        self.a1 = -2.0 * c * inv_a0;
        self.a2 = (1.0 - alpha) * inv_a0;
    }

    /// Current normalized coefficients `(b0, b1, b2, a1, a2)` (conformance
    /// evidence for piccle-spec/test-vectors/numeric/dsp-values.json).
    #[must_use]
    pub fn coefficients(&self) -> [f64; 5] {
        [self.b0, self.b1, self.b2, self.a1, self.a2]
    }

    /// Processes one input sample through the direct-form I difference
    /// equation.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = flush_subnormal(
            self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2,
        );
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCENDENTAL_TOLERANCE: f64 = 1e-14;

    fn approximately_equal(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= TRANSCENDENTAL_TOLERANCE
    }

    #[test]
    fn lowpass_1000hz_48k_res0_coefficients_match_dsp_values() {
        let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
        biquad.set_frequency(1_000.0);
        assert!(approximately_equal(biquad.b0, 0.0039160766836994635));
    }

    #[test]
    fn lowpass_a1_matches_dsp_values() {
        let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
        biquad.set_frequency(1_000.0);
        assert!(approximately_equal(biquad.a1, -1.8153179156742147));
    }

    #[test]
    fn lowpass_a2_matches_dsp_values() {
        let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
        biquad.set_frequency(1_000.0);
        assert!(approximately_equal(biquad.a2, 0.8309822224090126));
    }

    #[test]
    fn q_mapping_at_full_resonance() {
        let biquad = Biquad::new(FilterType::Bandpass, 1.0, 48_000);
        assert_eq!(biquad.q, 0.707 + 11.293);
    }

    #[test]
    fn reset_state_zeroes_the_delay_line_but_keeps_coefficients() {
        let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
        biquad.set_frequency(1_000.0);
        biquad.process(1.0);
        biquad.reset_state();
        assert!(
            biquad.x1 == 0.0
                && biquad.y1 == 0.0
                && approximately_equal(biquad.b0, 0.0039160766836994635)
        );
    }

    #[test]
    fn coefficients_returns_the_normalized_five_tuple() {
        let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
        biquad.set_frequency(1_000.0);
        let expected = [
            0.0039160766836994635,
            0.007832153367398927,
            0.0039160766836994635,
            -1.8153179156742147,
            0.8309822224090126,
        ];
        assert!(
            biquad
                .coefficients()
                .into_iter()
                .zip(expected)
                .all(|(actual, expected)| approximately_equal(actual, expected))
        );
    }
}
