//! Default resolution: builds the typed `piccle-core` document model from a
//! validated JSON value with every schema default materialized.
//!
//! Runs only after schema and semantic stages pass, so every extraction is
//! guaranteed to succeed; any miss is an engine bug reported as `Internal`.

use piccle_core::curve::Curve;
use piccle_core::error::{PiccleError, PiccleResult};
use piccle_core::model::{
    ContourEntry, Document, FadeStage, Filter, FilterType, Layer, NoiseCharacter, NoiseSource,
    Reverb, Source, ToneSource, VolumeContour, Waveform,
};
use serde_json::{Map, Value};

fn internal(what: &str) -> PiccleError {
    PiccleError::internal(format!("resolution reached invalid input: {what}"))
}

/// Extracts a non-negative integer field (accepting `1`, `1.0`, `1e0`
/// forms), applying a default when absent.
pub(crate) fn integer_field(
    obj: &Map<String, Value>,
    key: &str,
    default: u64,
) -> PiccleResult<u64> {
    let Some(value) = obj.get(key)
    else {
        return Ok(default);
    };
    if let Some(unsigned) = value.as_u64() {
        return Ok(unsigned);
    }
    let number = value.as_f64().ok_or_else(|| internal("integer field is not numeric"))?;
    if number.fract() != 0.0 || !(0.0..=9_007_199_254_740_991.0).contains(&number) {
        return Err(internal("integer field out of range"));
    }
    Ok(number as u64)
}

/// Extracts a signed integer field (e.g. `offset_cents` ∈ [-1200, 1200]).
fn signed_integer_field(obj: &Map<String, Value>, key: &str, default: i64) -> PiccleResult<i64> {
    let Some(value) = obj.get(key)
    else {
        return Ok(default);
    };
    if let Some(signed) = value.as_i64() {
        return Ok(signed);
    }
    let number = value.as_f64().ok_or_else(|| internal("integer field is not numeric"))?;
    if number.fract() != 0.0
        || !(-9_007_199_254_740_991.0..=9_007_199_254_740_991.0).contains(&number)
    {
        return Err(internal("integer field out of range"));
    }
    Ok(number as i64)
}

fn number_field(obj: &Map<String, Value>, key: &str, default: f64) -> PiccleResult<f64> {
    match obj.get(key) {
        None => Ok(default),
        Some(value) => value.as_f64().ok_or_else(|| internal("number field is not numeric")),
    }
}

fn string_field<'a>(obj: &'a Map<String, Value>, key: &str) -> PiccleResult<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| internal("string field missing or not a string"))
}

fn curve_field(obj: &Map<String, Value>, key: &str, default: Curve) -> PiccleResult<Curve> {
    match obj.get(key) {
        None => Ok(default),
        Some(value) => {
            let name = value.as_str().ok_or_else(|| internal("curve is not a string"))?;
            Curve::from_schema_name(name).ok_or_else(|| internal("unknown curve name"))
        }
    }
}

/// Resolves a validated JSON value into the typed document model.
///
/// # Errors
///
/// `Internal` when reached without a schema+semantic-valid document.
pub fn resolve_document(value: &Value) -> PiccleResult<Document> {
    let root = value.as_object().ok_or_else(|| internal("root not object"))?;

    let name = match root.get("name") {
        Some(v) => Some(string_field_root(v, "name")?.to_owned()),
        None => None,
    };
    let description = match root.get("description") {
        Some(v) => Some(string_field_root(v, "description")?.to_owned()),
        None => None,
    };

    let layer_values =
        root.get("layers").and_then(Value::as_array).ok_or_else(|| internal("layers missing"))?;
    let mut layers = Vec::with_capacity(layer_values.len());
    let mut max_layer_end_ms = 0u64;
    for layer_value in layer_values {
        let layer = resolve_layer(layer_value)?;
        max_layer_end_ms = max_layer_end_ms.max(layer.start_ms + layer.duration_ms);
        layers.push(layer);
    }

    let duration_ms = match root.get("duration_ms") {
        Some(_) => integer_field(root, "duration_ms", 0)?,
        None => max_layer_end_ms,
    };

    let master_volume_level = number_field(root, "master_volume_level", 1.0)?;

    let reverb = match root.get("reverb") {
        Some(v) => Some(resolve_reverb(v)?),
        None => None,
    };

    Ok(Document { name, description, duration_ms, master_volume_level, reverb, layers })
}

fn string_field_root<'a>(value: &'a Value, what: &str) -> PiccleResult<&'a str> {
    value.as_str().ok_or_else(|| internal(what))
}

fn resolve_reverb(value: &Value) -> PiccleResult<Reverb> {
    let obj = value.as_object().ok_or_else(|| internal("reverb not object"))?;
    Ok(Reverb {
        amount: number_field(obj, "amount", 0.0)?,
        tail_ms: integer_field(obj, "tail_ms", 0)?,
        soften_hz: number_field(obj, "soften_hz", 0.0)?,
    })
}

