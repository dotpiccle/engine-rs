//! Unit tests for the boundary schedule and production render loop.

use piccle_core::curve::Curve;
use piccle_core::model::{
    ContourEntry, Document, Echo, Layer, NoiseSource, Reverb, Source, SpatialEffect, ToneSource,
    VolumeContour, Waveform,
};
use piccle_core::schedule::CANONICAL_SAMPLE_RATE;
use piccle_render::plan::{RenderPlan, SourcePlan};
use piccle_render::renderer::Renderer;

const RATE: u32 = CANONICAL_SAMPLE_RATE;

fn entry(target: f64, hold_ms: u64, transition_ms: u64, curve: Curve) -> ContourEntry {
    ContourEntry { target, hold_ms, transition_ms, transition_curve: curve }
}

fn tone_layer(id: &str, start_ms: u64, duration_ms: u64, wave: Waveform, hz: f64) -> Layer {
    Layer {
        id: id.to_string(),
        start_ms,
        duration_ms,
        source: Source::Tone(ToneSource {
            wave,
            frequencies: vec![entry(hz, 0, 0, Curve::Linear)],
            offset_cents: 0,
        }),
        volume: VolumeContour::constant(1.0),
        balance: 0.0,
        filters: Vec::new(),
    }
}

fn document(duration_ms: u64, layers: Vec<Layer>) -> Document {
    Document {
        name: None,
        description: None,
        duration_ms,
        master_volume_level: 1.0,
        spatial_effects: Vec::new(),
        layers,
    }
}

fn pitch_plan(layer: Layer) -> piccle_render::plan::ContourPlan {
    let plan = RenderPlan::compile_validated(&document(layer.duration_ms, vec![layer]), RATE);
    let SourcePlan::Tone { pitch, .. } = plan.layers()[0].source().clone()
    else {
        panic!("expected a tone layer");
    };
    pitch
}

#[test]
fn contour_holds_entry_value_during_hold() {
    let mut layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let Source::Tone(tone) = &mut layer.source
    else {
        panic!("tone")
    };
    tone.frequencies = vec![entry(100.0, 10, 10, Curve::Linear), entry(200.0, 0, 0, Curve::Linear)];
    let pitch = pitch_plan(layer);
    let mut cursor = 0;
    // Hold spans frames [0, 480); frame 100 is inside the hold.
    assert_eq!(pitch.value_at(&mut cursor, 100), 100.0);
}

#[test]
fn contour_linear_transition_midpoint() {
    let mut layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let Source::Tone(tone) = &mut layer.source
    else {
        panic!("tone")
    };
    tone.frequencies = vec![entry(100.0, 0, 10, Curve::Linear), entry(200.0, 0, 0, Curve::Linear)];
    let pitch = pitch_plan(layer);
    let mut cursor = 0;
    // N = 480 frames; transition frame j = 240 gives t = 0.5.
    assert_eq!(pitch.value_at(&mut cursor, 240), 150.0);
}

#[test]
fn contour_target_is_exact_at_transition_end() {
    let mut layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let Source::Tone(tone) = &mut layer.source
    else {
        panic!("tone")
    };
    tone.frequencies = vec![entry(100.0, 0, 10, Curve::Linear), entry(200.0, 0, 0, Curve::Linear)];
    let pitch = pitch_plan(layer);
    let mut cursor = 0;
    assert_eq!(pitch.value_at(&mut cursor, 480), 200.0);
}

#[test]
fn contour_zero_frame_chain_last_target_wins() {
    let mut layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let Source::Tone(tone) = &mut layer.source
    else {
        panic!("tone")
    };
    tone.frequencies = vec![
        entry(0.1, 0, 0, Curve::Linear),
        entry(0.2, 0, 0, Curve::Linear),
        entry(0.3, 0, 0, Curve::Linear),
    ];
    let pitch = pitch_plan(layer);
    let mut cursor = 0;
    // piccle-spec/docs/10-curves.md: zero-frame jumps process in array order
    // before the boundary frame is emitted.
    assert_eq!(pitch.value_at(&mut cursor, 0), 0.3);
}

