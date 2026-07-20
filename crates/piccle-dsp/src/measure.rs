//! Spectral measurement implementing the oscillator conformance procedure of
//! piccle-spec/docs/03-sources.md:
//!
//! ```text
//! C[k] = (2/N) × Σ(n = 0 .. N-1) x[n] × exp(-i × 2πkn/N)
//! amplitude[k] = |C[k]|
//! phase_from_sine[k] = wrap_to_pi(arg(C[k]) + π/2)
//! DC = abs((1/N) × Σ x[n])
//! ```
//!
//! Each bin is evaluated with the Goertzel recurrence (exactly the DFT sum,
//! not an FFT approximation), and bins are distributed across threads so the
//! full `N = 48000` sweep stays practical.

/// Complex DFT coefficient `C[k]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DftCoefficient {
    /// Real part of `C[k]`.
    pub real: f64,
    /// Imaginary part of `C[k]`.
    pub imaginary: f64,
}

impl DftCoefficient {
    /// `|C[k]|` — harmonic amplitude relative to full scale.
    #[must_use]
    pub fn amplitude(&self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    /// `wrap_to_pi(arg(C[k]) + π/2)` — phase relative to a sine reference.
    #[must_use]
    pub fn phase_from_sine(&self) -> f64 {
        wrap_to_pi(self.imaginary.atan2(self.real) + std::f64::consts::FRAC_PI_2)
    }
}

/// Wraps an angle to `(-π, π]`.
fn wrap_to_pi(angle: f64) -> f64 {
    angle - 2.0 * std::f64::consts::PI * (angle / (2.0 * std::f64::consts::PI)).round()
}

/// Goertzel evaluation of one DFT bin: `C[k] = (2/N) Σ x[n] e^{-i2πkn/N}`.
fn goertzel(samples: &[f64], bin: usize) -> DftCoefficient {
    let n = samples.len();
    let w = 2.0 * std::f64::consts::PI * bin as f64 / n as f64;
    let coeff = 2.0 * w.cos();
    let mut s_prev = 0.0_f64;
    let mut s_prev2 = 0.0_f64;
    for &x in samples {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    // One extra zero-input recurrence step, then X[k] = s[N] - s[N-1] × e^{-iw}
    // (spec's e^{-iwn} convention).
    let s_final = coeff * s_prev - s_prev2;
    let real = s_final - s_prev * w.cos();
    let imag = s_prev * w.sin();
    DftCoefficient { real: 2.0 / n as f64 * real, imaginary: 2.0 / n as f64 * imag }
}

/// Computes `C[k]` for bins `0 ..= N/2` (the spec's conformance sweep uses
/// bins `1 .. N/2 - 1`; the boundary bins are included for convenience).
/// The returned vector is indexed by bin number.
#[must_use]
pub fn dft(samples: &[f64]) -> Vec<DftCoefficient> {
    let n = samples.len();
    let mut bins = vec![DftCoefficient { real: 0.0, imaginary: 0.0 }; n / 2 + 1];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = bins.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let mut rest = bins.as_mut_slice();
        let mut start = 0_usize;
        while !rest.is_empty() {
            let take = chunk.min(rest.len());
            let (head, tail) = rest.split_at_mut(take);
            let first = start;
            scope.spawn(move || {
                for (offset, slot) in head.iter_mut().enumerate() {
                    *slot = goertzel(samples, first + offset);
                }
            });
            start += take;
            rest = tail;
        }
    });
    bins
}

/// `DC = |(1/N) × Σ x[n]|` — residual DC magnitude.
#[must_use]
pub fn dc_magnitude(samples: &[f64]) -> f64 {
    let sum = samples.iter().sum::<f64>();
    (sum / samples.len() as f64).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(bin: usize, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * bin as f64 * i as f64 / n as f64).sin())
            .collect()
    }

    #[test]
    fn dft_of_pure_sine_matches_published_reference() {
        // piccle-spec/test-vectors/numeric/dsp-values.json dft_sine_reference.
        let samples = sine_wave(3, 64);
        let coefficient = dft(&samples)[3];
        assert!((coefficient.real - 0.0).abs() < 1e-12);
    }

    #[test]
    fn dft_of_pure_sine_has_minus_unit_imaginary() {
        let samples = sine_wave(3, 64);
        let coefficient = dft(&samples)[3];
        assert!((coefficient.imaginary - -1.0).abs() < 1e-12);
    }

    #[test]
    fn dft_of_pure_sine_has_unit_amplitude() {
        let samples = sine_wave(5, 128);
        assert!((dft(&samples)[5].amplitude() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn dft_of_pure_sine_has_zero_phase_from_sine() {
        let samples = sine_wave(5, 128);
        assert!(dft(&samples)[5].phase_from_sine().abs() < 1e-9);
    }

    #[test]
    fn goertzel_matches_naive_dft_sum() {
        let n = 48;
        let samples: Vec<f64> = (0..n).map(|i| ((i * 7 + 1) as f64).sin()).collect();
        let bin = 7;
        let w = 2.0 * std::f64::consts::PI * bin as f64 / n as f64;
        let mut real = 0.0_f64;
        let mut imag = 0.0_f64;
        for (i, &x) in samples.iter().enumerate() {
            real += x * (w * i as f64).cos();
            imag -= x * (w * i as f64).sin();
        }
        let expected =
            DftCoefficient { real: 2.0 / n as f64 * real, imaginary: 2.0 / n as f64 * imag };
        let got = goertzel(&samples, bin);
        assert!((got.real - expected.real).abs() < 1e-12);
    }

    #[test]
    fn dc_magnitude_of_constant_signal() {
        let samples = vec![0.25_f64; 100];
        assert_eq!(dc_magnitude(&samples), 0.25);
    }
}
