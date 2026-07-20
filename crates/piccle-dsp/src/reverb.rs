//! Deterministic diffused eight-line FDN reverb.
//!
//! Spec: piccle-spec/docs/07-reverb.md (wet harness) and
//! piccle-spec/docs/13-implementer-notes.md §Reference reverb runtime. The
//! arithmetic ordering below mirrors the reference generator
//! (piccle-spec/scripts/generate_reverb_reference_irs.py) step for step. The
//! specification requires perceptual-equivalence tolerances rather than
//! cross-platform bit identity because preparation uses platform
//! transcendental functions.

use piccle_core::schedule::{frame_at, render_frequency_max};

use crate::denormal::flush_subnormal_if;
use crate::noise::Pcg32;

/// Number of FDN delay lines.
const NUM_LINES: usize = 8;
/// Recursive reverb state is explicitly flushed at this bounded cadence.
const DENORMAL_FLUSH_INTERVAL_FRAMES: u8 = 32;
/// Number of allpass diffuser sections per channel.
const NUM_DIFFUSERS: usize = 4;

/// Left-channel diffuser length caps (ms).
const DIFFUSER_CAP_LEFT_MS: [f64; NUM_DIFFUSERS] = [0.17, 0.31, 0.53, 0.89];
/// Right-channel diffuser length caps (ms).
const DIFFUSER_CAP_RIGHT_MS: [f64; NUM_DIFFUSERS] = [0.23, 0.41, 0.67, 1.07];
/// Diffuser length ratios against the tail length `R`.
const DIFFUSER_RATIOS: [f64; NUM_DIFFUSERS] = [0.003, 0.006, 0.012, 0.024];
/// Left diffuser allpass gain.
const DIFFUSER_GAIN_LEFT: f64 = 0.7;
/// Right diffuser allpass gain.
const DIFFUSER_GAIN_RIGHT: f64 = -0.7;

/// FDN delay length ratios against the tail length `R`.
const FDN_RATIOS: [f64; NUM_LINES] = [0.004, 0.006, 0.009, 0.013, 0.019, 0.027, 0.038, 0.053];

/// 32-bit golden-ratio multiplier in the normative configuration seed.
const CONFIGURATION_SEED_MULTIPLIER: u32 = 0x9E37_79B9;
/// Gram-Schmidt degeneracy threshold from the reference generator.
const ORTHOGONAL_NORM_FLOOR: f64 = 1e-15;

/// Input-mixing sign vector for the mid channel.
const MID_SIGN: [f64; NUM_LINES] = [1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0];
/// Input-mixing sign vector for the side channel.
const SIDE_SIGN: [f64; NUM_LINES] = [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];
/// Output-tap sign vector for the left core channel.
const LEFT_SIGN: [f64; NUM_LINES] = [1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, -1.0];
/// Output-tap sign vector for the right core channel.
const RIGHT_SIGN: [f64; NUM_LINES] = [1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0];

/// Direct (early) gain clamp bounds.
const DIRECT_GAIN_MIN: f64 = 0.7;
const DIRECT_GAIN_MAX: f64 = 1.5;
/// Direct gain reference tail (ms): `0.7 × sqrt(220 / tail_ms)`.
const DIRECT_GAIN_REFERENCE_TAIL_MS: f64 = 220.0;

/// Bisection calibration range for the decay exponent `p`.
const CALIBRATION_P_LO: f64 = 0.5;
const CALIBRATION_P_HI: f64 = 6.0;
/// Number of bisection steps (normative).
const CALIBRATION_STEPS: usize = 16;
/// RT60 crossing threshold as a linear energy ratio (−60 dB).
const RT60_ENERGY_RATIO: f64 = 1e-6;

/// Millisecond → frame rule evaluated on a fractional millisecond value
/// (diffuser caps are fractional): `floor(ms × r / 1000 + 0.5)`.
fn frame_at_f64(ms: f64, sample_rate: u32) -> u64 {
    (ms * f64::from(sample_rate) / 1000.0 + 0.5).floor() as u64
}

