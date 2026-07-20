//! Production render loop.
//!
//! Spec: piccle-spec/docs/13-implementer-notes.md §Render-loop discipline —
//! rendering never allocates, parses, sorts, or searches from the beginning.
//! All state lives in pre-built per-layer voices; contour cursors only move
//! forward.

use piccle_core::error::PiccleError;
use piccle_core::schedule::FREQUENCY_MIN_HZ;
use piccle_dsp::echo::Echo;
use piccle_dsp::filter::Biquad;
use piccle_dsp::noise::NoiseVoice;
use piccle_dsp::oscillator::Oscillator;
use piccle_dsp::reverb::Reverb;

use crate::plan::{RenderPlan, SourcePlan, SpatialEffectPlan};

/// Maximum allocation made by [`Renderer::render_to_vec`] (64 MiB).
///
/// Longer assets remain renderable through chunked [`Renderer::render_into`]
/// without a document-sized allocation.
pub const MAX_RENDER_TO_VEC_BYTES: u64 = 64 * 1024 * 1024;

/// Per-layer runtime state, built once from the plan.
#[derive(Debug)]
struct LayerVoice {
    source: VoiceSource,
    filters: Vec<Biquad>,
    pitch_cursor: usize,
    filter_cursors: Vec<usize>,
    volume_cursor: usize,
    last_frequency_bits: u64,
}

#[derive(Debug)]
enum VoiceSource {
    Tone(Oscillator),
    Noise(NoiseVoice),
}

impl LayerVoice {
    fn new(layer: &crate::plan::LayerPlan, sample_rate: u32) -> Self {
        let source = match layer.source() {
            SourcePlan::Tone { wave, .. } => VoiceSource::Tone(Oscillator::new(*wave, sample_rate)),
            SourcePlan::Noise { character, seed } => {
                VoiceSource::Noise(NoiseVoice::new(*character, *seed, sample_rate))
            }
        };
        let filters = layer
            .filters()
            .iter()
            .map(|filter| Biquad::new(filter.filter_type, filter.resonance, sample_rate))
            .collect::<Vec<_>>();
        let filter_cursors = vec![0_usize; layer.filters().len()];
        Self {
            source,
            filters,
            pitch_cursor: 0,
            filter_cursors,
            volume_cursor: 0,
            last_frequency_bits: 0,
        }
    }
}

#[derive(Debug)]
enum SpatialEffectVoice {
    Reverb(Option<Box<Reverb>>),
    Echo(Option<Echo>),
}

/// Streaming renderer over an immutable [`RenderPlan`].
///
/// The renderer owns all mutable voice state; the plan stays shared and
/// immutable so one plan can feed any number of renderers.
#[derive(Debug)]
pub struct Renderer<'a> {
    plan: &'a RenderPlan,
    frame_cursor: u64,
    voices: Vec<LayerVoice>,
    active_layers: Vec<usize>,
    next_start: usize,
    next_end: usize,
    next_boundary_frame: u64,
    spatial_effects: Vec<SpatialEffectVoice>,
}

impl<'a> Renderer<'a> {
    /// Builds a renderer with zeroed voice and reverb state.
    #[must_use]
    pub fn new(plan: &'a RenderPlan) -> Self {
        piccle_dsp::oscillator::init_sine_lut();
        let voices =
            plan.layers().iter().map(|layer| LayerVoice::new(layer, plan.sample_rate())).collect();
        let active_layers = Vec::with_capacity(plan.layers().len());
        let next_start_frame = plan
            .start_order()
            .first()
            .map_or(u64::MAX, |&index| plan.layers()[index].start_frame());
        let next_end_frame = plan
            .end_order()
            .first()
            .map_or(u64::MAX, |&index| plan.layers()[index].active_end_frame());
        let spatial_effects = plan
            .spatial_effects()
            .iter()
            .map(|effect| match effect {
                SpatialEffectPlan::Reverb(reverb) => SpatialEffectVoice::Reverb(
                    reverb.config().map(|config| Box::new(Reverb::new(config))),
                ),
                SpatialEffectPlan::Echo(echo) => {
                    SpatialEffectVoice::Echo(echo.config().map(Echo::new))
                }
            })
            .collect();
        Self {
            plan,
            frame_cursor: 0,
            voices,
            active_layers,
            next_start: 0,
            next_end: 0,
            next_boundary_frame: next_start_frame.min(next_end_frame),
            spatial_effects,
        }
    }

