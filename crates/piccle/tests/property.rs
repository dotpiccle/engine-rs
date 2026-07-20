//! Property tests over randomly generated valid documents (AGENTS.md §8.6).
//!
//! Strategies generate only documents that pass validation (contour budgets
//! respected by construction), so `piccle::prepare` must succeed and the
//! render invariants must hold: finite output, exact frame count, clipped
//! range, determinism.

use proptest::prelude::*;
use serde_json::{Value, json};

fn curve() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&["linear", "exponential", "easeIn", "easeOut", "easeInOut"][..])
}

fn wave() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&["sine", "triangle", "square", "saw"][..])
}

fn character() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&["soft", "neutral", "sharp"][..])
}

fn filter_type() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&["lowpass", "highpass", "bandpass"][..])
}

/// A unit interval with millisecond-style granularity.
fn unit_interval() -> impl Strategy<Value = f64> {
    (0u32..=1000).prop_map(|x| f64::from(x) / 1000.0)
}

/// A pitch/filter frequency contour plus its timing budget
/// (`Σ(hold+transition)` over all but the last entry).
fn hz_contour(max_entries: usize) -> impl Strategy<Value = (Vec<Value>, u64)> {
    prop::collection::vec((200u32..=20_000, 0u64..=30, 0u64..=30, curve()), 1..=max_entries)
        .prop_map(|raw| {
            let budget = raw
                .iter()
                .take(raw.len().saturating_sub(1))
                .map(|(_, hold, transition, _)| hold + transition)
                .sum();
            let entries = raw
                .iter()
                .map(|(hz, hold, transition, curve)| {
                    json!({
                        "hz": f64::from(*hz) / 10.0,
                        "hold_ms": hold,
                        "transition_ms": transition,
                        "transition_curve": curve,
                    })
                })
                .collect();
            (entries, budget)
        })
}

/// A layer source plus its contour budget.
fn source() -> impl Strategy<Value = (Value, u64)> {
    prop_oneof![
        (wave(), hz_contour(4), -1200i32..=1200).prop_map(|(wave, (entries, budget), cents)| {
            (
                json!({
                    "type": "tone",
                    "wave": wave,
                    "pitch": {"frequencies": entries, "offset_cents": cents},
                }),
                budget,
            )
        }),
        (character(), any::<u32>()).prop_map(|(character, seed)| (
            json!({"type": "noise", "character": character, "seed": seed}),
            0
        )),
    ]
}

/// A layer volume (shorthand number or contour object) plus its budget.
fn volume() -> impl Strategy<Value = (Value, u64)> {
    prop_oneof![
        2 => unit_interval().prop_map(|level| (json!(level), 0u64)),
        1 => (0u64..=20, 0u64..=20, unit_interval(), curve(), curve()).prop_map(
            |(fade_in, fade_out, level, curve_in, curve_out)| {
                (
                    json!({
                        "fade_in": {"ms": fade_in, "curve": curve_in},
                        "fade_out": {"ms": fade_out, "curve": curve_out},
                        "levels": [{
                            "level": level,
                            "hold_ms": 0,
                            "transition_ms": 0,
                            "transition_curve": "linear",
                        }],
                    }),
                    // min(fade_out, duration) ≤ fade_out, so this bounds the true budget.
                    fade_in + fade_out,
                )
            },
        ),
    ]
}

/// A filter chain plus the largest per-filter contour budget.
fn filters() -> impl Strategy<Value = (Vec<Value>, u64)> {
    prop::collection::vec((filter_type(), hz_contour(3), unit_interval()), 0..=2).prop_map(|raw| {
        let budget = raw.iter().map(|(_, (_, budget), _)| *budget).max().unwrap_or(0);
        let filters = raw
                .into_iter()
                .map(|(filter_type, (entries, _), resonance)| {
                    json!({"type": filter_type, "frequencies": entries, "resonance": resonance})
                })
                .collect();
        (filters, budget)
    })
}

