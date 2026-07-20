//! Hand-rolled structural validator mirroring piccle-spec/schemas/v1.json.
//!
//! The v1 format is frozen, so a direct structural walk replaces a generic
//! JSON Schema engine and produces the exact `schema.*` codes and paths of
//! piccle-spec/test-vectors/invalid-expectations.json. Checks run in
//! document order (structural → required → per-member) for determinism.

use piccle_core::error::{PiccleError, PiccleResult};
use serde_json::{Map, Value};

/// Canonical Piccle v1 JSON Schema URI (`$schema` const).
const SCHEMA_URI: &str = "https://spec.dotpiccle.com/schema/v1.json";
/// Largest permitted integer millisecond value (2^53 − 1).
const MAX_SAFE_INTEGER_MS: f64 = 9_007_199_254_740_991.0;
/// Largest permitted seed value (2^32 − 1).
const MAX_SEED: f64 = 4_294_967_295.0;
/// Largest permitted `offset_cents` magnitude.
const MAX_OFFSET_CENTS: f64 = 1_200.0;

/// Curve enum values (`$defs/curves`).
const CURVE_VALUES: [&str; 5] = ["linear", "exponential", "easeIn", "easeOut", "easeInOut"];

fn err(code: &'static str, path: String, msg: &str) -> PiccleError {
    PiccleError::schema(code, path, msg.to_owned())
}

fn type_err(path: &str, expected: &'static str) -> PiccleError {
    err("schema.type", path.to_owned(), expected)
}

fn as_object<'a>(value: &'a Value, path: &str) -> PiccleResult<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| type_err(path, "expected object"))
}

fn as_array<'a>(value: &'a Value, path: &str) -> PiccleResult<&'a Vec<Value>> {
    value.as_array().ok_or_else(|| type_err(path, "expected array"))
}

fn as_string<'a>(value: &'a Value, path: &str) -> PiccleResult<&'a str> {
    value.as_str().ok_or_else(|| type_err(path, "expected string"))
}

fn as_number(value: &Value, path: &str) -> PiccleResult<f64> {
    value.as_f64().ok_or_else(|| type_err(path, "expected number"))
}

/// Integer-typed JSON Schema fields accept any mathematically integral
/// number (`1`, `1.0`, `1e0`) per the valid fixture integer-number-forms.
fn as_integer(value: &Value, path: &str) -> PiccleResult<f64> {
    let number = as_number(value, path)?;
    if number.fract() != 0.0 {
        return Err(type_err(path, "expected integer"));
    }
    Ok(number)
}

fn check_enum(value: &Value, path: &str, allowed: &[&str]) -> PiccleResult<()> {
    let text = as_string(value, path)?;
    if !allowed.contains(&text) {
        return Err(err("schema.enum", path.to_owned(), "value not in enum"));
    }
    Ok(())
}

fn check_int_range(value: &Value, path: &str, minimum: f64, maximum: f64) -> PiccleResult<()> {
    let number = as_integer(value, path)?;
    if number < minimum {
        return Err(err("schema.minimum", path.to_owned(), "integer below minimum"));
    }
    if number > maximum {
        return Err(err("schema.maximum", path.to_owned(), "integer above maximum"));
    }
    Ok(())
}

fn check_num_range(value: &Value, path: &str, minimum: f64, maximum: f64) -> PiccleResult<()> {
    let number = as_number(value, path)?;
    if number < minimum {
        return Err(err("schema.minimum", path.to_owned(), "number below minimum"));
    }
    if number > maximum {
        return Err(err("schema.maximum", path.to_owned(), "number above maximum"));
    }
    Ok(())
}

fn check_num_exclusive_max(value: &Value, path: &str, maximum: f64) -> PiccleResult<()> {
    let number = as_number(value, path)?;
    if number >= maximum {
        return Err(err("schema.exclusiveMaximum", path.to_owned(), "number at or above maximum"));
    }
    Ok(())
}

/// Structural object checks: unknown members in document order, then
/// required members in schema-declared order.
fn check_object(
    obj: &Map<String, Value>,
    path: &str,
    allowed: &[&str],
    required: &[&str],
) -> PiccleResult<()> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(err(
                "schema.additionalProperties",
                format!("{path}.{key}"),
                "unknown member",
            ));
        }
    }
    for &name in required {
        if !obj.contains_key(name) {
            return Err(err(
                "schema.required",
                format!("{path}.{name}"),
                "required member missing",
            ));
        }
    }
    Ok(())
}

