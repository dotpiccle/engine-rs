//! Performance benches (AGENTS.md §9).
//!
//! - `reverb_tail_flat_cost`: FDN reverb per-frame arithmetic MUST remain
//!   constant while its retained delay state scales with `tail_ms`.
//! - `voice_scaling`: steady render cost scales with active voices.
//! - `contour_boundary_cost`: advancing every active pitch-contour cursor on
//!   one frame does not introduce an unbounded render-cost spike.
//! - `oscillator_harmonic_load`: guards the bounded-cost wavetable path against
//!   regressing toward work proportional to retained harmonics.
//!
//! Each timed iteration renders `CHUNK_FRAMES` into a shared caller-owned
//! buffer with a freshly constructed (zeroed) renderer; plan construction
//! (including reverb calibration) happens once outside the timed region.
//! Render errors cannot occur on these finite, prepared plans, so the
//! workspace's no-unwrap lint is relaxed here (bench tooling, not shipped
//! code).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use piccle_core::curve::Curve;
use piccle_core::model::{
    ContourEntry, Document, Echo, Filter, FilterType, Layer, Reverb, Source, SpatialEffect,
    ToneSource, VolumeContour, Waveform,
};
use piccle_render::{RenderPlan, Renderer};

const SAMPLE_RATE: u32 = 48_000;
const CHUNK_FRAMES: usize = 4096;
const MAX_BENCH_VOICES: usize = 128;
const CONTOUR_BOUNDARY_MS: u64 = 20;

fn held_hz(hz: f64) -> ContourEntry {
    ContourEntry { target: hz, hold_ms: 0, transition_ms: 0, transition_curve: Curve::Linear }
}

fn tone_layer(id: &str, duration_ms: u64, wave: Waveform, hz: f64) -> Layer {
    Layer {
        id: id.to_owned(),
        start_ms: 0,
        duration_ms,
        source: Source::Tone(ToneSource { wave, frequencies: vec![held_hz(hz)], offset_cents: 0 }),
        volume: VolumeContour::constant(1.0),
        balance: 0.0,
        filters: Vec::new(),
    }
}

fn boundary_contour_layer(id: &str, duration_ms: u64) -> Layer {
    Layer {
        id: id.to_owned(),
        start_ms: 0,
        duration_ms,
        source: Source::Tone(ToneSource {
            wave: Waveform::Sine,
            frequencies: vec![
                ContourEntry {
                    target: 440.0,
                    hold_ms: CONTOUR_BOUNDARY_MS,
                    transition_ms: 0,
                    transition_curve: Curve::Linear,
                },
                held_hz(880.0),
            ],
            offset_cents: 0,
        }),
        volume: VolumeContour::constant(1.0),
        balance: 0.0,
        filters: Vec::new(),
    }
}

fn document(layers: Vec<Layer>, reverb: Option<Reverb>, duration_ms: u64) -> Document {
    let spatial_effects =
        reverb.map_or_else(Vec::new, |reverb| vec![SpatialEffect::Reverb(reverb)]);
    document_with_spatial_effects(layers, spatial_effects, duration_ms)
}

fn document_with_spatial_effects(
    layers: Vec<Layer>,
    spatial_effects: Vec<SpatialEffect>,
    duration_ms: u64,
) -> Document {
    Document {
        name: Some("bench".to_owned()),
        description: None,
        duration_ms,
        master_volume_level: 1.0,
        spatial_effects,
        layers,
    }
}

fn bench_echo_delay_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("echo_delay_cost");
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    for delay_ms in [20_u64, 90, 200, 2_000] {
        let doc = document_with_spatial_effects(
            vec![tone_layer("a", 1_000, Waveform::Sine, 440.0)],
            vec![SpatialEffect::Echo(Echo {
                delay_ms,
                feedback: 0.6,
                wet_gain: 0.3,
                damp_hz: 4_000.0,
            })],
            1_000,
        );
        let plan = RenderPlan::compile_validated(&doc, SAMPLE_RATE);
        group.bench_function(format!("delay_{delay_ms}ms"), |b| bench_plan(b, &plan));
    }
    group.finish();
}

fn bench_plan(b: &mut criterion::Bencher, plan: &RenderPlan) {
    let mut buffer = vec![0.0f32; CHUNK_FRAMES * 2];
    b.iter_batched(
        || Renderer::new(plan),
        |mut renderer| {
            renderer.render_into(&mut buffer).expect("render cannot fail");
            criterion::black_box(&buffer);
        },
        BatchSize::SmallInput,
    );
}

struct FrameProbe<'plan> {
    plan: &'plan RenderPlan,
    pre_roll_frames: usize,
}

fn bench_single_frame(b: &mut criterion::Bencher, probe: &FrameProbe<'_>) {
    let mut output = [0.0_f32; 2];
    b.iter_batched(
        || {
            let mut renderer = Renderer::new(probe.plan);
            let mut pre_roll = vec![0.0_f32; probe.pre_roll_frames * 2];
            renderer.render_into(&mut pre_roll).expect("pre-roll cannot fail");
            renderer
        },
        |mut renderer| {
            renderer.render_into(&mut output).expect("single-frame render cannot fail");
            criterion::black_box(output);
        },
        BatchSize::SmallInput,
    );
}

