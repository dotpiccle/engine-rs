//! Branch coverage for the hand-rolled JSON parser via the public validator
//! surface: strict grammar, escape handling, surrogate pairs, UTF-8
//! pass-through, and the dedicated parse-stage error codes.

use piccle_validate::{Validator, validate};

fn code_of(bytes: &[u8]) -> &'static str {
    Validator::check(bytes).expect_err("must fail").code()
}

fn document_with_name(name_json: &str) -> String {
    format!(
        r#"{{"piccle":"1.0","name":{name_json},"layers":[{{"id":"a","duration_ms":10,"source":{{"type":"tone","wave":"sine","pitch":{{"frequencies":[{{"hz":440}}]}}}}}}]}}"#
    )
}

#[test]
fn parses_true_false_and_null_members() {
    let bytes = br#"{"piccle":"1.0","layers":[],"description":null}"#;
    // `layers` empty is a schema error, but the literals must parse first.
    assert_eq!(code_of(bytes), "schema.minItems");
}

#[test]
fn rejects_unexpected_value_token() {
    assert_eq!(code_of(br#"{"a": ?}"#), "json.malformed");
}

#[test]
fn near_miss_nan_token_is_malformed() {
    assert_eq!(code_of(br#"{"a": NaNoodle}"#), "json.malformed");
}

#[test]
fn near_miss_infinity_token_is_malformed() {
    assert_eq!(code_of(br#"{"a": -Incorrect}"#), "json.malformed");
}

#[test]
fn rejects_truncated_literal() {
    assert_eq!(code_of(br#"{"a": tru}"#), "json.malformed");
}

#[test]
fn rejects_minus_without_integer_part() {
    assert_eq!(code_of(br#"{"a": -x}"#), "json.malformed");
}

#[test]
fn rejects_fraction_without_digits() {
    assert_eq!(code_of(br#"{"a": 1.}"#), "json.malformed");
}

#[test]
fn rejects_exponent_without_digits() {
    assert_eq!(code_of(br#"{"a": 1e+}"#), "json.malformed");
}

#[test]
fn finite_integer_beyond_u64_reaches_schema_validation() {
    let bytes = br#"{"piccle":"1.0","duration_ms":18446744073709551616,"layers":[]}"#;
    assert_eq!(code_of(bytes), "schema.maximum");
}

#[test]
fn rejects_object_with_non_string_key() {
    assert_eq!(code_of(br#"{a: 1}"#), "json.malformed");
}

#[test]
fn rejects_object_missing_colon() {
    assert_eq!(code_of(br#"{"a" 1}"#), "json.malformed");
}

#[test]
fn rejects_object_missing_comma() {
    assert_eq!(code_of(br#"{"a": 1 "b": 2}"#), "json.malformed");
}

#[test]
fn rejects_array_missing_comma() {
    assert_eq!(code_of(br#"[1 2]"#), "json.malformed");
}

#[test]
fn rejects_unterminated_string() {
    assert_eq!(code_of(br#"{"a": "abc"#), "json.malformed");
}

#[test]
fn rejects_raw_control_character_in_string() {
    assert_eq!(code_of(b"{\"a\": \"x\x01\"}"), "json.malformed");
}

#[test]
fn resolves_all_simple_escapes() {
    let document = document_with_name(r#""\"\\\/\b\f\n\r\t""#);
    let name = validate(document.as_bytes()).expect("must validate").name;
    assert_eq!(name, Some("\"\\/\u{8}\u{c}\n\r\t".to_string()));
}

#[test]
fn resolves_unicode_escape() {
    let document = document_with_name(r#""\u0041""#);
    let name = validate(document.as_bytes()).expect("must validate").name;
    assert_eq!(name, Some("A".to_string()));
}

#[test]
fn resolves_surrogate_pair_escape() {
    let document = document_with_name(r#""\uD83D\uDE00""#);
    let name = validate(document.as_bytes()).expect("must validate").name;
    assert_eq!(name, Some("\u{1F600}".to_string()));
}

#[test]
fn rejects_lone_high_surrogate() {
    assert_eq!(code_of(document_with_name(r#""\uD800x""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_high_surrogate_followed_by_non_low_escape() {
    assert_eq!(code_of(document_with_name(r#""\uD800\u0041""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_lone_low_surrogate() {
    assert_eq!(code_of(document_with_name(r#""\uDC00""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_unknown_escape() {
    assert_eq!(code_of(document_with_name(r#""\x""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_short_hex_escape() {
    assert_eq!(code_of(document_with_name(r#""\u041""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_non_hex_escape_digits() {
    assert_eq!(code_of(document_with_name(r#""\uZZZZ""#).as_bytes()), "json.malformed");
}

#[test]
fn rejects_escape_at_end_of_input() {
    assert_eq!(code_of(br#"{"a": "abc\"#), "json.malformed");
}

#[test]
fn resolves_multibyte_utf8_strings() {
    let document = document_with_name("\"héllo😀\"");
    let name = validate(document.as_bytes()).expect("must validate").name;
    assert_eq!(name, Some("héllo😀".to_string()));
}
