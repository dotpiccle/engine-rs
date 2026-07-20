//! Document-level schedule conformance against
//! piccle-spec/test-vectors/behavior/render-cases.json.

use std::path::{Path, PathBuf};

use piccle_render::plan::RenderPlan;

fn spec_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PICCLE_SPEC_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../piccle-spec")
}

#[derive(Debug, PartialEq)]
struct LayerSchedule {
    id: String,
    declared_start_frame: u64,
    declared_end_frame: u64,
    active_end_frame: u64,
    fade_start_frame: u64,
    fade_frames: u64,
}

#[derive(Debug, PartialEq)]
struct Schedule {
    document_duration_ms: u64,
    dry_end_frame: u64,
    output_end_frame: u64,
    tail_frames: u64,
    terminal_window_frames: u64,
    layers: Vec<LayerSchedule>,
}

fn actual_schedule(spec: &Path, case: &serde_json::Value) -> Schedule {
    let document_path = case["document"].as_str().expect("document path");
    let bytes = std::fs::read(spec.join("test-vectors/behavior").join(document_path))
        .expect("read case document");
    let sample_rate = case["sample_rate"].as_u64().expect("sample rate") as u32;
    let document = piccle_validate::Validator::validate(&bytes).expect("valid document");
    let plan = RenderPlan::compile_validated(&document, sample_rate);

    let layers = document
        .layers
        .iter()
        .zip(plan.layers())
        .map(|(layer, plan)| LayerSchedule {
            id: layer.id.clone(),
            declared_start_frame: plan.start_frame(),
            declared_end_frame: plan.declared_end_frame(),
            active_end_frame: plan.active_end_frame(),
            fade_start_frame: plan.envelope().fade_start_frame(),
            fade_frames: plan.envelope().fade_frames(),
        })
        .collect();

    Schedule {
        document_duration_ms: document.duration_ms,
        dry_end_frame: plan.dry_end_frame(),
        output_end_frame: plan.output_frames(),
        tail_frames: plan.reverb().map_or(0, |reverb| reverb.tail_frames()),
        terminal_window_frames: plan.reverb().map_or(0, |reverb| reverb.window_frames()),
        layers,
    }
}

fn expected_schedule(case: &serde_json::Value) -> Schedule {
    let expected = &case["expected"];
    let layers = expected["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .map(|layer| LayerSchedule {
            id: layer["id"].as_str().expect("id").to_string(),
            declared_start_frame: layer["declared_start_frame"].as_u64().expect("start"),
            declared_end_frame: layer["declared_end_frame"].as_u64().expect("end"),
            active_end_frame: layer["active_end_frame"].as_u64().expect("active end"),
            fade_start_frame: layer["fade_start_frame"].as_u64().expect("fade start"),
            fade_frames: layer["fade_frames"].as_u64().expect("fade frames"),
        })
        .collect();
    Schedule {
        document_duration_ms: expected["document_duration_ms"].as_u64().expect("duration"),
        dry_end_frame: expected["dry_end_frame"].as_u64().expect("dry end"),
        output_end_frame: expected["output_end_frame"].as_u64().expect("output end"),
        tail_frames: expected["tail_frames"].as_u64().expect("tail"),
        terminal_window_frames: expected["terminal_window_frames"].as_u64().expect("window"),
        layers,
    }
}

fn case_by_id<'a>(cases: &'a [serde_json::Value], id: &str) -> &'a serde_json::Value {
    cases.iter().find(|case| case["id"].as_str() == Some(id)).expect("case present")
}

fn load_cases() -> (PathBuf, Vec<serde_json::Value>) {
    let spec = spec_dir();
    let text = std::fs::read_to_string(spec.join("test-vectors/behavior/render-cases.json"))
        .expect("read render-cases.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse render-cases");
    let cases = value["cases"].as_array().expect("cases array").clone();
    (spec, cases)
}

#[test]
fn computed_duration_and_default_fade_schedule_matches() {
    let (spec, cases) = load_cases();
    let case = case_by_id(&cases, "computed-duration-and-default-fade");
    assert_eq!(actual_schedule(&spec, case), expected_schedule(case));
}

#[test]
fn hard_root_truncation_schedule_matches() {
    let (spec, cases) = load_cases();
    let case = case_by_id(&cases, "hard-root-truncation-does-not-move-fade");
    assert_eq!(actual_schedule(&spec, case), expected_schedule(case));
}

#[test]
fn simultaneous_half_open_boundary_schedule_matches() {
    let (spec, cases) = load_cases();
    let case = case_by_id(&cases, "simultaneous-half-open-boundary");
    assert_eq!(actual_schedule(&spec, case), expected_schedule(case));
}

#[test]
fn nonadditive_reverb_tail_boundary_schedule_matches() {
    let (spec, cases) = load_cases();
    let case = case_by_id(&cases, "nonadditive-reverb-tail-boundary");
    assert_eq!(actual_schedule(&spec, case), expected_schedule(case));
}
