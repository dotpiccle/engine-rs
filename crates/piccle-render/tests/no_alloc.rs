//! Render-loop allocation discipline: a counting global allocator asserts
//! that `render_into` performs zero heap allocations.
//!
//! Spec: piccle-spec/docs/13-implementer-notes.md §Render-loop discipline.

use alloc_counter::{AllocCounterSystem, count_alloc};
use piccle_core::curve::Curve;
use piccle_core::model::{
    ContourEntry, Document, Echo, Filter, Layer, Reverb, Source, SpatialEffect, ToneSource,
    VolumeContour, Waveform,
};
use piccle_render::plan::RenderPlan;
use piccle_render::renderer::Renderer;

#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

fn stress_document() -> Document {
    // Multiple layers, a moving pitch contour, serial filters, and parallel
    // reverb/echo: representative simultaneous render-path machinery.
    let layer = |id: &str, seed_offset: i32| Layer {
        id: id.to_string(),
        start_ms: 0,
        duration_ms: 100,
        source: Source::Tone(ToneSource {
            wave: Waveform::Saw,
            frequencies: vec![
                ContourEntry {
                    target: 220.0,
                    hold_ms: 10,
                    transition_ms: 40,
                    transition_curve: Curve::Exponential,
                },
                ContourEntry {
                    target: 880.0,
                    hold_ms: 0,
                    transition_ms: 0,
                    transition_curve: Curve::Linear,
                },
            ],
            offset_cents: seed_offset,
        }),
        volume: VolumeContour::constant(0.5),
        balance: 0.0,
        filters: vec![Filter {
            filter_type: piccle_core::model::FilterType::Lowpass,
            frequencies: vec![
                ContourEntry {
                    target: 200.0,
                    hold_ms: 0,
                    transition_ms: 80,
                    transition_curve: Curve::Linear,
                },
                ContourEntry {
                    target: 9_000.0,
                    hold_ms: 0,
                    transition_ms: 0,
                    transition_curve: Curve::Linear,
                },
            ],
            resonance: 0.4,
        }],
    };
    Document {
        name: None,
        description: None,
        duration_ms: 100,
        master_volume_level: 1.0,
        spatial_effects: vec![
            SpatialEffect::Reverb(Reverb { amount: 0.3, tail_ms: 220, soften_hz: 4_000.0 }),
            SpatialEffect::Echo(Echo {
                delay_ms: 90,
                feedback: 0.3,
                wet_gain: 0.2,
                damp_hz: 4_000.0,
            }),
        ],
        layers: vec![layer("a", 0), layer("b", 12), layer("c", -7)],
    }
}

#[test]
fn render_into_allocates_nothing() {
    let plan = RenderPlan::compile_validated(&stress_document(), 48_000);
    let mut renderer = Renderer::new(&plan);
    let mut output = vec![0.0_f32; 2 * plan.output_frames() as usize];

    let ((allocations, reallocations, deallocations), ()) = count_alloc(|| {
        renderer.render_into(&mut output).expect("render");
    });
    assert_eq!((allocations, reallocations, deallocations), (0, 0, 0));
}

#[test]
fn non_finite_error_path_allocates_nothing() {
    let mut document = stress_document();
    document.master_volume_level = f64::NAN;
    let plan = RenderPlan::compile_validated(&document, 48_000);
    let mut renderer = Renderer::new(&plan);
    let mut output = [0.0_f32; 2];

    let ((allocations, reallocations, deallocations), result) =
        count_alloc(|| renderer.render_into(&mut output));
    assert!(matches!(
        (allocations, reallocations, deallocations, result),
        (0, 0, 0, Err(piccle_core::error::PiccleError::Internal(_)))
    ));
}

/// Spec `docs/15-engine-build-guide.md` §Engine conformance verification
/// step 9: no allocation at maximum supported voices and filters.
#[test]
fn render_into_allocates_nothing_at_max_voices_and_filters() {
    let filters = (0..16)
        .map(|i| Filter {
            filter_type: match i % 3 {
                0 => piccle_core::model::FilterType::Lowpass,
                1 => piccle_core::model::FilterType::Highpass,
                _ => piccle_core::model::FilterType::Bandpass,
            },
            frequencies: vec![ContourEntry {
                target: 1_000.0 + 100.0 * i as f64,
                hold_ms: 0,
                transition_ms: 0,
                transition_curve: Curve::Linear,
            }],
            resonance: 0.5,
        })
        .collect::<Vec<_>>();
    let layers = (0..128)
        .map(|i| Layer {
            id: format!("l{i}"),
            start_ms: 0,
            duration_ms: 10,
            source: Source::Tone(ToneSource {
                wave: Waveform::Saw,
                frequencies: vec![
                    ContourEntry {
                        target: 220.0,
                        hold_ms: 0,
                        transition_ms: 5,
                        transition_curve: Curve::Linear,
                    },
                    ContourEntry {
                        target: 880.0,
                        hold_ms: 0,
                        transition_ms: 0,
                        transition_curve: Curve::Linear,
                    },
                ],
                offset_cents: 0,
            }),
            volume: VolumeContour::constant(0.1),
            balance: 0.0,
            filters: filters.clone(),
        })
        .collect();
    let document = Document {
        name: None,
        description: None,
        duration_ms: 10,
        master_volume_level: 1.0,
        spatial_effects: vec![SpatialEffect::Reverb(Reverb {
            amount: 0.5,
            tail_ms: 1,
            soften_hz: 12_000.0,
        })],
        layers,
    };
    let plan = RenderPlan::compile_validated(&document, 48_000);
    let mut renderer = Renderer::new(&plan);
    let mut output = vec![0.0_f32; 2 * plan.output_frames() as usize];

    let ((allocations, reallocations, deallocations), ()) = count_alloc(|| {
        renderer.render_into(&mut output).expect("render");
    });
    assert_eq!((allocations, reallocations, deallocations), (0, 0, 0));
}

/// Spec step 9: no allocation with a long reverb tail. 500 ms already
/// saturates every FDN delay cap (~1 570 samples total — identical state to
/// the 60 s engine maximum), while keeping prepare-time RT60 calibration
/// fast enough for a debug-build test.
#[test]
fn render_into_allocates_nothing_at_max_tail() {
    let document = Document {
        name: None,
        description: None,
        duration_ms: 10,
        master_volume_level: 1.0,
        spatial_effects: vec![SpatialEffect::Reverb(Reverb {
            amount: 0.5,
            tail_ms: 500,
            soften_hz: 4_000.0,
        })],
        layers: vec![Layer {
            id: "a".to_string(),
            start_ms: 0,
            duration_ms: 10,
            source: Source::Tone(ToneSource {
                wave: Waveform::Sine,
                frequencies: vec![ContourEntry {
                    target: 440.0,
                    hold_ms: 0,
                    transition_ms: 0,
                    transition_curve: Curve::Linear,
                }],
                offset_cents: 0,
            }),
            volume: VolumeContour::constant(1.0),
            balance: 0.0,
            filters: Vec::new(),
        }],
    };
    let plan = RenderPlan::compile_validated(&document, 48_000);
    let mut renderer = Renderer::new(&plan);
    let mut output = vec![0.0_f32; 2 * 48_000];

    let ((allocations, reallocations, deallocations), ()) = count_alloc(|| {
        renderer.render_into(&mut output).expect("render");
    });
    assert_eq!((allocations, reallocations, deallocations), (0, 0, 0));
}