/// Everything a layer needs except its id; duration is derived so every
/// contour budget fits by construction.
fn layer_body() -> impl Strategy<Value = Value> {
    (
        0u64..=100,
        1u64..=60,
        source(),
        volume(),
        filters(),
        (-1000i32..=1000).prop_map(|x| f64::from(x) / 1000.0),
    )
        .prop_map(|(start, extra, (source, b1), (volume, b2), (filters, b3), balance)| {
            let duration = b1.max(b2).max(b3) + extra;
            json!({
                "start_ms": start,
                "duration_ms": duration,
                "source": source,
                "volume": volume,
                "balance": balance,
                "filters": filters,
            })
        })
}

fn spatial_effect() -> impl Strategy<Value = Value> {
    let reverb = (unit_interval(), 1u64..=200, 200u32..=12_000).prop_map(
        |(amount, tail, soften)| {
            json!({"type": "reverb", "amount": amount, "tail_ms": tail, "soften_hz": soften})
        },
    );
    let echo = (1u64..=50, 0u32..=700, unit_interval(), 200u32..=12_000).prop_map(
        |(delay, feedback, wet_gain, damp)| {
            json!({
                "type": "echo",
                "delay_ms": delay,
                "feedback": f64::from(feedback) / 1000.0,
                "wet_gain": wet_gain,
                "damp_hz": damp,
            })
        },
    );
    prop_oneof![reverb, echo]
}

fn spatial_effects() -> impl Strategy<Value = Vec<Value>> {
    prop_oneof![Just(Vec::new()), prop::collection::vec(spatial_effect(), 1..=3)]
}

/// A serialized, schema-valid Piccle document with 1–3 layers.
fn document_bytes() -> impl Strategy<Value = Vec<u8>> {
    (
        prop::collection::vec(layer_body(), 1..=3),
        spatial_effects(),
        prop::option::of(unit_interval()),
    )
        .prop_map(|(bodies, spatial_effects, master)| {
            let layers: Vec<Value> = bodies
                .into_iter()
                .enumerate()
                .map(|(i, body)| {
                    let mut layer = body;
                    layer["id"] = json!(format!("l{i}"));
                    layer
                })
                .collect();
            let mut document = json!({"piccle": "1.0", "name": "proptest", "layers": layers});
            if !spatial_effects.is_empty() {
                document["spatial_effects"] = json!(spatial_effects);
            }
            if let Some(master) = master {
                document["master_volume_level"] = json!(master);
            }
            serde_json::to_vec(&document).expect("serialization cannot fail")
        })
}

fn render(bytes: &[u8]) -> (piccle::RenderPlan, Vec<f32>) {
    let plan = piccle::prepare(bytes).expect("generated documents must prepare");
    let samples = piccle::Renderer::render_to_vec(&plan).expect("render must succeed");
    (plan, samples)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn generated_documents_prepare_successfully(bytes in document_bytes()) {
        prop_assert!(piccle::prepare(&bytes).is_ok());
    }

    #[test]
    fn rendered_frame_count_is_exact(bytes in document_bytes()) {
        let (plan, samples) = render(&bytes);
        prop_assert_eq!(samples.len(), plan.output_frames() as usize * 2);
    }

    #[test]
    fn rendered_samples_are_finite(bytes in document_bytes()) {
        let (_, samples) = render(&bytes);
        prop_assert!(samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn rendered_samples_stay_within_clip_bounds(bytes in document_bytes()) {
        let (_, samples) = render(&bytes);
        prop_assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn rendering_is_deterministic(bytes in document_bytes()) {
        let (plan, first) = render(&bytes);
        let second = piccle::Renderer::render_to_vec(&plan).expect("render must succeed");
        prop_assert_eq!(first, second);
    }

    #[test]
    fn validator_never_panics_and_errors_are_shaped(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        if let Err(error) = piccle_validate::Validator::check(&bytes) {
            prop_assert!(!error.code().is_empty());
        }
    }
}