/// Reference diffuser length construction:
/// `raw = max(1, min(frame(cap), floor(R × ratio + 0.5)))`,
/// `d[0] = raw`, `d[i] = min(R, max(raw, d[i-1] + 1))`.
fn capped_delay_lengths<const N: usize>(
    tail_frames: u64,
    sample_rate: u32,
    caps_ms: &[f64; N],
    ratios: &[f64; N],
) -> [usize; N] {
    let mut lengths = [0usize; N];
    for i in 0..N {
        let cap_frames = frame_at_f64(caps_ms[i], sample_rate);
        let scaled = (tail_frames as f64 * ratios[i] + 0.5).floor() as u64;
        let raw = cap_frames.min(scaled).max(1);
        let length = if i == 0 { raw } else { raw.max(lengths[i - 1] as u64 + 1).min(tail_frames) };
        lengths[i] = length as usize;
    }
    lengths
}

/// Reference uncapped FDN construction: proportional to `R`, positive,
/// distinct, and bounded by `R`.
fn proportional_delay_lengths<const N: usize>(tail_frames: u64, ratios: &[f64; N]) -> [usize; N] {
    let mut lengths = [0usize; N];
    for i in 0..N {
        let raw = (tail_frames as f64 * ratios[i] + 0.5).floor() as u64;
        let raw = raw.max(1);
        let length = if i == 0 { raw } else { raw.max(lengths[i - 1] as u64 + 1).min(tail_frames) };
        lengths[i] = length as usize;
    }
    lengths
}

/// Builds the language-neutral normative reverb configuration seed.
fn configuration_seed(tail_ms: u64, soften_hz: f64) -> u32 {
    // piccle-spec/docs/13-implementer-notes.md §Reference reverb runtime.
    let soften_millihz = (soften_hz * 1_000.0 + 0.5).floor() as u32;
    (tail_ms as u32).wrapping_mul(CONFIGURATION_SEED_MULTIPLIER).wrapping_add(soften_millihz)
}

fn random_source_matrix(seed: u32) -> [[f64; NUM_LINES]; NUM_LINES] {
    let mut rng = Pcg32::new(seed);
    std::array::from_fn(|_| std::array::from_fn(|_| rng.next_sample_f64()))
}

fn vector_norm(vector: &[f64; NUM_LINES]) -> f64 {
    let mut squared_norm = 0.0;
    for value in vector {
        squared_norm += value * value;
    }
    squared_norm.sqrt()
}

/// Builds the reference matrix column-by-column using modified Gram-Schmidt.
fn random_orthogonal_matrix(seed: u32) -> [[f64; NUM_LINES]; NUM_LINES] {
    let source = random_source_matrix(seed);
    let mut matrix = [[0.0; NUM_LINES]; NUM_LINES];

    for column in 0..NUM_LINES {
        let mut vector: [f64; NUM_LINES] = std::array::from_fn(|row| source[row][column]);
        for prior_column in 0..column {
            let mut dot = 0.0;
            for (matrix_row, value) in matrix.iter().zip(&vector) {
                dot += matrix_row[prior_column] * value;
            }
            for (value, matrix_row) in vector.iter_mut().zip(&matrix) {
                *value -= dot * matrix_row[prior_column];
            }
        }

        let mut norm = vector_norm(&vector);
        if norm < ORTHOGONAL_NORM_FLOOR {
            vector[column] = 1.0;
            norm = vector_norm(&vector);
        }
        for (matrix_row, value) in matrix.iter_mut().zip(vector) {
            matrix_row[column] = value / norm;
        }
    }

    matrix
}

/// Terminal-window width for a tail of `tail_frames` frames:
/// `W = max(2, min(five_ms_frames, ceil(N / 10)))`.
#[must_use]
pub fn terminal_window_frames(tail_frames: u64, sample_rate: u32) -> u64 {
    let five_ms = frame_at(5, sample_rate);
    five_ms.min((tail_frames as f64 / 10.0).ceil() as u64).max(2)
}

/// Single Schroeder all-pass section with a circular delay.
#[derive(Debug, Clone)]
struct Allpass {
    buffer: Vec<f64>,
    index: usize,
    gain: f64,
}

impl Allpass {
    fn new(length: usize, gain: f64) -> Self {
        Self { buffer: vec![0.0; length], index: 0, gain }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }

    #[inline]
    fn process<const FLUSH: bool>(&mut self, x: f64) -> f64 {
        let delayed = self.buffer[self.index];
        let y = flush_subnormal_if::<FLUSH>(delayed - self.gain * x);
        self.buffer[self.index] = flush_subnormal_if::<FLUSH>(x + self.gain * y);
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0;
        }
        y
    }
}