    /// Renders the whole plan into a freshly allocated interleaved stereo
    /// buffer (`2 × output_frames` samples).
    ///
    /// # Errors
    ///
    /// Returns [`PiccleError::Unsupported`] when the convenience allocation
    /// would exceed [`MAX_RENDER_TO_VEC_BYTES`], or [`PiccleError::Internal`]
    /// if allocation fails or a sample becomes non-finite.
    pub fn render_to_vec(plan: &RenderPlan) -> Result<Vec<f32>, PiccleError> {
        let output_bytes = plan
            .output_frames()
            .checked_mul(2)
            .and_then(|samples| samples.checked_mul(std::mem::size_of::<f32>() as u64))
            .ok_or_else(|| PiccleError::Unsupported {
                limit: "max_render_to_vec_bytes",
                actual: "overflow".to_owned(),
                max: MAX_RENDER_TO_VEC_BYTES.to_string(),
            })?;
        if output_bytes > MAX_RENDER_TO_VEC_BYTES {
            return Err(PiccleError::Unsupported {
                limit: "max_render_to_vec_bytes",
                actual: output_bytes.to_string(),
                max: MAX_RENDER_TO_VEC_BYTES.to_string(),
            });
        }
        let samples = usize::try_from(output_bytes / std::mem::size_of::<f32>() as u64)
            .map_err(|_| PiccleError::internal("output buffer does not fit this platform"))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(samples)
            .map_err(|_| PiccleError::internal("unable to allocate output buffer"))?;
        output.resize(samples, 0.0_f32);
        let mut renderer = Renderer::new(plan);
        renderer.render_into(&mut output)?;
        Ok(output)
    }

    /// Absolute frame the renderer will emit next.
    #[must_use]
    pub fn frame_cursor(&self) -> u64 {
        self.frame_cursor
    }

