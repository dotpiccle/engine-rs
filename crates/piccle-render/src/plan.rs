//! Immutable render plan: the resolved document plus its absolute frame
//! boundary schedule.
//!
//! Spec: piccle-spec/docs/11-engine-safety.md — "Engines MUST construct one
//! absolute boundary schedule before rendering." Every hold, transition, and
//! fade length is derived by subtracting absolute boundary frames; nothing is
//! rounded independently.

use piccle_core::curve::Curve;
use piccle_core::model::{
    ContourEntry, Document, FilterType, NoiseCharacter, Source, SpatialEffect, Waveform,
};
use piccle_core::schedule::{echo_repeat_count, frame_at, render_frequency_max};
use piccle_dsp::echo::EchoConfig;
use piccle_dsp::reverb::{ReverbConfig, terminal_window_frames};

/// One contour segment: hold `start_value` through `hold_end`, then move to
/// `target_value` across `[hold_end, transition_end)`.
///
/// Spec: piccle-spec/docs/10-curves.md §Frame scheduling.
#[derive(Debug, Clone, Copy)]
pub struct ContourSegment {
    /// First absolute frame no longer holding `start_value`.
    pub hold_end: u64,
    /// First absolute frame at which `target_value` is exact.
    pub transition_end: u64,
    /// Exact declared value held before the transition.
    pub start_value: f64,
    /// Exact declared value reached at `transition_end`.
    pub target_value: f64,
    /// Curve shaping the transition.
    pub curve: Curve,
}

/// A compiled contour: ordered segments plus the final held target. Forward
/// evaluation only; the render loop keeps a per-voice cursor.
#[derive(Debug, Clone)]
pub struct ContourPlan {
    segments: Vec<ContourSegment>,
    final_target: f64,
}

impl ContourPlan {
    /// Compiles `entries` whose cumulative offsets start at absolute
    /// document-time `base_ms`.
    ///
    /// Spec: piccle-spec/docs/10-curves.md — offsets are cumulative from the
    /// contour origin and every boundary converts via `frame(S + c)`.
    fn new(entries: &[ContourEntry], base_ms: u64, sample_rate: u32) -> Self {
        let mut segments = Vec::with_capacity(entries.len().saturating_sub(1));
        let mut offset_ms = 0_u64;
        for pair in entries.windows(2) {
            let entry = &pair[0];
            let next = &pair[1];
            let hold_end_ms = base_ms + offset_ms + entry.hold_ms;
            let transition_end_ms = hold_end_ms + entry.transition_ms;
            segments.push(ContourSegment {
                hold_end: frame_at(hold_end_ms, sample_rate),
                transition_end: frame_at(transition_end_ms, sample_rate),
                start_value: entry.target,
                target_value: next.target,
                curve: entry.transition_curve,
            });
            offset_ms += entry.hold_ms + entry.transition_ms;
        }
        let final_target = entries.last().map_or(0.0, |entry| entry.target);
        Self { segments, final_target }
    }

    /// Number of compiled segments (entries minus one).
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Value at absolute `frame`, advancing `cursor` forward past finished
    /// segments. Zero-frame segments collapse in array order; the last target
    /// reached at a shared boundary becomes current.
    ///
    /// Spec: piccle-spec/docs/10-curves.md — transition frame `j` of `N` uses
    /// `t = j / N`; the exact target begins at the following frame.
    pub fn value_at(&self, cursor: &mut usize, frame: u64) -> f64 {
        let mut index = *cursor;
        while index < self.segments.len() && frame >= self.segments[index].transition_end {
            index += 1;
        }
        *cursor = index;
        let Some(segment) = self.segments.get(index)
        else {
            return self.final_target;
        };
        if frame < segment.hold_end {
            return segment.start_value;
        }
        let length = segment.transition_end - segment.hold_end;
        let j = frame - segment.hold_end;
        let t = j as f64 / length as f64;
        segment.curve.value(segment.start_value, segment.target_value, t)
    }
}

