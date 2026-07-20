//! Semantic validation: checks beyond the JSON Schema.
//!
//! Spec: piccle-spec/docs/14-conformance.md (semantic stage) with the exact
//! codes/paths of piccle-spec/test-vectors/invalid-expectations.json. Runs
//! only after the schema stage passes; budget arithmetic uses 128-bit
//! integers so no adversarial document can overflow it.

use std::collections::HashSet;

use piccle_core::error::{PiccleError, PiccleResult};
use piccle_core::schedule::echo_repeat_count;
use serde_json::Value;

/// Largest permitted timestamp (2^53 − 1).
const MAX_SAFE_INTEGER_MS: u128 = 9_007_199_254_740_991;

fn semantic_err(code: &'static str, path: String, msg: &str) -> PiccleError {
    PiccleError::semantic(code, path, msg.to_owned())
}

fn internal(what: &str) -> PiccleError {
    PiccleError::internal(format!("semantic stage reached without schema-valid input: {what}"))
}

/// Extracts a non-negative integer millisecond field with a default. Always
/// succeeds after the schema stage; failures indicate an engine bug.
fn int_ms(obj: &serde_json::Map<String, Value>, key: &str, default: u64) -> PiccleResult<u64> {
    crate::resolve::integer_field(obj, key, default)
}