fn resolve_layer(value: &Value) -> PiccleResult<Layer> {
    let obj = value.as_object().ok_or_else(|| internal("layer not object"))?;
    let id = string_field(obj, "id")?.to_owned();
    let start_ms = integer_field(obj, "start_ms", 0)?;
    let duration_ms = integer_field(obj, "duration_ms", 0)?;
    let source = resolve_source(obj.get("source").ok_or_else(|| internal("source missing"))?)?;
    let volume = match obj.get("volume") {
        None => VolumeContour::constant(1.0),
        Some(v) => resolve_volume(v)?,
    };
    let balance = number_field(obj, "balance", 0.0)?;
    let filter_values = obj.get("filters").and_then(Value::as_array);
    let mut filters = Vec::with_capacity(filter_values.map_or(0, Vec::len));
    if let Some(filter_values) = filter_values {
        for filter_value in filter_values {
            filters.push(resolve_filter(filter_value)?);
        }
    }
    Ok(Layer { id, start_ms, duration_ms, source, volume, balance, filters })
}

fn resolve_source(value: &Value) -> PiccleResult<Source> {
    let obj = value.as_object().ok_or_else(|| internal("source not object"))?;
    match string_field(obj, "type")? {
        "tone" => {
            let wave = match string_field(obj, "wave")? {
                "sine" => Waveform::Sine,
                "triangle" => Waveform::Triangle,
                "square" => Waveform::Square,
                "saw" => Waveform::Saw,
                _ => return Err(internal("unknown wave")),
            };
            let pitch = obj
                .get("pitch")
                .and_then(Value::as_object)
                .ok_or_else(|| internal("pitch missing"))?;
            let frequencies = resolve_contour(
                pitch.get("frequencies").ok_or_else(|| internal("frequencies missing"))?,
                "hz",
            )?;
            let offset_cents = signed_integer_field(pitch, "offset_cents", 0)? as i32;
            Ok(Source::Tone(ToneSource { wave, frequencies, offset_cents }))
        }
        "noise" => {
            let character = match string_field(obj, "character")? {
                "soft" => NoiseCharacter::Soft,
                "neutral" => NoiseCharacter::Neutral,
                "sharp" => NoiseCharacter::Sharp,
                _ => return Err(internal("unknown noise character")),
            };
            let seed = integer_field(obj, "seed", 0)? as u32;
            Ok(Source::Noise(NoiseSource { character, seed }))
        }
        _ => Err(internal("unknown source type")),
    }
}

fn resolve_filter(value: &Value) -> PiccleResult<Filter> {
    let obj = value.as_object().ok_or_else(|| internal("filter not object"))?;
    let filter_type = match string_field(obj, "type")? {
        "lowpass" => FilterType::Lowpass,
        "highpass" => FilterType::Highpass,
        "bandpass" => FilterType::Bandpass,
        _ => return Err(internal("unknown filter type")),
    };
    let frequencies = resolve_contour(
        obj.get("frequencies").ok_or_else(|| internal("frequencies missing"))?,
        "hz",
    )?;
    let resonance = number_field(obj, "resonance", 0.0)?;
    Ok(Filter { filter_type, frequencies, resonance })
}

fn resolve_volume(value: &Value) -> PiccleResult<VolumeContour> {
    if let Some(level) = value.as_f64() {
        // Number shorthand: constant level with the default 5 ms linear
        // fade-out (piccle-spec/docs/05-layer-volume.md).
        return Ok(VolumeContour::constant(level));
    }
    let obj = value.as_object().ok_or_else(|| internal("volume not object"))?;
    let fade_in = match obj.get("fade_in") {
        Some(v) => resolve_fade_stage(v)?,
        None => FadeStage { ms: 0, curve: Curve::Linear },
    };
    let fade_out = match obj.get("fade_out") {
        Some(v) => resolve_fade_stage(v)?,
        None => FadeStage { ms: 5, curve: Curve::Linear },
    };
    let levels =
        resolve_contour(obj.get("levels").ok_or_else(|| internal("levels missing"))?, "level")?;
    Ok(VolumeContour { fade_in, fade_out, levels })
}

fn resolve_fade_stage(value: &Value) -> PiccleResult<FadeStage> {
    let obj = value.as_object().ok_or_else(|| internal("fade stage not object"))?;
    Ok(FadeStage {
        ms: integer_field(obj, "ms", 0)?,
        curve: curve_field(obj, "curve", Curve::Linear)?,
    })
}

fn resolve_contour(value: &Value, target_key: &str) -> PiccleResult<Vec<ContourEntry>> {
    let arr = value.as_array().ok_or_else(|| internal("contour not array"))?;
    let mut entries = Vec::with_capacity(arr.len());
    for entry in arr {
        let obj = entry.as_object().ok_or_else(|| internal("contour entry not object"))?;
        entries.push(ContourEntry {
            target: number_field(obj, target_key, 0.0)?,
            hold_ms: integer_field(obj, "hold_ms", 0)?,
            transition_ms: integer_field(obj, "transition_ms", 0)?,
            transition_curve: curve_field(obj, "transition_curve", Curve::Linear)?,
        });
    }
    Ok(entries)
}