fn bench_reverb_tail_flat_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("reverb_tail_flat_cost");
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    for tail_ms in [1u64, 10, 20, 220, 500] {
        let doc = document(
            vec![tone_layer("a", 1000, Waveform::Sine, 440.0)],
            Some(Reverb { amount: 0.6, tail_ms, soften_hz: 8000.0 }),
            1000,
        );
        let plan = RenderPlan::compile_validated(&doc, SAMPLE_RATE);
        group.bench_function(format!("tail_{tail_ms}ms"), |b| bench_plan(b, &plan));
    }
    group.finish();
}

fn bench_voice_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("voice_scaling");
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    for voices in [1usize, 8, 32, 64] {
        let layers = (0..voices)
            .map(|i| tone_layer(&format!("l{i}"), 1000, Waveform::Sine, 220.0 + 10.0 * i as f64))
            .collect();
        let plan = RenderPlan::compile_validated(&document(layers, None, 1000), SAMPLE_RATE);
        group.bench_function(format!("{voices}_voices"), |b| bench_plan(b, &plan));
    }
    group.finish();
}

fn bench_inactive_voice_gating(c: &mut Criterion) {
    let mut group = c.benchmark_group("inactive_voice_gating");
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    for (name, voices, start_ms) in [("none", 0usize, 0u64), ("64_inactive", 64, 500)] {
        let layers = (0..voices)
            .map(|i| {
                let mut layer = tone_layer(&format!("l{i}"), 500, Waveform::Sine, 220.0 + i as f64);
                layer.start_ms = start_ms;
                layer
            })
            .collect();
        let plan = RenderPlan::compile_validated(&document(layers, None, 1000), SAMPLE_RATE);
        group.bench_function(name, |b| bench_plan(b, &plan));
    }
    group.finish();
}

fn bench_contour_boundary_cost(c: &mut Criterion) {
    let layers = (0..MAX_BENCH_VOICES)
        .map(|index| boundary_contour_layer(&format!("l{index}"), 100))
        .collect();
    let plan = RenderPlan::compile_validated(&document(layers, None, 100), SAMPLE_RATE);
    let boundary_frame = (CONTOUR_BOUNDARY_MS * u64::from(SAMPLE_RATE) / 1000) as usize;
    let mut group = c.benchmark_group("contour_boundary_cost");
    group.throughput(Throughput::Elements(1));
    group.bench_function("128_voices_boundary_frame", |b| {
        bench_single_frame(b, &FrameProbe { plan: &plan, pre_roll_frames: boundary_frame });
    });
    group.bench_function("128_voices_steady_following_frame", |b| {
        bench_single_frame(b, &FrameProbe { plan: &plan, pre_roll_frames: boundary_frame + 1 });
    });
    group.finish();
}

fn bench_oscillator_harmonic_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("oscillator_harmonic_load");
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    for (name, wave, hz) in [
        ("sine_440hz", Waveform::Sine, 440.0),
        ("saw_440hz", Waveform::Saw, 440.0),
        ("saw_20hz_worst_case", Waveform::Saw, 20.0),
        ("square_20hz_worst_case", Waveform::Square, 20.0),
        ("triangle_20hz_worst_case", Waveform::Triangle, 20.0),
    ] {
        let plan = RenderPlan::compile_validated(
            &document(vec![tone_layer("a", 1000, wave, hz)], None, 1000),
            SAMPLE_RATE,
        );
        group.bench_function(name, |b| bench_plan(b, &plan));
    }
    group.finish();
}

fn bench_maximum_supported_workload(c: &mut Criterion) {
    let filters = (0..16)
        .map(|index| Filter {
            filter_type: match index % 3 {
                0 => FilterType::Lowpass,
                1 => FilterType::Highpass,
                _ => FilterType::Bandpass,
            },
            frequencies: vec![held_hz(500.0 + 500.0 * index as f64)],
            resonance: 0.5,
        })
        .collect::<Vec<_>>();
    let layers = (0..128)
        .map(|index| {
            let mut layer = tone_layer(&format!("l{index}"), 1000, Waveform::Saw, 20.0);
            layer.filters = filters.clone();
            layer
        })
        .collect();
    let plan = RenderPlan::compile_validated(
        &document(layers, Some(Reverb { amount: 0.5, tail_ms: 500, soften_hz: 4_000.0 }), 1000),
        SAMPLE_RATE,
    );
    let mut group = c.benchmark_group("maximum_supported_workload");
    group.sample_size(20);
    group.warm_up_time(std::time::Duration::from_secs(1));
    group.measurement_time(std::time::Duration::from_secs(5));
    group.throughput(Throughput::Elements(CHUNK_FRAMES as u64));
    group.bench_function("128_voices_16_filters_reverb", |b| bench_plan(b, &plan));
    group.finish();
}

criterion_group!(
    benches,
    bench_reverb_tail_flat_cost,
    bench_echo_delay_cost,
    bench_voice_scaling,
    bench_inactive_voice_gating,
    bench_contour_boundary_cost,
    bench_oscillator_harmonic_load,
    bench_maximum_supported_workload
);
criterion_main!(benches);