/// Validates a parsed document against the v1 schema.
///
/// # Errors
///
/// `SchemaInvalid` with the exact codes/paths of the spec's
/// invalid-expectations contract.
pub fn validate_document(value: &Value) -> PiccleResult<()> {
    let path = "$";
    let obj = as_object(value, path)?;
    check_object(
        obj,
        path,
        &[
            "$schema",
            "piccle",
            "name",
            "description",
            "duration_ms",
            "master_volume_level",
            "spatial_effects",
            "layers",
        ],
        &["piccle", "layers"],
    )?;
    for (key, val) in obj {
        let member_path = format!("$.{key}");
        match key.as_str() {
            "$schema" => {
                let text = as_string(val, &member_path)?;
                if text != SCHEMA_URI {
                    return Err(err("schema.const", member_path, "wrong schema URI"));
                }
            }
            "piccle" => check_enum(val, &member_path, &["1.0"])?,
            "name" | "description" => {
                let text = as_string(val, &member_path)?;
                if text.chars().count() < 1 {
                    return Err(err("schema.minLength", member_path, "string must not be empty"));
                }
            }
            "duration_ms" => check_int_range(val, &member_path, 1.0, MAX_SAFE_INTEGER_MS)?,
            "master_volume_level" => check_num_range(val, &member_path, 0.0, 1.0)?,
            "spatial_effects" => validate_spatial_effects(val, &member_path)?,
            "layers" => validate_layers(val, &member_path)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_layers(value: &Value, path: &str) -> PiccleResult<()> {
    let arr = as_array(value, path)?;
    if arr.is_empty() {
        return Err(err("schema.minItems", path.to_owned(), "at least one layer required"));
    }
    for (i, layer) in arr.iter().enumerate() {
        let layer_path = format!("{path}[{i}]");
        validate_layer(layer, &layer_path)?;
    }
    Ok(())
}

fn validate_layer(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    check_object(
        obj,
        path,
        &["id", "start_ms", "duration_ms", "source", "volume", "balance", "filters"],
        &["id", "duration_ms", "source"],
    )?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "id" => {
                let text = as_string(val, &member_path)?;
                if !is_valid_layer_id(text) {
                    return Err(err("schema.pattern", member_path, "invalid layer id"));
                }
            }
            "start_ms" => check_int_range(val, &member_path, 0.0, MAX_SAFE_INTEGER_MS)?,
            "duration_ms" => check_int_range(val, &member_path, 1.0, MAX_SAFE_INTEGER_MS)?,
            "source" => validate_source(val, &member_path)?,
            "volume" => validate_volume(val, &member_path)?,
            "balance" => check_num_range(val, &member_path, -1.0, 1.0)?,
            "filters" => validate_filters(val, &member_path)?,
            _ => {}
        }
    }
    Ok(())
}

/// `^[a-z][a-z0-9-]*$`
fn is_valid_layer_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next()
    else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_source(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    let type_path = format!("{path}.type");
    match obj.get("type") {
        None => {
            return Err(err("schema.required", type_path, "required member missing"));
        }
        Some(type_value) => {
            let type_text = as_string(type_value, &type_path)?;
            match type_text {
                "tone" => validate_tone_source(obj, path)?,
                "noise" => validate_noise_source(obj, path)?,
                _ => {
                    return Err(err("schema.enum", type_path, "source type must be tone or noise"));
                }
            }
        }
    }
    Ok(())
}

fn validate_tone_source(obj: &Map<String, Value>, path: &str) -> PiccleResult<()> {
    check_object(obj, path, &["type", "wave", "pitch"], &["type", "wave", "pitch"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "type" => {
                let text = as_string(val, &member_path)?;
                if text != "tone" {
                    return Err(err("schema.const", member_path, "expected tone"));
                }
            }
            "wave" => check_enum(val, &member_path, &["sine", "triangle", "square", "saw"])?,
            "pitch" => validate_pitch(val, &member_path)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_noise_source(obj: &Map<String, Value>, path: &str) -> PiccleResult<()> {
    check_object(obj, path, &["type", "character", "seed"], &["type", "character"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "type" => {
                let text = as_string(val, &member_path)?;
                if text != "noise" {
                    return Err(err("schema.const", member_path, "expected noise"));
                }
            }
            "character" => check_enum(val, &member_path, &["soft", "neutral", "sharp"])?,
            "seed" => check_int_range(val, &member_path, 0.0, MAX_SEED)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_pitch(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    check_object(obj, path, &["frequencies", "offset_cents"], &["frequencies"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "frequencies" => validate_contour(val, &member_path, ContourTarget::Hz)?,
            "offset_cents" => {
                check_int_range(val, &member_path, -MAX_OFFSET_CENTS, MAX_OFFSET_CENTS)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_filters(value: &Value, path: &str) -> PiccleResult<()> {
    let arr = as_array(value, path)?;
    for (i, filter) in arr.iter().enumerate() {
        let filter_path = format!("{path}[{i}]");
        validate_filter(filter, &filter_path)?;
    }
    Ok(())
}

fn validate_filter(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    check_object(obj, path, &["type", "frequencies", "resonance"], &["type", "frequencies"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "type" => check_enum(val, &member_path, &["lowpass", "highpass", "bandpass"])?,
            "frequencies" => validate_contour(val, &member_path, ContourTarget::Hz)?,
            "resonance" => check_num_range(val, &member_path, 0.0, 1.0)?,
            _ => {}
        }
    }
    Ok(())
}

/// The required numeric target of a contour entry (`hz` or `level`).
enum ContourTarget {
    Hz,
    Level,
}

fn validate_contour(value: &Value, path: &str, target: ContourTarget) -> PiccleResult<()> {
    let arr = as_array(value, path)?;
    if arr.is_empty() {
        return Err(err("schema.minItems", path.to_owned(), "at least one entry required"));
    }
    let (target_key, minimum, maximum) = match target {
        ContourTarget::Hz => ("hz", 20.0, 20_000.0),
        ContourTarget::Level => ("level", 0.0, 1.0),
    };
    for (i, entry) in arr.iter().enumerate() {
        let entry_path = format!("{path}[{i}]");
        let obj = as_object(entry, &entry_path)?;
        check_object(
            obj,
            &entry_path,
            &[target_key, "hold_ms", "transition_ms", "transition_curve"],
            &[target_key],
        )?;
        for (key, val) in obj {
            let member_path = format!("{entry_path}.{key}");
            match key.as_str() {
                "hz" | "level" => check_num_range(val, &member_path, minimum, maximum)?,
                "hold_ms" | "transition_ms" => {
                    check_int_range(val, &member_path, 0.0, MAX_SAFE_INTEGER_MS)?;
                }
                "transition_curve" => check_enum(val, &member_path, &CURVE_VALUES)?,
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_volume(value: &Value, path: &str) -> PiccleResult<()> {
    if value.is_number() {
        return check_num_range(value, path, 0.0, 1.0);
    }
    let obj = as_object(value, path)?;
    check_object(obj, path, &["fade_in", "fade_out", "levels"], &["levels"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "fade_in" | "fade_out" => validate_fade_stage(val, &member_path)?,
            "levels" => validate_contour(val, &member_path, ContourTarget::Level)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_fade_stage(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    check_object(obj, path, &["ms", "curve"], &["ms"])?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "ms" => check_int_range(val, &member_path, 0.0, MAX_SAFE_INTEGER_MS)?,
            "curve" => check_enum(val, &member_path, &CURVE_VALUES)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_spatial_effects(value: &Value, path: &str) -> PiccleResult<()> {
    let arr = as_array(value, path)?;
    for (i, effect) in arr.iter().enumerate() {
        validate_spatial_effect(effect, &format!("{path}[{i}]"))?;
    }
    Ok(())
}

fn validate_spatial_effect(value: &Value, path: &str) -> PiccleResult<()> {
    let obj = as_object(value, path)?;
    let type_path = format!("{path}.type");
    let Some(type_value) = obj.get("type")
    else {
        return Err(err("schema.required", type_path, "required member missing"));
    };
    let effect_type = as_string(type_value, &type_path)?;
    match effect_type {
        "reverb" => validate_reverb(obj, path),
        "echo" => validate_echo(obj, path),
        _ => Err(err("schema.enum", type_path, "spatial effect type must be reverb or echo")),
    }
}

fn validate_reverb(obj: &Map<String, Value>, path: &str) -> PiccleResult<()> {
    check_object(
        obj,
        path,
        &["type", "amount", "tail_ms", "soften_hz"],
        &["type", "amount", "tail_ms", "soften_hz"],
    )?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "type" => {
                let text = as_string(val, &member_path)?;
                if text != "reverb" {
                    return Err(err("schema.const", member_path, "expected reverb"));
                }
            }
            "amount" => check_num_range(val, &member_path, 0.0, 1.0)?,
            "tail_ms" => check_int_range(val, &member_path, 1.0, MAX_SAFE_INTEGER_MS)?,
            "soften_hz" => check_num_range(val, &member_path, 200.0, 12_000.0)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_echo(obj: &Map<String, Value>, path: &str) -> PiccleResult<()> {
    check_object(
        obj,
        path,
        &["type", "delay_ms", "feedback", "wet_gain", "damp_hz"],
        &["type", "delay_ms", "feedback", "wet_gain", "damp_hz"],
    )?;
    for (key, val) in obj {
        let member_path = format!("{path}.{key}");
        match key.as_str() {
            "type" => {
                let text = as_string(val, &member_path)?;
                if text != "echo" {
                    return Err(err("schema.const", member_path, "expected echo"));
                }
            }
            "delay_ms" => check_int_range(val, &member_path, 1.0, MAX_SAFE_INTEGER_MS)?,
            "feedback" => {
                check_num_range(val, &member_path, 0.0, 1.0)?;
                check_num_exclusive_max(val, &member_path, 1.0)?;
            }
            "wet_gain" => check_num_range(val, &member_path, 0.0, 1.0)?,
            "damp_hz" => check_num_range(val, &member_path, 200.0, 12_000.0)?,
            _ => {}
        }
    }
    Ok(())
}
