//! Band-limited tone oscillator.
//!
//! Spec: piccle-spec/docs/03-sources.md §Tone generation. The engine
//! synthesizes the spec's own band-limited harmonic series into exact-count
//! wavetable banks before rendering. Harmonic-waveform rendering is therefore
//! constant work per sample even at the 20 Hz frequency floor.
//!
//! Sinusoids are evaluated through a shared 8192-entry sine table with
//! linear interpolation. At this table depth the fundamental attenuation of
//! interpolation is below 5e-8 (≈ −4e-7 dB) and table images sit below
//! −150 dBFS, far inside the ±1 dB / −60 dBFS conformance tolerances.

use std::sync::OnceLock;

use piccle_core::model::Waveform;

/// Sine table length (power of two for cheap wrapping).
const LUT_LEN: usize = 8_192;
/// Band-limited wavetable length. This exceeds twice the highest retained
/// square harmonic so every target partial has a unique table bin.
const BAND_LIMITED_LUT_LEN: usize = 4_096;
/// Symmetric quantization range for prebuilt harmonic tables. The Gibbs peak
/// of every retained Piccle series remains below this bound.
const BAND_LIMITED_LUT_PEAK: f64 = 1.5;
/// Highest retained saw harmonic (amplitude (2/π)/636 ≥ −60 dBFS).
const SAW_MAX_HARMONIC: usize = 636;
/// Highest retained odd square harmonic (amplitude (4/π)/1273 ≥ −60 dBFS).
const SQUARE_MAX_ODD_HARMONIC: usize = 1_273;
/// Integer −60 dBFS cap for triangle harmonics ((8/π²)/28² ≥ −60 dBFS); the
/// highest retained odd harmonic is therefore 27.
const TRIANGLE_MAX_HARMONIC: usize = 28;
/// Highest retained odd triangle harmonic.
const TRIANGLE_MAX_ODD_HARMONIC: usize = TRIANGLE_MAX_HARMONIC - 1;

const TWO_OVER_PI: f64 = 2.0 / std::f64::consts::PI;
const FOUR_OVER_PI: f64 = 4.0 / std::f64::consts::PI;
const EIGHT_OVER_PI_SQUARED: f64 = 8.0 / (std::f64::consts::PI * std::f64::consts::PI);

/// Saw coefficients for harmonics 1..=636: `(2/π) × (-1)^(k+1) / k`.
const SAW_COEFFICIENTS: [f64; SAW_MAX_HARMONIC] = {
    let mut table = [0.0; SAW_MAX_HARMONIC];
    let mut k = 1;
    while k <= SAW_MAX_HARMONIC {
        let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
        table[k - 1] = TWO_OVER_PI * sign / k as f64;
        k += 1;
    }
    table
};

/// Square coefficients for odd harmonics 1,3,..,1273: `(4/π) / k`.
const SQUARE_ODD_COEFFICIENTS: [f64; SQUARE_MAX_ODD_HARMONIC / 2 + 1] = {
    let mut table = [0.0; SQUARE_MAX_ODD_HARMONIC / 2 + 1];
    let mut i = 0;
    while i <= SQUARE_MAX_ODD_HARMONIC / 2 {
        let k = 2 * i + 1;
        table[i] = FOUR_OVER_PI / k as f64;
        i += 1;
    }
    table
};

/// Triangle coefficients for odd harmonics 1,3,..,27:
/// `(8/π²) × (-1)^((k-1)/2) / k²`.
const TRIANGLE_ODD_COEFFICIENTS: [f64; TRIANGLE_MAX_ODD_HARMONIC / 2 + 1] = {
    let mut table = [0.0; TRIANGLE_MAX_ODD_HARMONIC / 2 + 1];
    let mut i = 0;
    while i <= TRIANGLE_MAX_ODD_HARMONIC / 2 {
        let k = 2 * i + 1;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let kf = k as f64;
        table[i] = EIGHT_OVER_PI_SQUARED * sign / (kf * kf);
        i += 1;
    }
    table
};

