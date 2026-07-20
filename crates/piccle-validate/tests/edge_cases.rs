//! Edge coverage for schema member checks, resolution of float-form integer
//! fields, and the `Validator` instance API.

use piccle_core::error::Stage;
use piccle_validate::{Validator, validate};

fn doc_with(extra_root: &str, layer_extra: &str) -> String {
    format!(
        r#"{{"piccle":"1.0"{extra_root},"layers":[{{"id":"a","duration_ms":10{layer_extra},"source":{{"type":"tone","wave":"sine","pitch":{{"frequencies":[{{"hz":440}}]}}}}}}]}}"#
    )
}

#[test]
fn fractional_value_in_an_integer_field_is_a_type_error() {
    let bytes = doc_with(r#","duration_ms":10.5"#, "");
    let error = Validator::check(bytes.as_bytes()).expect_err("must fail");
    assert_eq!(error.code(), "schema.type");
}

#[test]
fn float_form_offset_cents_resolves_to_the_same_integer() {
    let bytes = doc_with("", r#","volume":1.0,"balance":0.0"#).replace(
        r#""frequencies":[{"hz":440}]}"#,
        r#""frequencies":[{"hz":440}],"offset_cents":-5.0}"#,
    );
    let document = validate(bytes.as_bytes()).expect("must validate");
    let piccle_core::model::Source::Tone(tone) = &document.layers[0].source
    else {
        panic!("tone source expected");
    };
    assert_eq!(tone.offset_cents, -5);
}

#[test]
fn layer_id_must_match_the_schema_pattern() {
    let bytes = doc_with("", "").replace(r#""id":"a""#, r#""id":"A""#);
    let error = Validator::check(bytes.as_bytes()).expect_err("must fail");
    assert_eq!(error.code(), "schema.pattern");
}

#[test]
fn empty_layer_id_is_a_pattern_error_not_a_min_length_error() {
    let bytes = doc_with("", "").replace(r#""id":"a""#, r#""id":"""#);
    let error = Validator::check(bytes.as_bytes()).expect_err("must fail");
    assert_eq!(error.code(), "schema.pattern");
}

#[test]
fn validator_instance_constructs_and_static_api_matches_free_function() {
    let bytes = doc_with("", "");
    let _validator = Validator::new();
    assert_eq!(Validator::check(bytes.as_bytes()).is_ok(), validate(bytes.as_bytes()).is_ok());
}

#[test]
fn validator_error_stage_is_populated() {
    let error = Validator::check(br#"{"piccle":"1.0","layers":[]}"#).expect_err("must fail");
    assert_eq!(error.stage(), Stage::Schema);
}