/// Compiled layer-volume envelope: fade-in, level contour, fade-out.
///
/// Spec: piccle-spec/docs/05-layer-volume.md §Layer-envelope algorithm.
#[derive(Debug, Clone)]
pub struct EnvelopePlan {
    contour: ContourPlan,
    level0: f64,
    fade_in_curve: Curve,
    fade_in_frames: u64,
    fade_out_curve: Curve,
    fade_out_frames: u64,
    /// Absolute frame at which the fade-out begins: `frame(fade_start_ms)`.
    fade_start_frame: u64,
    /// Absolute layer start frame `frame(S)`.
    start_frame: u64,
}

impl EnvelopePlan {
    /// Envelope gain at absolute `frame` (only called inside the layer's
    /// active interval).
    ///
    /// Spec: piccle-spec/docs/05-layer-volume.md — `I`, `O`, and `T` come
    /// from absolute-boundary subtraction; piccle-spec/docs/10-curves.md
    /// §Fade curves — a fade is a transition between silence and the level.
    pub fn gain(&self, cursor: &mut usize, frame: u64) -> f64 {
        let local = frame - self.start_frame;
        if self.fade_in_frames > 0 && local < self.fade_in_frames {
            let t = local as f64 / self.fade_in_frames as f64;
            return self.fade_in_curve.value(0.0, self.level0, t);
        }
        let held = self.contour.value_at(cursor, frame);
        if self.fade_out_frames > 0 && frame >= self.fade_start_frame {
            let t = (frame - self.fade_start_frame) as f64 / self.fade_out_frames as f64;
            return self.fade_out_curve.value(held, 0.0, t);
        }
        held
    }

    /// Absolute frame at which the fade-out begins (conformance evidence).
    #[must_use]
    pub fn fade_start_frame(&self) -> u64 {
        self.fade_start_frame
    }

    /// Fade-out length in frames, `O` (conformance evidence).
    #[must_use]
    pub fn fade_frames(&self) -> u64 {
        self.fade_out_frames
    }
}

/// Compiled tone or noise source parameters.
#[derive(Debug, Clone)]
pub enum SourcePlan {
    /// Tone: waveform plus pitch contour and precomputed cents factor.
    Tone {
        /// Oscillator waveform.
        wave: Waveform,
        /// Pitch contour in Hz, origin at the layer start.
        pitch: ContourPlan,
        /// `2^(offset_cents / 1200)`, applied after the contour, before clamp.
        offset_factor: f64,
    },
    /// Noise: deterministic character and seed.
    Noise {
        /// Noise character (filter curve).
        character: NoiseCharacter,
        /// PCG32 stream seed.
        seed: u32,
    },
}

/// Compiled filter: fixed type/resonance plus its cutoff contour.
#[derive(Debug, Clone)]
pub struct FilterPlan {
    /// Biquad type, fixed for the layer.
    pub filter_type: FilterType,
    /// Declared resonance in `[0, 1]`.
    pub resonance: f64,
    /// Cutoff contour in Hz, origin at the layer start.
    pub frequencies: ContourPlan,
}

/// One layer's compiled schedule and parameters.
#[derive(Debug, Clone)]
pub struct LayerPlan {
    start_frame: u64,
    declared_end_frame: u64,
    active_end_frame: u64,
    source: SourcePlan,
    filters: Vec<FilterPlan>,
    envelope: EnvelopePlan,
    pan_left: f64,
    pan_right: f64,
}

impl LayerPlan {
    /// Absolute frame at which the layer starts: `frame(S)`.
    #[must_use]
    pub fn start_frame(&self) -> u64 {
        self.start_frame
    }

    /// Absolute declared end frame: `frame(S + duration_ms)`.
    #[must_use]
    pub fn declared_end_frame(&self) -> u64 {
        self.declared_end_frame
    }

    /// Absolute active end frame: `min(frame(E), frame(D))`.
    #[must_use]
    pub fn active_end_frame(&self) -> u64 {
        self.active_end_frame
    }

    /// Compiled source parameters.
    #[must_use]
    pub fn source(&self) -> &SourcePlan {
        &self.source
    }

    /// Compiled filters, applied in array order.
    #[must_use]
    pub fn filters(&self) -> &[FilterPlan] {
        &self.filters
    }