/// Band-limited series coefficient for `harmonic` of `wave` (0 when the
/// harmonic is absent from the series, e.g. even square harmonics).
///
/// Spec: piccle-spec/docs/03-sources.md §Band-limited harmonic target.
#[must_use]
pub fn harmonic_coefficient(wave: Waveform, harmonic: usize) -> f64 {
    match wave {
        Waveform::Sine => {
            if harmonic == 1 {
                1.0
            }
            else {
                0.0
            }
        }
        Waveform::Saw => SAW_COEFFICIENTS.get(harmonic.wrapping_sub(1)).copied().unwrap_or(0.0),
        Waveform::Square => {
            if harmonic % 2 == 0 || harmonic == 0 {
                0.0
            }
            else {
                SQUARE_ODD_COEFFICIENTS.get(harmonic / 2).copied().unwrap_or(0.0)
            }
        }
        Waveform::Triangle => {
            if harmonic % 2 == 0 || harmonic == 0 {
                0.0
            }
            else {
                TRIANGLE_ODD_COEFFICIENTS.get(harmonic / 2).copied().unwrap_or(0.0)
            }
        }
    }
}

/// Shared sine table, initialized once at preparation time.
static SINE_LUT: OnceLock<[f64; LUT_LEN]> = OnceLock::new();
static SAW_TABLES: OnceLock<Vec<i16>> = OnceLock::new();
static SQUARE_TABLES: OnceLock<Vec<i16>> = OnceLock::new();
static TRIANGLE_TABLES: OnceLock<Vec<i16>> = OnceLock::new();

fn build_sine_lut() -> [f64; LUT_LEN] {
    let mut table = [0.0; LUT_LEN];
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = (2.0 * std::f64::consts::PI * i as f64 / LUT_LEN as f64).sin();
    }
    table
}

/// Initializes (once) and returns the shared sine table. Allocation-free
/// after the first call; intended to run during plan preparation, never in
/// the render loop.
pub fn init_sine_lut() -> &'static [f64; LUT_LEN] {
    SINE_LUT.get_or_init(build_sine_lut)
}

fn init_saw_tables() -> &'static [i16] {
    SAW_TABLES
        .get_or_init(|| build_table_bank(init_sine_lut(), &SAW_COEFFICIENTS, |index| index + 1))
}

fn init_square_tables() -> &'static [i16] {
    SQUARE_TABLES.get_or_init(|| {
        build_table_bank(init_sine_lut(), &SQUARE_ODD_COEFFICIENTS, |index| 2 * index + 1)
    })
}

fn init_triangle_tables() -> &'static [i16] {
    TRIANGLE_TABLES.get_or_init(|| {
        build_table_bank(init_sine_lut(), &TRIANGLE_ODD_COEFFICIENTS, |index| 2 * index + 1)
    })
}

/// Prepares the immutable lookup data required by `wave`.
///
/// Call during render-plan compilation so table construction and allocation
/// cannot occur in the render loop. Calling it repeatedly is constant-time.
pub fn prepare_waveform(wave: Waveform) {
    match wave {
        Waveform::Saw => {
            init_saw_tables();
        }
        Waveform::Square => {
            init_square_tables();
        }
        Waveform::Triangle => {
            init_triangle_tables();
        }
        Waveform::Sine => {
            init_sine_lut();
        }
    }
}

fn build_table_bank(
    sine: &[f64; LUT_LEN],
    coefficients: &[f64],
    harmonic_at: impl Fn(usize) -> usize,
) -> Vec<i16> {
    let mut current = vec![0.0_f64; BAND_LIMITED_LUT_LEN];
    let mut bank = Vec::with_capacity(coefficients.len() * BAND_LIMITED_LUT_LEN);
    let sine_stride = LUT_LEN / BAND_LIMITED_LUT_LEN;

    for (coefficient_index, &coefficient) in coefficients.iter().enumerate() {
        let harmonic = harmonic_at(coefficient_index);
        let compensated = coefficient * linear_interpolation_compensation(harmonic);
        for (sample_index, sample) in current.iter_mut().enumerate() {
            let sine_index = (sine_stride * harmonic * sample_index) & (LUT_LEN - 1);
            *sample += compensated * sine[sine_index];
        }
        bank.extend(current.iter().map(|&sample| {
            (sample.clamp(-BAND_LIMITED_LUT_PEAK, BAND_LIMITED_LUT_PEAK) * f64::from(i16::MAX)
                / BAND_LIMITED_LUT_PEAK)
                .round() as i16
        }));
    }

    bank
}

/// Inverts the first-order-hold attenuation at one table harmonic. Images
/// remain below the spec's −60 dBFS unwanted-component floor at this table
/// depth and are verified by the full oscillator DFT gate.
fn linear_interpolation_compensation(harmonic: usize) -> f64 {
    let x = std::f64::consts::PI * harmonic as f64 / BAND_LIMITED_LUT_LEN as f64;
    let sinc = x.sin() / x;
    1.0 / (sinc * sinc)
}