/// The reverb core: diffuser chains plus the eight-line FDN. Shared between
/// the conformance harness and the production streaming processor.
#[derive(Debug, Clone)]
struct ReverbCore {
    sample_rate: u32,
    tail_frames: u64,
    diffusers_left: Vec<Allpass>,
    diffusers_right: Vec<Allpass>,
    fdn_lengths: [usize; NUM_LINES],
    fdn_buffers: [Vec<f64>; NUM_LINES],
    fdn_indices: [usize; NUM_LINES],
    feedback_gains: [f64; NUM_LINES],
    feedback_matrix: [[f64; NUM_LINES]; NUM_LINES],
    denormal_flush_phase: u8,
    direct_gain: f64,
    inv_sqrt2: f64,
    inv_sqrt8: f64,
}

impl ReverbCore {
    fn new(tail_ms: u64, soften_hz: f64, sample_rate: u32) -> Self {
        let feedback_matrix = random_orthogonal_matrix(configuration_seed(tail_ms, soften_hz));
        Self::with_feedback_matrix(tail_ms, sample_rate, feedback_matrix)
    }

    fn with_feedback_matrix(
        tail_ms: u64,
        sample_rate: u32,
        feedback_matrix: [[f64; NUM_LINES]; NUM_LINES],
    ) -> Self {
        let tail_frames = frame_at(tail_ms, sample_rate);
        let left_lengths =
            capped_delay_lengths(tail_frames, sample_rate, &DIFFUSER_CAP_LEFT_MS, &DIFFUSER_RATIOS);
        let right_lengths = capped_delay_lengths(
            tail_frames,
            sample_rate,
            &DIFFUSER_CAP_RIGHT_MS,
            &DIFFUSER_RATIOS,
        );
        let fdn_lengths = proportional_delay_lengths(tail_frames, &FDN_RATIOS);
        let direct_gain = DIRECT_GAIN_MAX.min(
            DIRECT_GAIN_MIN.max(0.7 * (DIRECT_GAIN_REFERENCE_TAIL_MS / tail_ms as f64).sqrt()),
        );
        Self {
            sample_rate,
            tail_frames,
            diffusers_left: left_lengths
                .iter()
                .map(|&len| Allpass::new(len, DIFFUSER_GAIN_LEFT))
                .collect(),
            diffusers_right: right_lengths
                .iter()
                .map(|&len| Allpass::new(len, DIFFUSER_GAIN_RIGHT))
                .collect(),
            fdn_lengths,
            fdn_buffers: std::array::from_fn(|i| vec![0.0; fdn_lengths[i]]),
            fdn_indices: [0; NUM_LINES],
            feedback_gains: [0.0; NUM_LINES],
            feedback_matrix,
            denormal_flush_phase: 0,
            direct_gain,
            inv_sqrt2: 1.0 / 2.0_f64.sqrt(),
            inv_sqrt8: 1.0 / 8.0_f64.sqrt(),
        }
    }

    fn diffuser_lengths_left(&self) -> Vec<usize> {
        self.diffusers_left.iter().map(|ap| ap.buffer.len()).collect()
    }

    fn diffuser_lengths_right(&self) -> Vec<usize> {
        self.diffusers_right.iter().map(|ap| ap.buffer.len()).collect()
    }

    fn reset_state(&mut self) {
        for diffuser in &mut self.diffusers_left {
            diffuser.reset();
        }
        for diffuser in &mut self.diffusers_right {
            diffuser.reset();
        }
        for i in 0..NUM_LINES {
            self.fdn_buffers[i].fill(0.0);
            self.fdn_indices[i] = 0;
        }
        self.denormal_flush_phase = 0;
    }

    /// `g[i] = 10^((-p × d[i]) / R)` per line.
    fn set_feedback_gains(&mut self, p: f64) {
        for i in 0..NUM_LINES {
            self.feedback_gains[i] =
                10.0_f64.powf(-p * self.fdn_lengths[i] as f64 / self.tail_frames as f64);
        }
    }

    /// One core frame: diffusers → FDN → stereo core taps (no wet lowpass).
    #[inline]
    fn process_frame(&mut self, in_left: f64, in_right: f64) -> (f64, f64, bool) {
        let flush = self.denormal_flush_phase == 0;
        self.denormal_flush_phase += 1;
        if self.denormal_flush_phase == DENORMAL_FLUSH_INTERVAL_FRAMES {
            self.denormal_flush_phase = 0;
        }
        let (left, right) = if flush {
            self.process_frame_inner::<true>(in_left, in_right)
        }
        else {
            self.process_frame_inner::<false>(in_left, in_right)
        };
        (left, right, flush)
    }