#[test]
fn contour_exponential_transition_midpoint_is_geometric_mean() {
    let mut layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let Source::Tone(tone) = &mut layer.source
    else {
        panic!("tone")
    };
    tone.frequencies = vec![entry(0.1, 0, 10, Curve::Exponential), entry(1.0, 0, 0, Curve::Linear)];
    let pitch = pitch_plan(layer);
    let mut cursor = 0;
    // piccle-spec/test-vectors/numeric/dsp-values.json curve_progress_at_half.
    // libm `pow` is allowed 1 ulp of platform variance (docs/11: exact
    // semantics do not require bit-identical transcendentals).
    let got = pitch.value_at(&mut cursor, 240);
    assert!((got - 0.316_227_766_016_837_94).abs() <= 1e-16);
}

#[test]
fn envelope_shorthand_holds_full_level_before_fade() {
    let layer = tone_layer("a", 0, 200, Waveform::Sine, 440.0);
    let plan = RenderPlan::compile_validated(&document(200, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    assert_eq!(envelope.gain(&mut cursor, 100), 1.0);
}

#[test]
fn envelope_shorthand_linear_fadeout_last_frame() {
    let layer = tone_layer("a", 0, 200, Waveform::Sine, 440.0);
    let plan = RenderPlan::compile_validated(&document(200, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    // O = 240 frames; final emitted frame n = T-1 uses the transition
    // formula `start + (target - start) × t` with t = 239/240.
    assert_eq!(envelope.gain(&mut cursor, 9599), 1.0 - 239.0 / 240.0);
}

#[test]
fn envelope_fade_in_reaches_first_level_exactly_at_boundary() {
    let mut layer = tone_layer("a", 0, 200, Waveform::Sine, 440.0);
    layer.volume = VolumeContour {
        fade_in: piccle_core::model::FadeStage { ms: 10, curve: Curve::Linear },
        fade_out: piccle_core::model::FadeStage { ms: 5, curve: Curve::Linear },
        levels: vec![entry(0.8, 0, 0, Curve::Linear)],
    };
    let plan = RenderPlan::compile_validated(&document(200, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    // I = 480 frames; the first level is exact at frame I.
    assert_eq!(envelope.gain(&mut cursor, 480), 0.8);
}

#[test]
fn envelope_fade_in_linear_midpoint() {
    let mut layer = tone_layer("a", 0, 200, Waveform::Sine, 440.0);
    layer.volume = VolumeContour {
        fade_in: piccle_core::model::FadeStage { ms: 10, curve: Curve::Linear },
        fade_out: piccle_core::model::FadeStage { ms: 5, curve: Curve::Linear },
        levels: vec![entry(0.8, 0, 0, Curve::Linear)],
    };
    let plan = RenderPlan::compile_validated(&document(200, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    assert_eq!(envelope.gain(&mut cursor, 240), 0.4);
}

#[test]
fn envelope_exponential_fadeout_follows_floor_formula() {
    let mut layer = tone_layer("a", 0, 200, Waveform::Sine, 440.0);
    layer.volume = VolumeContour {
        fade_in: piccle_core::model::FadeStage { ms: 0, curve: Curve::Linear },
        fade_out: piccle_core::model::FadeStage { ms: 5, curve: Curve::Exponential },
        levels: vec![entry(1.0, 0, 0, Curve::Linear)],
    };
    let plan = RenderPlan::compile_validated(&document(200, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    // Fade starts at frame 9360; at t = 0.5 (frame 9480), s×(e/s)^t with
    // s = 1, e = 1e-10 → sqrt(1e-10).
    assert_eq!(envelope.gain(&mut cursor, 9480), 1e-5);
}

#[test]
fn root_truncation_does_not_relocate_declared_fade() {
    let layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let plan = RenderPlan::compile_validated(&document(30, vec![layer]), RATE);
    let envelope = plan.layers()[0].envelope();
    let mut cursor = 0;
    // Declared fade spans [4560, 4800) but the layer is only active to 1440;
    // the gain stays at the held level through the truncation boundary.
    assert_eq!(envelope.gain(&mut cursor, 1439), 1.0);
}

#[test]
fn plan_active_interval_is_truncated_by_root_duration() {
    let layer = tone_layer("a", 0, 100, Waveform::Sine, 440.0);
    let plan = RenderPlan::compile_validated(&document(30, vec![layer]), RATE);
    assert_eq!(plan.layers()[0].active_end_frame(), 1440);
}

#[test]
fn plan_output_extends_by_reverb_tail() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.25,
        tail_ms: 4,
        soften_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, 44_100);
    // dry frame(4 ms) + reverb tail frame(4 ms) = 176 + 176 at 44.1 kHz.
    assert_eq!(plan.output_frames(), 352);
}

#[test]
fn plan_reverb_tail_frames_come_from_declared_tail() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.25,
        tail_ms: 4,
        soften_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, 44_100);
    let reverb = plan.reverb().expect("reverb");
    assert_eq!(reverb.tail_frames(), 176);
}

#[test]
fn renderer_first_tone_sample_uses_phase_zero() {
    let doc = document(10, vec![tone_layer("a", 0, 10, Waveform::Sine, 1000.0)]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    assert_eq!(output[0], 0.0);
}

#[test]
fn renderer_sine_second_sample_matches_phase_advance() {
    let mut layer = tone_layer("a", 0, 10, Waveform::Sine, 1000.0);
    layer.balance = -1.0;
    let doc = document(10, vec![layer]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    let expected = (2.0 * std::f64::consts::PI * 1000.0 / 48_000.0).sin() as f32;
    assert_eq!(output[2], expected);
}

#[test]
fn renderer_hard_clips_above_full_scale() {
    let mut left_a = tone_layer("a", 0, 10, Waveform::Sine, 1000.0);
    left_a.balance = -1.0;
    let mut left_b = tone_layer("b", 0, 10, Waveform::Sine, 1000.0);
    left_b.balance = -1.0;
    let doc = document(10, vec![left_a, left_b]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    let peak = output.chunks_exact(2).map(|frame| frame[0]).fold(0.0_f32, f32::max);
    assert!(peak <= 1.0);
}

#[test]
fn renderer_rejects_non_finite_post_master_sample_without_reverb() {
    let mut doc = document(10, vec![tone_layer("a", 0, 10, Waveform::Sine, 1000.0)]);
    doc.master_volume_level = f64::NAN;
    let plan = RenderPlan::compile_validated(&doc, RATE);
    assert!(matches!(
        Renderer::render_to_vec(&plan),
        Err(piccle_core::error::PiccleError::Internal(_))
    ));
}

#[test]
fn render_to_vec_rejects_output_above_the_convenience_allocation_limit() {
    let plan = RenderPlan::compile_validated(&document(175_000, Vec::new()), RATE);
    assert!(matches!(
        Renderer::render_to_vec(&plan),
        Err(piccle_core::error::PiccleError::Unsupported { limit: "max_render_to_vec_bytes", .. })
    ));
}

#[test]
fn renderer_amount_zero_reverb_still_extends_timeline() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.0,
        tail_ms: 10,
        soften_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    assert_eq!(output.len(), 2 * 672);
}

#[test]
fn renderer_amount_zero_reverb_tail_is_silent_after_dry_end() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.0,
        tail_ms: 10,
        soften_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    // Dry ends at frame 192; tail frames must be exactly zero at amount 0.
    assert_eq!(output[2 * 200], 0.0);
}

#[test]
fn amount_zero_reverb_skips_wet_processing_configuration() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.0,
        tail_ms: 60_000,
        soften_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, RATE);
    assert!(plan.reverb().is_some_and(|reverb| reverb.config().is_none()));
}

#[test]
fn plan_output_extends_by_longest_spatial_effect_tail() {
    let mut doc = document(4, vec![tone_layer("a", 0, 4, Waveform::Sine, 1000.0)]);
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.0,
        tail_ms: 4,
        soften_hz: 4000.0,
    }));
    doc.spatial_effects.push(SpatialEffect::Echo(Echo {
        delay_ms: 5,
        feedback: 0.0,
        wet_gain: 0.0,
        damp_hz: 4000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, RATE);
    assert_eq!(plan.output_frames(), 432);
}

#[test]
fn parallel_spatial_effect_output_is_independent_of_array_order() {
    let mut forward = document(100, vec![tone_layer("tone", 0, 100, Waveform::Sine, 880.0)]);
    let reverb = SpatialEffect::Reverb(Reverb { amount: 0.2, tail_ms: 100, soften_hz: 4_000.0 });
    let echo =
        SpatialEffect::Echo(Echo { delay_ms: 80, feedback: 0.3, wet_gain: 0.15, damp_hz: 4_000.0 });
    forward.spatial_effects = vec![reverb, echo];
    let mut reverse = forward.clone();
    reverse.spatial_effects = vec![echo, reverb];

    let forward_output = Renderer::render_to_vec(&RenderPlan::compile_validated(&forward, RATE))
        .expect("render forward order");
    let reverse_output = Renderer::render_to_vec(&RenderPlan::compile_validated(&reverse, RATE))
        .expect("render reverse order");

    assert_eq!(forward_output, reverse_output);
}

#[test]
fn renderer_chunked_streaming_matches_one_shot() {
    let doc = document(20, vec![tone_layer("a", 0, 20, Waveform::Triangle, 440.0)]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let one_shot = Renderer::render_to_vec(&plan).expect("render");
    let mut renderer = Renderer::new(&plan);
    let mut streamed = Vec::new();
    let mut chunk = [0.0_f32; 200];
    loop {
        let written = renderer.render_into(&mut chunk).expect("render");
        if written == 0 {
            break;
        }
        streamed.extend_from_slice(&chunk[..2 * written]);
    }
    assert_eq!(streamed, one_shot);
}

#[test]
fn renderer_reset_reproduces_identical_output() {
    let doc = document(10, vec![tone_layer("a", 0, 10, Waveform::Saw, 220.0)]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let mut renderer = Renderer::new(&plan);
    let mut first = vec![0.0_f32; 2 * plan.output_frames() as usize];
    renderer.render_into(&mut first).expect("render");
    renderer.reset();
    let mut second = vec![0.0_f32; 2 * plan.output_frames() as usize];
    renderer.render_into(&mut second).expect("render");
    assert_eq!(first, second);
}

#[test]
fn renderer_mixes_layers_in_document_order_with_balance() {
    let mut left = Layer {
        id: "left".to_string(),
        start_ms: 0,
        duration_ms: 10,
        source: Source::Noise(NoiseSource {
            character: piccle_core::model::NoiseCharacter::Neutral,
            seed: 0,
        }),
        volume: VolumeContour::constant(1.0),
        balance: -1.0,
        filters: Vec::new(),
    };
    left.balance = -1.0;
    let mut right = left.clone();
    right.id = "right".to_string();
    right.balance = 1.0;
    let doc = document(10, vec![left, right]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let output = Renderer::render_to_vec(&plan).expect("render");
    // piccle-spec/test-vectors/numeric/dsp-values.json pcg32_seed_0 first word.
    let x = 2.0 * (3_894_649_422.0 / 4_294_967_296.0) - 1.0;
    let gain = 0.25 / (1.0_f64 / 3.0).sqrt();
    let expected_left = (x * gain) as f32;
    assert_eq!(output[0], expected_left);
}

#[test]
fn frame_cursor_advances_with_each_rendered_chunk() {
    let doc = document(10, vec![tone_layer("a", 0, 10, Waveform::Sine, 440.0)]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let mut renderer = Renderer::new(&plan);
    let mut buffer = vec![0.0f32; 100 * 2];
    renderer.render_into(&mut buffer).expect("render");
    assert_eq!(renderer.frame_cursor(), 100);
}

#[test]
fn is_finished_after_the_final_frame() {
    let doc = document(1, vec![tone_layer("a", 0, 1, Waveform::Sine, 440.0)]);
    let plan = RenderPlan::compile_validated(&doc, RATE);
    let mut renderer = Renderer::new(&plan);
    let mut buffer = vec![0.0f32; plan.output_frames() as usize * 2];
    renderer.render_into(&mut buffer).expect("render");
    assert!(renderer.is_finished());
}

#[test]
fn terminal_window_gain_is_zero_beyond_the_output_end() {
    assert_eq!(piccle_render::renderer::terminal_window_gain(100, 100, 5), 0.0);
}

#[test]
fn one_frame_terminal_window_reaches_zero_without_nan() {
    assert_eq!(piccle_render::renderer::terminal_window_gain(9, 10, 1), 0.0);
}