    /// Compiled volume envelope.
    #[must_use]
    pub fn envelope(&self) -> &EnvelopePlan {
        &self.envelope
    }

    /// Static equal-power left gain.
    #[must_use]
    pub fn pan_left(&self) -> f64 {
        self.pan_left
    }

    /// Static equal-power right gain.
    #[must_use]
    pub fn pan_right(&self) -> f64 {
        self.pan_right
    }
}

/// Compiled reverb: calibrated config plus production tail schedule.
#[derive(Debug, Clone)]
pub struct ReverbPlan {
    config: Option<ReverbConfig>,
    amount: f64,
    tail_frames: u64,
    window_frames: u64,
}

impl ReverbPlan {
    /// Calibrated reverb configuration (delay lengths, gains, norm), omitted
    /// when a zero amount makes the wet path inaudible.
    #[must_use]
    pub fn config(&self) -> Option<&ReverbConfig> {
        self.config.as_ref()
    }

    /// Declared additive wet gain.
    #[must_use]
    pub fn amount(&self) -> f64 {
        self.amount
    }

    /// Production tail length: `frame(tail_ms)`.
    #[must_use]
    pub fn tail_frames(&self) -> u64 {
        self.tail_frames
    }

    /// Automatic terminal-window width `W` in frames.
    #[must_use]
    pub fn window_frames(&self) -> u64 {
        self.window_frames
    }
}

/// Compiled echo: prepared delay-line config plus production tail schedule.
#[derive(Debug, Clone)]
pub struct EchoPlan {
    config: Option<EchoConfig>,
    wet_gain: f64,
    tail_frames: u64,
    window_frames: u64,
}

impl EchoPlan {
    /// Prepared echo configuration, omitted when `wet_gain` makes the wet path
    /// inaudible.
    #[must_use]
    pub fn config(&self) -> Option<&EchoConfig> {
        self.config.as_ref()
    }

    /// Declared additive wet gain.
    #[must_use]
    pub fn wet_gain(&self) -> f64 {
        self.wet_gain
    }

    /// Production tail length: `N_total × delay_length`.
    #[must_use]
    pub fn tail_frames(&self) -> u64 {
        self.tail_frames
    }

    /// Automatic terminal-window width `W` in frames.
    #[must_use]
    pub fn window_frames(&self) -> u64 {
        self.window_frames
    }
}

/// Compiled whole-document spatial effect.
#[derive(Debug, Clone)]
pub enum SpatialEffectPlan {
    /// Diffuse additive reverb.
    Reverb(Box<ReverbPlan>),
    /// Discrete additive echo.
    Echo(EchoPlan),
}

impl SpatialEffectPlan {
    /// Effect tail length in frames.
    #[must_use]
    pub fn tail_frames(&self) -> u64 {
        match self {
            Self::Reverb(reverb) => reverb.tail_frames(),
            Self::Echo(echo) => echo.tail_frames(),
        }
    }

    /// Effect terminal-window width in frames.
    #[must_use]
    pub fn window_frames(&self) -> u64 {
        match self {
            Self::Reverb(reverb) => reverb.window_frames(),
            Self::Echo(echo) => echo.window_frames(),
        }
    }
}

/// Immutable render plan consumed by [`crate::renderer::Renderer`].
///
/// Construction performs every allocation the render path will ever need;
/// rendering itself is allocation-free.
#[derive(Debug, Clone)]
pub struct RenderPlan {
    sample_rate: u32,
    frequency_max: f64,
    dry_end_frame: u64,
    output_end_frame: u64,
    master_volume_level: f64,
    layers: Vec<LayerPlan>,
    start_order: Vec<usize>,
    end_order: Vec<usize>,
    spatial_effects: Vec<SpatialEffectPlan>,
}