    #[inline]
    fn process_frame_inner<const FLUSH: bool>(
        &mut self,
        in_left: f64,
        in_right: f64,
    ) -> (f64, f64) {
        let mut diff_left = in_left;
        for diffuser in &mut self.diffusers_left {
            diff_left = diffuser.process::<FLUSH>(diff_left);
        }
        let mut diff_right = in_right;
        for diffuser in &mut self.diffusers_right {
            diff_right = diffuser.process::<FLUSH>(diff_right);
        }

        let mut z = [0.0; NUM_LINES];
        for (z_slot, (buffer, index)) in
            z.iter_mut().zip(self.fdn_buffers.iter().zip(&self.fdn_indices))
        {
            *z_slot = buffer[*index];
        }

        let mid = (diff_left + diff_right) * self.inv_sqrt2;
        let side = (diff_left - diff_right) * self.inv_sqrt2;

        let mut u = [0.0; NUM_LINES];
        for i in 0..NUM_LINES {
            u[i] = (MID_SIGN[i] * mid + SIDE_SIGN[i] * side) * self.inv_sqrt8;
        }

        let mut feedback_input = [0.0; NUM_LINES];
        for i in 0..NUM_LINES {
            feedback_input[i] = self.feedback_gains[i] * z[i];
        }

        let mut feedback_output = [0.0; NUM_LINES];
        for (row, output) in feedback_output.iter_mut().enumerate() {
            for (column, input) in feedback_input.iter().enumerate() {
                *output += self.feedback_matrix[row][column] * input;
            }
        }

        for i in 0..NUM_LINES {
            self.fdn_buffers[i][self.fdn_indices[i]] =
                flush_subnormal_if::<FLUSH>(u[i] + feedback_output[i]);
            self.fdn_indices[i] += 1;
            if self.fdn_indices[i] == self.fdn_lengths[i] {
                self.fdn_indices[i] = 0;
            }
        }

        let mut core_left = self.direct_gain * diff_left;
        let mut core_right = self.direct_gain * diff_right;
        for i in 0..NUM_LINES {
            core_left += LEFT_SIGN[i] * z[i] * self.inv_sqrt8;
            core_right += RIGHT_SIGN[i] * z[i] * self.inv_sqrt8;
        }
        (core_left, core_right)
    }
}

#[derive(Clone, Copy)]
struct HarnessProfile {
    lowpass_a: f64,
    sqrt_half: f64,
}

impl HarnessProfile {
    /// Runs the impulse, lowpass, and terminal window once in exact frame
    /// order. The callback receives unnormalized wet samples.
    fn run(self, core: &mut ReverbCore, mut consume: impl FnMut(f64, f64)) {
        core.reset_state();
        let t = core.tail_frames as usize + 1;
        let window = terminal_window_frames(core.tail_frames, core.sample_rate) as usize;
        let divisor = (window - 1).max(1) as f64;
        let mut y_left = 0.0;
        let mut y_right = 0.0;

        for frame in 0..t {
            let input = if frame == 0 { self.sqrt_half } else { 0.0 };
            let (core_left, core_right, _) = core.process_frame(input, input);
            let mut wet_left = self.lowpass_a * y_left + (1.0 - self.lowpass_a) * core_left;
            let mut wet_right = self.lowpass_a * y_right + (1.0 - self.lowpass_a) * core_right;
            y_left = wet_left;
            y_right = wet_right;
            if frame >= t - window {
                let gain = (t - 1 - frame) as f64 / divisor;
                wet_left *= gain;
                wet_right *= gain;
            }
            consume(wet_left, wet_right);
        }
    }
}

struct CalibrationHarness {
    profile: HarnessProfile,
    frame_energy: Vec<f64>,
}

impl CalibrationHarness {
    fn new(core: &ReverbCore, profile: HarnessProfile) -> Self {
        Self { profile, frame_energy: vec![0.0; core.tail_frames as usize + 1] }
    }