/// Validates semantic rules over a schema-valid document.
///
/// # Errors
///
/// `SemanticInvalid` with the spec's exact codes/paths, or `Internal` when
/// reached without a schema-valid document.
pub fn validate_semantics(value: &Value) -> PiccleResult<()> {
    let root = value.as_object().ok_or_else(|| internal("root not object"))?;
    let layers =
        root.get("layers").and_then(Value::as_array).ok_or_else(|| internal("layers missing"))?;

    let mut seen_ids: HashSet<&str> = HashSet::with_capacity(layers.len());
    let mut max_layer_end_ms: u64 = 0;

    for (i, layer) in layers.iter().enumerate() {
        let layer_path = format!("$.layers[{i}]");
        let lobj = layer.as_object().ok_or_else(|| internal("layer not object"))?;

        let id =
            lobj.get("id").and_then(Value::as_str).ok_or_else(|| internal("layer id missing"))?;
        if !seen_ids.insert(id) {
            return Err(semantic_err(
                "semantic.duplicate_layer_id",
                format!("{layer_path}.id"),
                "layer id must be unique",
            ));
        }

        let start_ms = int_ms(lobj, "start_ms", 0)?;
        let duration_ms = int_ms(lobj, "duration_ms", 0)?;
        max_layer_end_ms = max_layer_end_ms.max(start_ms + duration_ms);

        // Contour timing budgets: Σ(hold+transition) over all but the last
        // entry must fit the layer duration.
        let source = lobj.get("source").and_then(Value::as_object);
        if let Some(source_obj) = source {
            if source_obj.get("type").and_then(Value::as_str) == Some("tone") {
                let frequencies = source_obj
                    .get("pitch")
                    .and_then(Value::as_object)
                    .and_then(|pitch| pitch.get("frequencies"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| internal("pitch frequencies missing"))?;
                let budget = contour_budget_ms(frequencies)?;
                if budget > u128::from(duration_ms) {
                    return Err(semantic_err(
                        "semantic.pitch_timing_exceeds_duration",
                        format!("{layer_path}.source.pitch.frequencies"),
                        "pitch contour timing exceeds layer duration",
                    ));
                }
            }
        }

        if let Some(filters) = lobj.get("filters").and_then(Value::as_array) {
            for (j, filter) in filters.iter().enumerate() {
                let fobj = filter.as_object().ok_or_else(|| internal("filter not object"))?;
                let frequencies = fobj
                    .get("frequencies")
                    .and_then(Value::as_array)
                    .ok_or_else(|| internal("filter frequencies missing"))?;
                let budget = contour_budget_ms(frequencies)?;
                if budget > u128::from(duration_ms) {
                    return Err(semantic_err(
                        "semantic.filter_timing_exceeds_duration",
                        format!("{layer_path}.filters[{j}].frequencies"),
                        "filter contour timing exceeds layer duration",
                    ));
                }
            }
        }

        if let Some(volume) = lobj.get("volume").and_then(Value::as_object) {
            let fade_in_ms = volume
                .get("fade_in")
                .and_then(Value::as_object)
                .map(|stage| int_ms(stage, "ms", 0))
                .transpose()?
                .unwrap_or(0);
            let fade_out_ms = volume
                .get("fade_out")
                .and_then(Value::as_object)
                .map(|stage| int_ms(stage, "ms", 0))
                .transpose()?
                .unwrap_or(5);
            let levels = volume
                .get("levels")
                .and_then(Value::as_array)
                .ok_or_else(|| internal("volume levels missing"))?;
            let budget = u128::from(fade_in_ms)
                + contour_budget_ms(levels)?
                + u128::from(fade_out_ms.min(duration_ms));
            if budget > u128::from(duration_ms) {
                return Err(semantic_err(
                    "semantic.volume_timing_exceeds_duration",
                    format!("{layer_path}.volume"),
                    "volume contour timing exceeds layer duration",
                ));
            }
        }

        if u128::from(start_ms) + u128::from(duration_ms) > MAX_SAFE_INTEGER_MS {
            return Err(semantic_err(
                "semantic.layer_end_out_of_range",
                format!("{layer_path}.duration_ms"),
                "layer end exceeds the safe-integer bound",
            ));
        }
    }

    // Document duration: explicit, or computed from the latest layer end.
    let document_duration_ms = match root.get("duration_ms") {
        Some(_) => int_ms(root, "duration_ms", 0)?,
        None => max_layer_end_ms,
    };

    if let Some(spatial_effects) = root.get("spatial_effects").and_then(Value::as_array) {
        let mut max_tail_ms = 0_u128;
        let mut max_tail_path = None;
        for (index, effect) in spatial_effects.iter().enumerate() {
            let effect_obj =
                effect.as_object().ok_or_else(|| internal("spatial effect not object"))?;
            let effect_type = effect_obj
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("spatial effect type missing"))?;
            let (tail_ms, path) = match effect_type {
                "reverb" => (
                    u128::from(int_ms(effect_obj, "tail_ms", 0)?),
                    format!("$.spatial_effects[{index}].tail_ms"),
                ),
                "echo" => {
                    let feedback = effect_obj
                        .get("feedback")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| internal("echo feedback missing"))?;
                    let Some(repeat_count) = echo_repeat_count(feedback)
                    else {
                        return Err(semantic_err(
                            "semantic.echo_tail_unbounded",
                            format!("$.spatial_effects[{index}].feedback"),
                            "echo repeat count exceeds the bounded iteration cap",
                        ));
                    };
                    (
                        u128::from(int_ms(effect_obj, "delay_ms", 0)?) * u128::from(repeat_count),
                        format!("$.spatial_effects[{index}].feedback"),
                    )
                }
                _ => return Err(internal("unknown spatial effect type")),
            };
            if tail_ms > max_tail_ms {
                max_tail_ms = tail_ms;
                max_tail_path = Some(path);
            }
        }
        if u128::from(document_duration_ms) + max_tail_ms > MAX_SAFE_INTEGER_MS {
            return Err(semantic_err(
                "semantic.output_end_out_of_range",
                max_tail_path.unwrap_or_else(|| "$.spatial_effects".to_owned()),
                "document duration plus tail exceeds the safe-integer bound",
            ));
        }
    }

    Ok(())
}

/// `Σ(hold_ms + transition_ms)` over all entries except the last (the last
/// entry's timing fields are ignored per spec).
fn contour_budget_ms(entries: &[Value]) -> PiccleResult<u128> {
    let mut budget = 0u128;
    for entry in entries.iter().take(entries.len().saturating_sub(1)) {
        let obj = entry.as_object().ok_or_else(|| internal("contour entry not object"))?;
        budget += u128::from(int_ms(obj, "hold_ms", 0)?);
        budget += u128::from(int_ms(obj, "transition_ms", 0)?);
    }
    Ok(budget)
}
