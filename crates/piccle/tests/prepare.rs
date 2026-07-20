//! Engine-limit classification and the `piccle::prepare` security boundary.

use piccle::{PiccleError, Stage};

fn minimal_document(extra_root: &str, layer: &str) -> Vec<u8> {
    format!(r#"{{"piccle":"1.0"{extra_root},"layers":[{layer}]}}"#).into_bytes()
}

fn tone_layer(id: &str, duration_ms: u64, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","duration_ms":{duration_ms}{extra},"source":{{"type":"tone","wave":"sine","pitch":{{"frequencies":[{{"hz":440}}]}}}}}}"#
    )
}

#[test]
fn prepare_accepts_a_minimal_document() {
    let bytes = minimal_document("", &tone_layer("a", 10, ""));
    assert!(piccle::prepare(&bytes).is_ok());
}

#[test]
fn prepare_rejects_malformed_json_at_the_parse_stage() {
    let error = piccle::prepare(b"{not json").expect_err("must fail");
    assert_eq!(error.stage(), Stage::Parse);
}

#[test]
fn duration_above_the_engine_limit_is_unsupported_not_invalid() {
    let bytes = minimal_document("", &tone_layer("a", 600_001, ""));
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert_eq!(error.stage(), Stage::Unsupported);
}

#[test]
fn duration_at_the_engine_limit_is_supported() {
    let bytes = minimal_document("", &tone_layer("a", 600_000, ""));
    assert!(piccle::prepare(&bytes).is_ok());
}

#[test]
fn too_many_layers_is_unsupported() {
    let layers =
        (0..129).map(|i| tone_layer(&format!("layer-{i}"), 1, "")).collect::<Vec<_>>().join(",");
    let bytes = minimal_document("", &layers);
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_layers", .. }));
}

#[test]
fn too_many_filters_in_one_layer_is_unsupported() {
    let filter = r#"{"type":"lowpass","frequencies":[{"hz":1000}]}"#;
    let filters = (0..17).map(|_| filter).collect::<Vec<_>>().join(",");
    let layer = tone_layer("a", 10, &format!(r#","filters":[{filters}]"#));
    let bytes = minimal_document("", &layer);
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_filters_per_layer", .. }));
}

#[test]
fn reverb_tail_above_the_engine_limit_is_unsupported() {
    let bytes = minimal_document(
        r#","spatial_effects":[{"type":"reverb","amount":0.2,"tail_ms":60001,"soften_hz":4000}]"#,
        &tone_layer("a", 10, ""),
    );
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_tail_ms", .. }));
}

#[test]
fn high_rate_reverb_above_the_preparation_frame_budget_is_unsupported() {
    let bytes = minimal_document(
        r#","spatial_effects":[{"type":"reverb","amount":0.2,"tail_ms":15001,"soften_hz":4000}]"#,
        &tone_layer("a", 10, ""),
    );
    let error = piccle::prepare_with_rate(&bytes, 192_000).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_reverb_tail_frames", .. }));
}

#[test]
fn zero_amount_reverb_does_not_consume_the_wet_preparation_budget() {
    let bytes = minimal_document(
        r#","spatial_effects":[{"type":"reverb","amount":0,"tail_ms":60000,"soften_hz":4000}]"#,
        &tone_layer("a", 10, ""),
    );
    assert!(piccle::prepare_with_rate(&bytes, 192_000).is_ok());
}

#[test]
fn echo_delay_above_the_engine_limit_is_unsupported() {
    let bytes = minimal_document(
        r#","spatial_effects":[{"type":"echo","delay_ms":2001,"feedback":0,"wet_gain":0.3,"damp_hz":4000}]"#,
        &tone_layer("a", 10, ""),
    );
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_echo_delay_ms", .. }));
}

#[test]
fn echo_effective_tail_above_the_engine_limit_is_unsupported() {
    let bytes = minimal_document(
        r#","spatial_effects":[{"type":"echo","delay_ms":1000,"feedback":0.99,"wet_gain":0.3,"damp_hz":4000}]"#,
        &tone_layer("a", 10, ""),
    );
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_tail_ms", .. }));
}

#[test]
fn contour_above_the_engine_limit_is_unsupported() {
    let entries = (0..1025).map(|_| r#"{"hz":440}"#).collect::<Vec<_>>().join(",");
    let layer = format!(
        r#"{{"id":"a","duration_ms":10,"source":{{"type":"tone","wave":"sine","pitch":{{"frequencies":[{entries}]}}}}}}"#
    );
    let bytes = minimal_document("", &layer);
    let error = piccle::prepare(&bytes).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_contour_entries", .. }));
}

#[test]
fn sample_rate_below_8000_is_rejected_before_validation() {
    let bytes = minimal_document("", &tone_layer("a", 10, ""));
    let error = piccle::prepare_with_rate(&bytes, 7_999).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "min_sample_rate", .. }));
}

#[test]
fn sample_rate_above_the_engine_limit_is_rejected_before_validation() {
    let bytes = minimal_document("", &tone_layer("a", 10, ""));
    let error = piccle::prepare_with_rate(&bytes, 192_001).expect_err("must fail");
    assert!(matches!(error, PiccleError::Unsupported { limit: "max_sample_rate", .. }));
}

#[test]
fn noncanonical_rate_renders_the_absolute_schedule() {
    let bytes = minimal_document("", &tone_layer("a", 100, ""));
    let plan = piccle::prepare_with_rate(&bytes, 44_100).expect("valid");
    assert_eq!(plan.output_frames(), 4_410);
}