/// Mono band-limited oscillator.
///
/// Phase is kept in cycles (`[0, 1)`); the sample is emitted from the
/// current phase before advancing, per spec.
#[derive(Debug, Clone)]
pub struct Oscillator {
    wave: Waveform,
    sample_rate: u32,
    /// Oscillator phase in cycles.
    phase_cycles: f64,
    /// Current frequency in Hz (post pitch-transform and clamp).
    freq_hz: f64,
    /// Number of coefficient-table entries active at `freq_hz`.
    active_count: usize,
    /// Shared sine table reference (initialized at construction).
    lut: &'static [f64; LUT_LEN],
    /// Exact-count band-limited tables for harmonic waveforms.
    harmonic_bank: Option<&'static [i16]>,
}

impl Oscillator {
    /// Creates an oscillator with phase zero at the layer start.
    #[must_use]
    pub fn new(wave: Waveform, sample_rate: u32) -> Self {
        let harmonic_bank = match wave {
            Waveform::Saw => Some(init_saw_tables()),
            Waveform::Square => Some(init_square_tables()),
            Waveform::Triangle => Some(init_triangle_tables()),
            Waveform::Sine => None,
        };
        Self {
            wave,
            sample_rate,
            phase_cycles: 0.0,
            freq_hz: 0.0,
            active_count: 0,
            lut: init_sine_lut(),
            harmonic_bank,
        }
    }

    /// Resets phase to zero (layer restart).
    pub fn reset(&mut self) {
        self.phase_cycles = 0.0;
    }

    /// Sets the current frequency in Hz and re-derives the active harmonic
    /// count: harmonics `k` with `k × f < r / 2`, intersected with the
    /// per-waveform −60 dBFS retention caps.
    pub fn set_frequency(&mut self, freq_hz: f64) {
        self.freq_hz = freq_hz;
        // Largest k with k × f < r / 2 (strict), i.e. ceil(r / 2f) − 1.
        let nyquist_cap = (f64::from(self.sample_rate) / (2.0 * freq_hz)).ceil() as usize;
        let nyquist_cap = nyquist_cap.saturating_sub(1);
        self.active_count = match self.wave {
            Waveform::Sine => 1,
            Waveform::Saw => nyquist_cap.min(SAW_MAX_HARMONIC),
            Waveform::Square => {
                let highest_odd = nyquist_cap.min(SQUARE_MAX_ODD_HARMONIC);
                highest_odd.div_ceil(2)
            }
            Waveform::Triangle => {
                let highest_odd = nyquist_cap.min(TRIANGLE_MAX_ODD_HARMONIC);
                highest_odd.div_ceil(2)
            }
        };
    }

    /// Emits the current sample and advances the phase by `f / r` cycles.
    ///
    /// The harmonic set of the current frame is used for emission; the phase
    /// integral is preserved across frequency changes.
    #[inline]
    pub fn next_sample(&mut self) -> f64 {
        let lut = self.lut;
        let phase = self.phase_cycles;
        let mut sum = 0.0;
        match self.wave {
            Waveform::Sine => {
                sum = lut_lookup(lut, phase);
            }
            Waveform::Saw => {
                if let Some(bank) = self.harmonic_bank {
                    sum = band_limited_lookup(bank, self.active_count, phase);
                }
            }
            Waveform::Square => {
                if let Some(bank) = self.harmonic_bank {
                    sum = band_limited_lookup(bank, self.active_count, phase);
                }
            }
            Waveform::Triangle => {
                if let Some(bank) = self.harmonic_bank {
                    sum = band_limited_lookup(bank, self.active_count, phase);
                }
            }
        }
        self.phase_cycles += self.freq_hz / f64::from(self.sample_rate);
        if self.phase_cycles >= 1.0 {
            self.phase_cycles -= 1.0;
        }
        sum
    }
}

/// Linear-interpolated lookup in the table for exactly `active_count`
/// retained harmonics.
#[inline(always)]
fn band_limited_lookup(bank: &[i16], active_count: usize, phase_cycles: f64) -> f64 {
    if active_count == 0 {
        return 0.0;
    }
    let table_start = (active_count - 1) * BAND_LIMITED_LUT_LEN;
    let index = phase_cycles * BAND_LIMITED_LUT_LEN as f64;
    let i0 = index as usize;
    let frac = index - i0 as f64;
    let decode_scale = BAND_LIMITED_LUT_PEAK / f64::from(i16::MAX);
    let s0 = f64::from(bank[table_start + i0]) * decode_scale;
    let s1 = f64::from(bank[table_start + ((i0 + 1) & (BAND_LIMITED_LUT_LEN - 1))]) * decode_scale;
    s0 + frac * (s1 - s0)
}