    fn measure(&mut self, core: &mut ReverbCore, p: f64) -> (usize, f64) {
        core.set_feedback_gains(p);
        let mut total_energy = 0.0;
        let mut index = 0;
        self.profile.run(core, |left, right| {
            let energy = left * left + right * right;
            self.frame_energy[index] = energy;
            total_energy += energy;
            index += 1;
        });
        let crossing = rt60_crossing_from_energy(&self.frame_energy);
        let norm_gain = if total_energy > 0.0 { 1.0 / total_energy.sqrt() } else { 1.0 };
        (crossing, norm_gain)
    }

    fn calibrate(&mut self, core: &mut ReverbCore) -> (f64, f64) {
        let target = 1 + (0.95 * core.tail_frames as f64).floor() as usize;
        let mut lo = CALIBRATION_P_LO;
        let mut hi = CALIBRATION_P_HI;
        for _ in 0..CALIBRATION_STEPS {
            let p = (lo + hi) / 2.0;
            let (crossing, _) = self.measure(core, p);
            if crossing < target {
                hi = p;
            }
            else {
                lo = p;
            }
        }
        let calibrated_p = (lo + hi) / 2.0;
        let (_, norm_gain) = self.measure(core, calibrated_p);
        (calibrated_p, norm_gain)
    }

    fn capture(&self, core: &mut ReverbCore, p: f64) -> (Vec<f64>, Vec<f64>) {
        core.set_feedback_gains(p);
        let frames = core.tail_frames as usize + 1;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        self.profile.run(core, |left_sample, right_sample| {
            left.push(left_sample);
            right.push(right_sample);
        });
        let energy = left.iter().zip(&right).map(|(&l, &r)| l * l + r * r).sum::<f64>();
        if energy > 0.0 {
            let norm_gain = 1.0 / energy.sqrt();
            for sample in left.iter_mut().chain(&mut right) {
                *sample *= norm_gain;
            }
        }
        (left, right)
    }
}

/// First index whose backward-integrated energy is at most `1e-6 × E[0]`;
/// `T - 1` when never crossed (normative EDC rule).
fn rt60_crossing_from_energy(frame_energy: &[f64]) -> usize {
    let e0 = frame_energy.iter().rev().sum::<f64>();
    let threshold = RT60_ENERGY_RATIO * e0;
    let mut suffix = 0.0;
    let mut crossing = frame_energy.len().saturating_sub(1);
    for (index, &energy) in frame_energy.iter().enumerate().rev() {
        suffix += energy;
        if suffix > threshold {
            break;
        }
        crossing = index;
    }
    crossing
}

/// Calibrated wet-path configuration for one reverb declaration. Built once
/// per document at preparation time.
#[derive(Debug, Clone)]
pub struct ReverbConfig {
    sample_rate: u32,
    tail_ms: u64,
    tail_frames: u64,
    diffuser_lengths_left: Vec<usize>,
    diffuser_lengths_right: Vec<usize>,
    fdn_lengths: [usize; NUM_LINES],
    feedback_gains: [f64; NUM_LINES],
    feedback_matrix: [[f64; NUM_LINES]; NUM_LINES],
    direct_gain: f64,
    /// Wet lowpass coefficient `a = exp(-2π × f / r)`.
    lowpass_a: f64,
    /// Constant normalization gain from the conformance harness.
    norm_gain: f64,
    /// Calibrated decay exponent (bisection result).
    calibrated_p: f64,
}

impl ReverbConfig {
    /// Builds and calibrates a reverb configuration (16 bisection runs plus
    /// one final normalization measurement of the conformance harness).
    #[must_use]
    pub fn new(tail_ms: u64, soften_hz: f64, sample_rate: u32) -> Self {
        let mut core = ReverbCore::new(tail_ms, soften_hz, sample_rate);
        let frequency_max = render_frequency_max(sample_rate);
        let soften = soften_hz.min(frequency_max);
        let lowpass_a = (-2.0 * std::f64::consts::PI * soften / f64::from(sample_rate)).exp();
        let profile = HarnessProfile { lowpass_a, sqrt_half: 0.5_f64.sqrt() };
        let (calibrated_p, norm_gain) =
            CalibrationHarness::new(&core, profile).calibrate(&mut core);

        Self {
            sample_rate,
            tail_ms,
            tail_frames: core.tail_frames,
            diffuser_lengths_left: core.diffuser_lengths_left(),
            diffuser_lengths_right: core.diffuser_lengths_right(),
            fdn_lengths: core.fdn_lengths,
            feedback_gains: core.feedback_gains,
            feedback_matrix: core.feedback_matrix,
            direct_gain: core.direct_gain,
            lowpass_a,
            norm_gain,
            calibrated_p,
        }
    }