    /// Whether the renderer has emitted every output frame.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.frame_cursor >= self.plan.output_frames()
    }

    /// Restores the renderer to its initial zero state.
    pub fn reset(&mut self) {
        *self = Renderer::new(self.plan);
    }

    /// Fills `output` with up to `output.len() / 2` interleaved stereo frames
    /// and returns how many frames were written. Call again to continue;
    /// rendering stops at the plan's output end.
    ///
    /// This function performs no allocation.
    ///
    /// # Errors
    ///
    /// Returns `PiccleError::Internal` if a non-finite sample is produced;
    /// the document must not be rendered further.
    ///
    /// Spec: piccle-spec/docs/08-output.md §Signal flow.
    pub fn render_into(&mut self, output: &mut [f32]) -> Result<usize, PiccleError> {
        let capacity = output.len() / 2;
        let mut written = 0_usize;
        while written < capacity && self.frame_cursor < self.plan.output_frames() {
            let frame = self.frame_cursor;
            let (dry_left, dry_right) = self.mix_layers(frame)?;
            let (out_left, out_right) = self.apply_spatial_effects(frame, dry_left, dry_right)?;
            let master = self.plan.master_volume_level();
            let mastered_left = out_left * master;
            let mastered_right = out_right * master;
            if !mastered_left.is_finite() || !mastered_right.is_finite() {
                return Err(PiccleError::internal("non-finite sample produced during render"));
            }
            let final_left = mastered_left.clamp(-1.0, 1.0);
            let final_right = mastered_right.clamp(-1.0, 1.0);
            output[2 * written] = final_left as f32;
            output[2 * written + 1] = final_right as f32;
            written += 1;
            self.frame_cursor += 1;
        }
        Ok(written)
    }

    /// Source → filters → envelope → pan → dry sum, in document array order.
    fn mix_layers(&mut self, frame: u64) -> Result<(f64, f64), PiccleError> {
        self.update_active_layers(frame);
        let mut dry_left = 0.0_f64;
        let mut dry_right = 0.0_f64;
        for &index in &self.active_layers {
            let layer = &self.plan.layers()[index];
            let voice = &mut self.voices[index];
            let mut sample = match (layer.source(), &mut voice.source) {
                (SourcePlan::Tone { pitch, offset_factor, .. }, VoiceSource::Tone(oscillator)) => {
                    // piccle-spec/docs/04-pitch.md: contour → cents → clamp.
                    let contour_hz = pitch.value_at(&mut voice.pitch_cursor, frame);
                    let hz = (contour_hz * offset_factor)
                        .clamp(FREQUENCY_MIN_HZ, self.plan.frequency_max());
                    if hz.to_bits() != voice.last_frequency_bits {
                        oscillator.set_frequency(hz);
                        voice.last_frequency_bits = hz.to_bits();
                    }
                    oscillator.next_sample()
                }
                (SourcePlan::Noise { .. }, VoiceSource::Noise(noise)) => noise.next_sample(),
                _ => {
                    return Err(PiccleError::internal("render plan and voice state disagree"));
                }
            };
            for (filter_index, biquad) in voice.filters.iter_mut().enumerate() {
                let Some(filter) = layer.filters().get(filter_index)
                else {
                    return Err(PiccleError::internal("render plan and filter state disagree"));
                };
                let Some(cursor) = voice.filter_cursors.get_mut(filter_index)
                else {
                    return Err(PiccleError::internal("render plan and filter state disagree"));
                };
                // piccle-spec/docs/06-filters.md: per-frame contour, clamp,
                // coefficients, then process (the biquad clamps internally).
                let cutoff = filter.frequencies.value_at(cursor, frame);
                biquad.set_frequency(cutoff);
                sample = biquad.process(sample);
            }
            let gain = layer.envelope().gain(&mut voice.volume_cursor, frame);
            sample *= gain;
            dry_left += sample * layer.pan_left();
            dry_right += sample * layer.pan_right();
        }
        Ok((dry_left, dry_right))
    }

    /// Advances the pre-sorted boundary schedule while retaining document
    /// order for deterministic layer summation.
    fn update_active_layers(&mut self, frame: u64) {
        if frame < self.next_boundary_frame {
            return;
        }

        let end_order = self.plan.end_order();
        while let Some(&index) = end_order.get(self.next_end) {
            if self.plan.layers()[index].active_end_frame() > frame {
                break;
            }
            self.next_end += 1;
            if let Ok(slot) = self.active_layers.binary_search(&index) {
                self.active_layers.remove(slot);
            }
        }

        let start_order = self.plan.start_order();
        while let Some(&index) = start_order.get(self.next_start) {
            let layer = &self.plan.layers()[index];
            if layer.start_frame() > frame {
                break;
            }
            self.next_start += 1;
            if layer.active_end_frame() <= frame {
                continue;
            }
            let insertion = self.active_layers.binary_search(&index).unwrap_or_else(|slot| slot);
            self.active_layers.insert(insertion, index);
        }

        let next_start_frame = start_order
            .get(self.next_start)
            .map_or(u64::MAX, |&index| self.plan.layers()[index].start_frame());
        let next_end_frame = end_order
            .get(self.next_end)
            .map_or(u64::MAX, |&index| self.plan.layers()[index].active_end_frame());
        self.next_boundary_frame = next_start_frame.min(next_end_frame);
    }

    /// Parallel additive spatial effects plus the dry mix.
    fn apply_spatial_effects(
        &mut self,
        frame: u64,
        dry_left: f64,
        dry_right: f64,
    ) -> Result<(f64, f64), PiccleError> {
        let mut out_left = dry_left;
        let mut out_right = dry_right;
        for index in 0..self.plan.spatial_effects().len() {
            let Some(effect_plan) = self.plan.spatial_effects().get(index)
            else {
                return Err(PiccleError::internal("render plan and spatial effect state disagree"));
            };
            let Some(effect_voice) = self.spatial_effects.get_mut(index)
            else {
                return Err(PiccleError::internal("render plan and spatial effect state disagree"));
            };
            let effect_end = self.plan.dry_end_frame() + effect_plan.tail_frames();
            if frame >= effect_end {
                continue;
            }
            let terminal_gain =
                terminal_window_gain(frame, effect_end, effect_plan.window_frames());
            match (effect_plan, effect_voice) {
                (SpatialEffectPlan::Reverb(reverb_plan), SpatialEffectVoice::Reverb(reverb)) => {
                    if reverb_plan.amount() == 0.0 {
                        continue;
                    }
                    let Some(reverb) = reverb.as_mut()
                    else {
                        return Err(PiccleError::internal("render plan and reverb state disagree"));
                    };
                    let (wet_left, wet_right) = reverb.process(dry_left, dry_right, terminal_gain);
                    out_left += reverb_plan.amount() * wet_left;
                    out_right += reverb_plan.amount() * wet_right;
                }
                (SpatialEffectPlan::Echo(echo_plan), SpatialEffectVoice::Echo(echo)) => {
                    if echo_plan.wet_gain() == 0.0 {
                        continue;
                    }
                    let Some(echo) = echo.as_mut()
                    else {
                        return Err(PiccleError::internal("render plan and echo state disagree"));
                    };
                    let (wet_left, wet_right) = echo.process(dry_left, dry_right, terminal_gain);
                    out_left += echo_plan.wet_gain() * wet_left;
                    out_right += echo_plan.wet_gain() * wet_right;
                }
                _ => {
                    return Err(PiccleError::internal(
                        "render plan and spatial effect state disagree",
                    ));
                }
            }
        }
        Ok((out_left, out_right))
    }
}

/// Automatic terminal-window gain at absolute `frame`.
///
/// Spec: piccle-spec/docs/07-spatial-effects.md — 1 before `T - W`, then a
/// linear ramp reaching exactly 0 on the final emitted frame.
#[must_use]
pub fn terminal_window_gain(frame: u64, output_end: u64, window: u64) -> f64 {
    if window <= 1 {
        return if frame < output_end.saturating_sub(1) { 1.0 } else { 0.0 };
    }
    if frame < output_end.saturating_sub(window) {
        return 1.0;
    }
    if frame < output_end {
        return (output_end - 1 - frame) as f64 / (window - 1) as f64;
    }
    0.0
}