impl RenderPlan {
    /// Compiles a trusted, already validated document into an immutable plan.
    ///
    /// This low-level crate cannot establish that `document` passed Piccle's
    /// parser, schema, semantic, and engine-limit checks. Applications should
    /// use `piccle::prepare` instead. Direct callers are responsible for
    /// enforcing that complete boundary before calling this function.
    ///
    /// Spec: piccle-spec/docs/08-output.md §Document and output timelines.
    #[must_use]
    pub fn compile_validated(document: &Document, sample_rate: u32) -> Self {
        let dry_end_frame = frame_at(document.duration_ms, sample_rate);

        let layers: Vec<LayerPlan> = document
            .layers
            .iter()
            .map(|layer| {
                let start_ms = layer.start_ms;
                let end_ms = start_ms + layer.duration_ms;
                let start_frame = frame_at(start_ms, sample_rate);
                let declared_end_frame = frame_at(end_ms, sample_rate);
                let active_end_frame = declared_end_frame.min(dry_end_frame);

                let source = match &layer.source {
                    Source::Tone(tone) => {
                        piccle_dsp::oscillator::prepare_waveform(tone.wave);
                        let pitch = ContourPlan::new(&tone.frequencies, start_ms, sample_rate);
                        // piccle-spec/docs/04-pitch.md: contour × 2^(cents/1200),
                        // clamped afterwards at render time.
                        let offset_factor = (f64::from(tone.offset_cents) / 1200.0).exp2();
                        SourcePlan::Tone { wave: tone.wave, pitch, offset_factor }
                    }
                    Source::Noise(noise) => {
                        SourcePlan::Noise { character: noise.character, seed: noise.seed }
                    }
                };

                let filters = layer
                    .filters
                    .iter()
                    .map(|filter| FilterPlan {
                        filter_type: filter.filter_type,
                        resonance: filter.resonance,
                        frequencies: ContourPlan::new(&filter.frequencies, start_ms, sample_rate),
                    })
                    .collect();

                // piccle-spec/docs/05-layer-volume.md: I, O, and fade_start
                // from absolute-boundary subtraction; contour offsets begin
                // after fade_in.ms.
                let fade_in_frames =
                    frame_at(start_ms + layer.volume.fade_in.ms, sample_rate) - start_frame;
                let effective_fade_out_ms = layer.volume.fade_out.ms.min(layer.duration_ms);
                let fade_start_ms = end_ms - effective_fade_out_ms;
                let fade_start_frame = frame_at(fade_start_ms, sample_rate);
                let fade_out_frames = declared_end_frame - fade_start_frame;
                let level0 = layer.volume.levels.first().map_or(0.0, |entry| entry.target);
                let contour = ContourPlan::new(
                    &layer.volume.levels,
                    start_ms + layer.volume.fade_in.ms,
                    sample_rate,
                );
                let envelope = EnvelopePlan {
                    contour,
                    level0,
                    fade_in_curve: layer.volume.fade_in.curve,
                    fade_in_frames,
                    fade_out_curve: layer.volume.fade_out.curve,
                    fade_out_frames,
                    fade_start_frame,
                    start_frame,
                };

                // piccle-spec/docs/08-output.md §Equal-power balance.
                let x = (layer.balance + 1.0) / 2.0;
                let pan_left = (x * std::f64::consts::FRAC_PI_2).cos();
                let pan_right = (x * std::f64::consts::FRAC_PI_2).sin();

                LayerPlan {
                    start_frame,
                    declared_end_frame,
                    active_end_frame,
                    source,
                    filters,
                    envelope,
                    pan_left,
                    pan_right,
                }
            })
            .collect();

        let mut start_order = (0..layers.len()).collect::<Vec<_>>();
        start_order.sort_by_key(|&index| (layers[index].start_frame, index));
        let mut end_order = (0..layers.len()).collect::<Vec<_>>();
        end_order.sort_by_key(|&index| (layers[index].active_end_frame, index));

        let mut spatial_effects = document.spatial_effects.iter().collect::<Vec<_>>();
        spatial_effects.sort_by_key(|effect| match effect {
            SpatialEffect::Reverb(reverb) => {
                [0, reverb.tail_ms, reverb.amount.to_bits(), reverb.soften_hz.to_bits(), 0]
            }
            SpatialEffect::Echo(echo) => [
                1,
                echo.delay_ms,
                echo.feedback.to_bits(),
                echo.wet_gain.to_bits(),
                echo.damp_hz.to_bits(),
            ],
        });
        let spatial_effects = spatial_effects
            .into_iter()
            .map(|effect| match effect {
                SpatialEffect::Reverb(reverb) => {
                    let tail_frames = frame_at(reverb.tail_ms, sample_rate);
                    let config = (reverb.amount > 0.0)
                        .then(|| ReverbConfig::new(reverb.tail_ms, reverb.soften_hz, sample_rate));
                    let window_frames = terminal_window_frames(tail_frames, sample_rate);
                    SpatialEffectPlan::Reverb(Box::new(ReverbPlan {
                        config,
                        amount: reverb.amount,
                        tail_frames,
                        window_frames,
                    }))
                }
                SpatialEffect::Echo(echo) => {
                    let repeat_count = echo_repeat_count(echo.feedback).unwrap_or(0);
                    let delay_length = frame_at(echo.delay_ms, sample_rate).max(1);
                    let tail_frames = repeat_count * delay_length;
                    let config = (echo.wet_gain > 0.0).then(|| {
                        EchoConfig::new(echo.delay_ms, echo.feedback, echo.damp_hz, sample_rate)
                    });
                    let window_frames = terminal_window_frames(tail_frames, sample_rate);
                    SpatialEffectPlan::Echo(EchoPlan {
                        config,
                        wet_gain: echo.wet_gain,
                        tail_frames,
                        window_frames,
                    })
                }
            })
            .collect::<Vec<_>>();
        let max_tail_frames =
            spatial_effects.iter().map(SpatialEffectPlan::tail_frames).max().unwrap_or(0);
        let output_end_frame = dry_end_frame + max_tail_frames;

        Self {
            sample_rate,
            frequency_max: render_frequency_max(sample_rate),
            dry_end_frame,
            output_end_frame,
            master_volume_level: document.master_volume_level,
            layers,
            start_order,
            end_order,
            spatial_effects,
        }
    }

