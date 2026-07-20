//! Deterministic per-channel lowpass-feedback echo.
//!
//! Spec: piccle-spec/docs/07-spatial-effects.md §Echo effect and
//! docs/13-implementer-notes.md §Reference echo runtime.

use piccle_core::schedule::{frame_at, render_frequency_max};

use crate::denormal::flush_subnormal_if;

/// Recursive echo state is explicitly flushed at this bounded cadence.
const DENORMAL_FLUSH_INTERVAL_FRAMES: u8 = 32;

#[derive(Debug, Clone)]
struct DelayLine {
    buffer: Vec<f64>,
    index: usize,
    lowpass_state: f64,
}

impl DelayLine {
    fn new(delay_length: usize) -> Self {
        Self { buffer: vec![0.0; delay_length], index: 0, lowpass_state: 0.0 }
    }

    #[inline]
    fn process<const FLUSH: bool>(
        &mut self,
        input: f64,
        feedback: f64,
        lowpass_a: f64,
        terminal_gain: f64,
    ) -> f64 {
        let delayed = self.buffer[self.index];
        let filtered = flush_subnormal_if::<FLUSH>(
            lowpass_a * self.lowpass_state + (1.0 - lowpass_a) * delayed,
        );
        self.lowpass_state = filtered;
        self.buffer[self.index] = flush_subnormal_if::<FLUSH>(input + feedback * filtered);
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0;
        }
        filtered * terminal_gain
    }
}

/// Prepared echo configuration.
#[derive(Debug, Clone)]
pub struct EchoConfig {
    delay_length: usize,
    feedback: f64,
    lowpass_a: f64,
}

impl EchoConfig {
    /// Builds an echo configuration for a render profile.
    #[must_use]
    pub fn new(delay_ms: u64, feedback: f64, damp_hz: f64, sample_rate: u32) -> Self {
        let delay_length = frame_at(delay_ms, sample_rate).max(1) as usize;
        let damp = damp_hz.min(render_frequency_max(sample_rate));
        let lowpass_a = (-2.0 * std::f64::consts::PI * damp / f64::from(sample_rate)).exp();
        Self { delay_length, feedback, lowpass_a }
    }

    /// Delay-line length in frames.
    #[must_use]
    pub fn delay_length(&self) -> usize {
        self.delay_length
    }
}

/// Streaming echo processor with independent L/R delay lines and lowpass state.
#[derive(Debug, Clone)]
pub struct Echo {
    config: EchoConfig,
    left: DelayLine,
    right: DelayLine,
    denormal_flush_phase: u8,
}

impl Echo {
    /// Creates a zero-state echo processor from a prepared config.
    #[must_use]
    pub fn new(config: &EchoConfig) -> Self {
        Self {
            config: config.clone(),
            left: DelayLine::new(config.delay_length),
            right: DelayLine::new(config.delay_length),
            denormal_flush_phase: 0,
        }
    }

    /// Processes one stereo frame and returns the wet echo signal.
    #[inline]
    pub fn process(&mut self, left: f64, right: f64, terminal_gain: f64) -> (f64, f64) {
        let flush = self.denormal_flush_phase == 0;
        self.denormal_flush_phase += 1;
        if self.denormal_flush_phase == DENORMAL_FLUSH_INTERVAL_FRAMES {
            self.denormal_flush_phase = 0;
        }
        if flush {
            self.process_inner::<true>(left, right, terminal_gain)
        }
        else {
            self.process_inner::<false>(left, right, terminal_gain)
        }
    }

    #[inline]
    fn process_inner<const FLUSH: bool>(
        &mut self,
        left: f64,
        right: f64,
        terminal_gain: f64,
    ) -> (f64, f64) {
        let wet_left = self.left.process::<FLUSH>(
            left,
            self.config.feedback,
            self.config.lowpass_a,
            terminal_gain,
        );
        let wet_right = self.right.process::<FLUSH>(
            right,
            self.config.feedback,
            self.config.lowpass_a,
            terminal_gain,
        );
        (wet_left, wet_right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_first_repeat_matches_reference_checkpoint() {
        let config = EchoConfig::new(200, 0.6, 4000.0, 48_000);
        let mut echo = Echo::new(&config);
        let mut first_repeat = 0.0;
        for frame in 0..=config.delay_length() {
            let input = if frame == 0 { 0.5_f64.sqrt() } else { 0.0 };
            let (left, _) = echo.process(input, input, 1.0);
            if frame == config.delay_length() {
                first_repeat = left;
            }
        }
        assert!((first_repeat - 0.288_227_438_667_480_94).abs() <= 1e-15);
    }
}