/// Linear-interpolated sine lookup at `phase_cycles` in `[0, 1)`.
#[inline(always)]
fn lut_lookup(lut: &[f64; LUT_LEN], phase_cycles: f64) -> f64 {
    let index = phase_cycles * LUT_LEN as f64;
    let i0 = index as usize;
    let frac = index - i0 as f64;
    let s0 = lut[i0];
    s0 + frac * (lut[(i0 + 1) & (LUT_LEN - 1)] - s0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficient_tables_match_dsp_values() {
        assert_eq!(SAW_COEFFICIENTS[0], std::f64::consts::FRAC_2_PI);
    }

    #[test]
    fn square_coefficients_match_dsp_values() {
        assert_eq!(SQUARE_ODD_COEFFICIENTS[1], 0.4244131815783876);
    }

    #[test]
    fn triangle_coefficients_match_dsp_values() {
        assert_eq!(TRIANGLE_ODD_COEFFICIENTS[1], -0.09006327434874468);
    }

    #[test]
    fn triangle_retains_the_twenty_seventh_harmonic() {
        let k = 27.0;
        let expected = -8.0 / (std::f64::consts::PI.powi(2) * k * k);
        assert!((harmonic_coefficient(Waveform::Triangle, 27) - expected).abs() <= f64::EPSILON);
    }

    #[test]
    fn nyquist_cap_counts_match_spec_measurement_points() {
        let mut osc = Oscillator::new(Waveform::Sine, 48_000);
        osc.set_frequency(375.0);
        assert_eq!(osc.active_count, 1);
    }

    #[test]
    fn saw_harmonic_count_at_375hz() {
        let mut osc = Oscillator::new(Waveform::Saw, 48_000);
        osc.set_frequency(375.0);
        // ceil(48000 / 750) − 1 = 64 − 1 = 63 harmonics.
        assert_eq!(osc.active_count, 63);
    }

    #[test]
    fn first_sine_sample_is_zero_at_phase_zero() {
        let mut osc = Oscillator::new(Waveform::Sine, 48_000);
        osc.set_frequency(440.0);
        assert_eq!(osc.next_sample(), 0.0);
    }

    #[test]
    fn unconfigured_oscillator_emits_silence() {
        let mut osc = Oscillator::new(Waveform::Sine, 48_000);
        assert_eq!(osc.next_sample(), 0.0);
    }

    #[test]
    fn harmonic_table_banks_fit_within_eleven_mebibytes() {
        let bytes =
            (init_saw_tables().len() + init_square_tables().len() + init_triangle_tables().len())
                * std::mem::size_of::<i16>();
        assert!(bytes <= 11 * 1024 * 1024);
    }

    #[test]
    fn harmonic_table_quantization_does_not_clip() {
        assert!(
            init_saw_tables()
                .iter()
                .chain(init_square_tables())
                .chain(init_triangle_tables())
                .all(|&sample| sample != i16::MIN && sample != i16::MAX)
        );
    }

    #[test]
    fn harmonic_coefficient_is_one_only_for_the_sine_fundamental() {
        assert_eq!(
            (
                super::harmonic_coefficient(Waveform::Sine, 1),
                super::harmonic_coefficient(Waveform::Sine, 2)
            ),
            (1.0, 0.0)
        );
    }

    #[test]
    fn harmonic_coefficient_is_zero_for_even_square_and_triangle_harmonics() {
        assert_eq!(
            (
                super::harmonic_coefficient(Waveform::Square, 2),
                super::harmonic_coefficient(Waveform::Triangle, 4)
            ),
            (0.0, 0.0)
        );
    }

    #[test]
    fn harmonic_coefficient_is_zero_beyond_the_sixty_db_caps() {
        assert_eq!(
            (
                super::harmonic_coefficient(Waveform::Saw, 637),
                super::harmonic_coefficient(Waveform::Saw, 0)
            ),
            (0.0, 0.0)
        );
    }

    #[test]
    fn reset_returns_phase_to_zero() {
        let mut osc = Oscillator::new(Waveform::Sine, 48_000);
        osc.set_frequency(440.0);
        osc.next_sample();
        osc.reset();
        assert_eq!(osc.next_sample(), 0.0);
    }
}