    /// Render sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// `render_frequency_max` for this plan's profile.
    #[must_use]
    pub fn frequency_max(&self) -> f64 {
        self.frequency_max
    }

    /// Absolute frame at which the dry mix ends: `frame(D)`.
    #[must_use]
    pub fn dry_end_frame(&self) -> u64 {
        self.dry_end_frame
    }

    /// Total output length in frames: `frame(D) + max_i(tail_frames_i)`.
    #[must_use]
    pub fn output_frames(&self) -> u64 {
        self.output_end_frame
    }

    /// Root master gain, applied after spatial effects.
    #[must_use]
    pub fn master_volume_level(&self) -> f64 {
        self.master_volume_level
    }

    /// Compiled layers in document array order.
    #[must_use]
    pub fn layers(&self) -> &[LayerPlan] {
        &self.layers
    }

    pub(crate) fn start_order(&self) -> &[usize] {
        &self.start_order
    }

    pub(crate) fn end_order(&self) -> &[usize] {
        &self.end_order
    }

    /// Compiled spatial effects.
    #[must_use]
    pub fn spatial_effects(&self) -> &[SpatialEffectPlan] {
        &self.spatial_effects
    }

    /// First compiled reverb, when the document declares one.
    #[must_use]
    pub fn reverb(&self) -> Option<&ReverbPlan> {
        self.spatial_effects.iter().find_map(|effect| match effect {
            SpatialEffectPlan::Reverb(reverb) => Some(reverb.as_ref()),
            SpatialEffectPlan::Echo(_) => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use piccle_core::curve::Curve;

    use super::*;

    #[test]
    fn contour_plan_reports_one_segment_per_entry_pair() {
        let entries = vec![
            ContourEntry {
                target: 440.0,
                hold_ms: 0,
                transition_ms: 0,
                transition_curve: Curve::Linear,
            },
            ContourEntry {
                target: 880.0,
                hold_ms: 5,
                transition_ms: 5,
                transition_curve: Curve::Linear,
            },
            ContourEntry {
                target: 660.0,
                hold_ms: 0,
                transition_ms: 0,
                transition_curve: Curve::Linear,
            },
        ];
        let contour = ContourPlan::new(&entries, 0, 48_000);
        assert_eq!(contour.segment_count(), 2);
    }
}