    /// Calibrated decay exponent `p` (conformance evidence).
    #[must_use]
    pub fn calibrated_p(&self) -> f64 {
        self.calibrated_p
    }

    /// Constant wet normalization gain from the conformance harness.
    #[must_use]
    pub fn norm_gain(&self) -> f64 {
        self.norm_gain
    }

    /// Tail length in frames (`R = frame(tail_ms)`).
    #[must_use]
    pub fn tail_frames(&self) -> u64 {
        self.tail_frames
    }

    /// Declared tail in milliseconds.
    #[must_use]
    pub fn tail_ms(&self) -> u64 {
        self.tail_ms
    }

    /// FDN delay-line lengths in frames (conformance evidence).
    #[must_use]
    pub fn fdn_lengths(&self) -> [usize; NUM_LINES] {
        self.fdn_lengths
    }

    /// Left diffuser lengths in frames (conformance evidence).
    #[must_use]
    pub fn diffuser_lengths_left(&self) -> &[usize] {
        &self.diffuser_lengths_left
    }

    /// Right diffuser lengths in frames (conformance evidence).
    #[must_use]
    pub fn diffuser_lengths_right(&self) -> &[usize] {
        &self.diffuser_lengths_right
    }

    /// Direct (early) gain (conformance evidence).
    #[must_use]
    pub fn direct_gain(&self) -> f64 {
        self.direct_gain
    }
}

/// Production streaming reverb processor. All state starts at zero for each
/// document and is discarded after the final output frame.
#[derive(Debug, Clone)]
pub struct Reverb {
    core: ReverbCore,
    lowpass_a: f64,
    norm_gain: f64,
    y_left: f64,
    y_right: f64,
}

impl Reverb {
    /// Creates a zero-state streaming processor from a calibrated config.
    #[must_use]
    pub fn new(config: &ReverbConfig) -> Self {
        let mut core = ReverbCore::with_feedback_matrix(
            config.tail_ms,
            config.sample_rate,
            config.feedback_matrix,
        );
        core.feedback_gains = config.feedback_gains;
        Self {
            core,
            lowpass_a: config.lowpass_a,
            norm_gain: config.norm_gain,
            y_left: 0.0,
            y_right: 0.0,
        }
    }

    /// Processes one dry stereo frame and returns the wet stereo frame after
    /// the reverb core, wet lowpass, terminal window gain, and normalization
    /// (everything except the dry/wet crossfade, which the renderer applies).
    ///
    /// `terminal_gain` is the renderer-computed automatic terminal window
    /// gain for this absolute frame (1 outside the window region).
    #[inline]
    pub fn process(&mut self, dry_left: f64, dry_right: f64, terminal_gain: f64) -> (f64, f64) {
        let (core_left, core_right, flush) = self.core.process_frame(dry_left, dry_right);
        if flush {
            self.process_lowpass::<true>(core_left, core_right, terminal_gain)
        }
        else {
            self.process_lowpass::<false>(core_left, core_right, terminal_gain)
        }
    }

    #[inline]
    fn process_lowpass<const FLUSH: bool>(
        &mut self,
        core_left: f64,
        core_right: f64,
        terminal_gain: f64,
    ) -> (f64, f64) {
        let wet_left = flush_subnormal_if::<FLUSH>(
            self.lowpass_a * self.y_left + (1.0 - self.lowpass_a) * core_left,
        );
        self.y_left = wet_left;
        let wet_right = flush_subnormal_if::<FLUSH>(
            self.lowpass_a * self.y_right + (1.0 - self.lowpass_a) * core_right,
        );
        self.y_right = wet_right;
        (wet_left * terminal_gain * self.norm_gain, wet_right * terminal_gain * self.norm_gain)
    }
}

