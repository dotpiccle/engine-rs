//! Deterministic PCG32 noise source with character filters.
//!
//! Spec: piccle-spec/docs/09-noise-and-determinism.md. All arithmetic and
//! initialization steps are normative; do not substitute another generator.

use piccle_core::model::NoiseCharacter;
use piccle_core::schedule::render_frequency_max;

/// PCG-XSH-RR 64/32 fixed increment (the normative Piccle stream).
const PCG32_INCREMENT: u64 = 1_442_695_040_888_963_407;
/// PCG-XSH-RR 64/32 multiplier.
const PCG32_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
/// 2^32 as binary64 for the uniform conversion.
const TWO_POW_32: f64 = 4_294_967_296.0;

/// `soft` character lowpass corner (Hz) before render-profile clamping.
const SOFT_CORNER_HZ: f64 = 400.0;
/// `sharp` character highpass corner (Hz) before render-profile clamping.
const SHARP_CORNER_HZ: f64 = 2_000.0;

/// Stationary expected RMS after character shaping.
const TARGET_RMS: f64 = 0.25;

/// PCG-XSH-RR 64/32 generator with the normative Piccle init sequence.
///
/// Spec: piccle-spec/docs/09-noise-and-determinism.md §PCG32 stream.
#[derive(Debug, Clone)]
pub struct Pcg32 {
    state: u64,
}

impl Pcg32 {
    /// Initializes with the normative sequence: state = 0, one discarded
    /// `next`, state += seed, one more discarded `next`.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let mut rng = Self { state: 0 };
        let _ = rng.next_u32();
        rng.state = rng.state.wrapping_add(u64::from(seed));
        let _ = rng.next_u32();
        rng
    }

    /// Advances the generator and returns one unsigned 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        let old_state = self.state;
        self.state = old_state.wrapping_mul(PCG32_MULTIPLIER).wrapping_add(PCG32_INCREMENT);
        let xorshifted = (((old_state >> 18) ^ old_state) >> 27) as u32;
        let rotation = (old_state >> 59) as u32;
        xorshifted.rotate_right(rotation)
    }

    /// One raw uniform sample in `[-1, 1)`: `x = 2 × (u / 2^32) − 1`.
    pub fn next_sample_f64(&mut self) -> f64 {
        2.0 * (f64::from(self.next_u32()) / TWO_POW_32) - 1.0
    }
}

/// Mono deterministic noise voice with character filtering and RMS gain.
///
/// State starts at zero at the layer's `start_ms`.
#[derive(Debug, Clone)]
pub struct NoiseVoice {
    rng: Pcg32,
    character: NoiseCharacter,
    /// Character filter coefficient (`soft`/`sharp`).
    a: f64,
    /// Constant RMS normalization gain: `0.25 / sqrt(variance)`.
    gain: f64,
    /// Character filter output state `y[n-1]`.
    y1: f64,
    /// Character filter input state `x[n-1]` (`sharp` only).
    x1: f64,
}

impl NoiseVoice {
    /// Builds a voice for `character`/`seed` at `sample_rate`.
    ///
    /// Character corners clamp to `render_frequency_max` per spec.
    #[must_use]
    pub fn new(character: NoiseCharacter, seed: u32, sample_rate: u32) -> Self {
        let frequency_max = render_frequency_max(sample_rate);
        let rate = f64::from(sample_rate);
        let (a, variance) = match character {
            NoiseCharacter::Neutral => (0.0, 1.0 / 3.0),
            NoiseCharacter::Soft => {
                let corner = SOFT_CORNER_HZ.min(frequency_max);
                let a = (-2.0 * std::f64::consts::PI * corner / rate).exp();
                (a, (1.0 / 3.0) * (1.0 - a) / (1.0 + a))
            }
            NoiseCharacter::Sharp => {
                let corner = SHARP_CORNER_HZ.min(frequency_max);
                let a = (-2.0 * std::f64::consts::PI * corner / rate).exp();
                (a, (1.0 / 3.0) * (2.0 * a * a) / (1.0 + a))
            }
        };
        Self {
            rng: Pcg32::new(seed),
            character,
            a,
            gain: TARGET_RMS / variance.sqrt(),
            y1: 0.0,
            x1: 0.0,
        }
    }

    /// Resets generator and filter state to the layer start.
    pub fn reset(&mut self, seed: u32) {
        self.rng = Pcg32::new(seed);
        self.y1 = 0.0;
        self.x1 = 0.0;
    }

    /// Emits the next shaped, RMS-normalized noise sample.
    pub fn next_sample(&mut self) -> f64 {
        let x = self.rng.next_sample_f64();
        let y = match self.character {
            NoiseCharacter::Neutral => x,
            NoiseCharacter::Soft => {
                let y = self.a * self.y1 + (1.0 - self.a) * x;
                self.y1 = y;
                y
            }
            NoiseCharacter::Sharp => {
                let y = self.a * (self.y1 + x - self.x1);
                self.y1 = y;
                self.x1 = x;
                y
            }
        };
        y * self.gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcg32_seed_0_matches_dsp_values() {
        let mut rng = Pcg32::new(0);
        assert_eq!(
            [rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32()],
            [3894649422, 2055130073, 2315086854, 2925816488, 3443325253]
        );
    }

    #[test]
    fn pcg32_seed_1_matches_dsp_values() {
        let mut rng = Pcg32::new(1);
        assert_eq!(
            [rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32()],
            [1412771199, 1791099446, 124312908, 1968572995, 1080415314]
        );
    }

    #[test]
    fn pcg32_seed_max_matches_dsp_values() {
        let mut rng = Pcg32::new(u32::MAX);
        assert_eq!(
            [rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32(), rng.next_u32()],
            [1690806306, 1175666736, 601713809, 1455133790, 2659000460]
        );
    }

    #[test]
    fn neutral_gain_targets_quarter_rms() {
        let voice = NoiseVoice::new(NoiseCharacter::Neutral, 0, 48_000);
        assert_eq!(voice.gain, 0.25 / (1.0_f64 / 3.0).sqrt());
    }

    #[test]
    fn reset_restarts_the_generator_and_clears_filter_state() {
        let mut voice = NoiseVoice::new(NoiseCharacter::Soft, 7, 48_000);
        let first = voice.next_sample();
        voice.next_sample();
        voice.reset(7);
        assert_eq!(voice.next_sample(), first);
    }
}