/// Runs the full reference harness for one configuration and returns the
/// post-normalization stereo capture. Used by the reverb equivalence gate
/// against piccle-spec/test-vectors/numeric/reverb-reference-irs/.
#[must_use]
pub fn generate_reference_ir(
    tail_ms: u64,
    soften_hz: f64,
    sample_rate: u32,
) -> (Vec<f64>, Vec<f64>) {
    let mut core = ReverbCore::new(tail_ms, soften_hz, sample_rate);
    let frequency_max = render_frequency_max(sample_rate);
    let soften = soften_hz.min(frequency_max);
    let lowpass_a = (-2.0 * std::f64::consts::PI * soften / f64::from(sample_rate)).exp();
    let profile = HarnessProfile { lowpass_a, sqrt_half: 0.5_f64.sqrt() };
    let mut harness = CalibrationHarness::new(&core, profile);
    let (calibrated_p, _) = harness.calibrate(&mut core);
    harness.capture(&mut core, calibrated_p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATRIX_VECTOR: &str = include_str!("../test-data/reverb-matrix-vector.json");

    fn matrix_vector() -> serde_json::Value {
        serde_json::from_str(MATRIX_VECTOR).expect("published matrix vector must be valid JSON")
    }

    fn matrix_bits(vector: &serde_json::Value, key: &str) -> Vec<u64> {
        vector[key]
            .as_array()
            .expect("matrix must contain rows")
            .iter()
            .flat_map(|row| row.as_array().expect("matrix row must be an array"))
            .map(|value| {
                value.to_string().parse::<f64>().expect("matrix entry must be binary64").to_bits()
            })
            .collect()
    }

    #[test]
    fn configuration_seed_matches_the_published_matrix_vector() {
        let vector = matrix_vector();
        let tail_ms = vector["configuration"]["tail_ms"].as_u64().expect("tail_ms");
        let soften_hz = vector["configuration"]["soften_hz"].as_f64().expect("soften_hz");
        let expected = vector["seed"].as_u64().expect("seed") as u32;

        assert_eq!(configuration_seed(tail_ms, soften_hz), expected);
    }

    #[test]
    fn pcg32_stream_matches_the_published_matrix_vector() {
        let vector = matrix_vector();
        let seed = vector["seed"].as_u64().expect("seed") as u32;
        let expected = vector["pcg32_first_8_u32"]
            .as_array()
            .expect("PCG32 outputs")
            .iter()
            .map(|value| value.as_u64().expect("PCG32 output") as u32)
            .collect::<Vec<_>>();
        let mut rng = Pcg32::new(seed);
        let actual = std::array::from_fn::<_, NUM_LINES, _>(|_| rng.next_u32());

        assert_eq!(actual.as_slice(), expected);
    }

    #[test]
    fn source_matrix_matches_the_published_matrix_vector() {
        let vector = matrix_vector();
        let seed = vector["seed"].as_u64().expect("seed") as u32;
        let actual =
            random_source_matrix(seed).into_iter().flatten().map(f64::to_bits).collect::<Vec<_>>();

        assert_eq!(actual, matrix_bits(&vector, "source_matrix_a"));
    }

    #[test]
    fn feedback_matrix_matches_the_published_matrix_vector() {
        let vector = matrix_vector();
        let seed = vector["seed"].as_u64().expect("seed") as u32;
        let actual = random_orthogonal_matrix(seed)
            .into_iter()
            .flatten()
            .map(f64::to_bits)
            .collect::<Vec<_>>();

        assert_eq!(actual, matrix_bits(&vector, "feedback_matrix_q"));
    }

    #[test]
    fn allpass_flushes_subnormal_state_and_output() {
        let mut allpass = Allpass::new(1, DIFFUSER_GAIN_LEFT);
        let output = allpass.process::<true>(f64::from_bits(1));
        assert_eq!((output, allpass.buffer[0]), (0.0, 0.0));
    }

    #[test]
    fn reverb_denormal_maintenance_cadence_is_bounded() {
        let mut core = ReverbCore::new(1, 4_000.0, 48_000);
        for _ in 0..DENORMAL_FLUSH_INTERVAL_FRAMES {
            let _ = core.process_frame(0.0, 0.0);
        }
        assert_eq!(core.denormal_flush_phase, 0);
    }

    #[test]
    fn tail_1ms_lengths_match_dsp_values() {
        let core = ReverbCore::new(1, 4_000.0, 48_000);
        assert_eq!(core.diffuser_lengths_left(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn tail_1ms_fdn_lengths_match_dsp_values() {
        let core = ReverbCore::new(1, 4_000.0, 48_000);
        assert_eq!(core.fdn_lengths, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn tail_20ms_lengths_match_dsp_values() {
        let core = ReverbCore::new(20, 4_000.0, 48_000);
        assert_eq!(core.diffuser_lengths_left(), vec![3, 6, 12, 23]);
    }

    #[test]
    fn tail_20ms_fdn_lengths_match_dsp_values() {
        let core = ReverbCore::new(20, 4_000.0, 48_000);
        assert_eq!(core.fdn_lengths, [4, 6, 9, 12, 18, 26, 36, 51]);
    }

    #[test]
    fn tail_220ms_lengths_match_dsp_values() {
        let core = ReverbCore::new(220, 4_000.0, 48_000);
        assert_eq!(core.diffuser_lengths_left(), vec![8, 15, 25, 43]);
    }

    #[test]
    fn tail_220ms_right_diffusers_match_dsp_values() {
        let core = ReverbCore::new(220, 4_000.0, 48_000);
        assert_eq!(core.diffuser_lengths_right(), vec![11, 20, 32, 51]);
    }

    #[test]
    fn tail_220ms_fdn_lengths_match_dsp_values() {
        let core = ReverbCore::new(220, 4_000.0, 48_000);
        assert_eq!(core.fdn_lengths, [42, 63, 95, 137, 201, 285, 401, 560]);
    }

    #[test]
    fn tail_500ms_fdn_lengths_match_dsp_values() {
        let core = ReverbCore::new(500, 4_000.0, 48_000);
        assert_eq!(core.fdn_lengths, [96, 144, 216, 312, 456, 648, 912, 1_272]);
    }

    #[test]
    fn direct_gain_tail_1ms() {
        let core = ReverbCore::new(1, 4_000.0, 48_000);
        assert_eq!(core.direct_gain, 1.5);
    }

    #[test]
    fn direct_gain_tail_220ms() {
        let core = ReverbCore::new(220, 4_000.0, 48_000);
        assert_eq!(core.direct_gain, 0.7);
    }

    #[test]
    fn terminal_window_widths_match_dsp_values() {
        assert_eq!(terminal_window_frames(48, 48_000), 5);
    }

    #[test]
    fn terminal_window_width_500ms() {
        assert_eq!(terminal_window_frames(24_000, 48_000), 240);
    }

    #[test]
    fn calibrated_p_tail_1ms_matches_reference() {
        let config = ReverbConfig::new(1, 4_000.0, 48_000);
        assert_eq!(config.calibrated_p(), 5.999958038330078);
    }

    #[test]
    fn wet_lowpass_clamps_to_the_low_rate_profile_bandwidth() {
        let config = ReverbConfig::new(1, 12_000.0, 8_000);
        let expected = (-2.0 * std::f64::consts::PI * 3_920.0 / 8_000.0).exp();
        assert_eq!(config.lowpass_a, expected);
    }

    #[test]
    fn calibrated_p_tail_220ms_matches_reference() {
        let config = ReverbConfig::new(220, 4_000.0, 48_000);
        assert_eq!(config.calibrated_p(), 2.8738975524902344);
    }

    #[test]
    fn calibration_scratch_uses_one_energy_value_per_frame() {
        let core = ReverbCore::new(500, 4_000.0, 48_000);
        let profile = HarnessProfile { lowpass_a: 0.5, sqrt_half: 0.5_f64.sqrt() };
        let harness = CalibrationHarness::new(&core, profile);
        assert_eq!(harness.frame_energy.len(), 24_001);
    }

    #[test]
    #[ignore = "release-profile resource-ceiling gate"]
    fn maximum_tail_prepares_with_finite_calibration() {
        let config = ReverbConfig::new(60_000, 8_000.0, 48_000);
        assert_eq!(
            (
                config.tail_frames(),
                config.calibrated_p().is_finite(),
                config.norm_gain().is_finite()
            ),
            (2_880_000, true, true)
        );
    }

    #[test]
    fn config_getters_expose_the_compiled_evidence() {
        let config = ReverbConfig::new(220, 4_000.0, 48_000);
        assert_eq!(
            (
                config.tail_ms(),
                config.tail_frames(),
                config.direct_gain(),
                config.fdn_lengths()[0],
                config.diffuser_lengths_left()[0],
                config.diffuser_lengths_right()[0],
                config.norm_gain().is_finite(),
            ),
            (220, 10_560, 0.7, 42, 8, 11, true)
        );
    }
}
