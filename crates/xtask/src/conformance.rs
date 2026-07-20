//! `cargo xtask conformance` — the spec-defined engine conformance gate of
//! piccle-spec/docs/15-engine-build-guide.md §Engine conformance verification.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use piccle_core::curve::Curve;
use piccle_core::error::PiccleError;
use piccle_core::model::{
    ContourEntry, Document, FilterType, Layer, Reverb, Source, SpatialEffect, ToneSource,
    VolumeContour, Waveform,
};
use piccle_core::schedule::{echo_repeat_count, frame_at, render_frequency_max};
use piccle_dsp::echo::{Echo, EchoConfig};
use piccle_dsp::filter::Biquad;
use piccle_dsp::measure::{dc_magnitude, dft};
use piccle_dsp::noise::Pcg32;
use piccle_dsp::oscillator::{Oscillator, harmonic_coefficient};
use piccle_dsp::reverb::{ReverbConfig, generate_reference_ir, terminal_window_frames};
use piccle_render::plan::{RenderPlan, SourcePlan, SpatialEffectPlan};
use piccle_render::renderer::terminal_window_gain;
use piccle_validate::Validator;

/// Conformance gate entry point.
pub fn run(spec: &Path) -> i32 {
    if !is_spec_root(spec) {
        eprintln!("Invalid Piccle spec root: {}", spec.display());
        return 1;
    }

    let mut report = Report::default();
    println!("piccle engine conformance — spec at {}", spec.display());

    check_valid_fixtures(spec, &mut report);
    check_invalid_fixtures(spec, &mut report);
    check_dsp_values(spec, &mut report);
    check_render_cases(spec, &mut report);
    check_oscillator_spectral_purity(&mut report);
    check_reverb_reference_irs(spec, &mut report);
    check_echo_reference(spec, &mut report);
    check_parallel_spatial_effects(spec, &mut report);
    check_examples(spec, &mut report);

    println!();
    if report.failures == 0 {
        println!("AUTOMATED CONFORMANCE CHECKS PASSED: {} checks", report.checks);
        println!("Release still requires profiling and perceptual review from the build guide.");
        0
    }
    else {
        println!(
            "AUTOMATED CONFORMANCE CHECKS INCOMPLETE: {} failed, {} total",
            report.failures, report.checks
        );
        1
    }
}

pub(super) fn is_spec_root(spec: &Path) -> bool {
    spec.join("docs/14-conformance.md").is_file()
        && spec.join("test-vectors/valid").is_dir()
        && spec.join("test-vectors/invalid").is_dir()
}

#[derive(Default)]
struct Report {
    checks: usize,
    failures: usize,
}

impl Report {
    fn check(&mut self, name: String, ok: bool) {
        self.checks += 1;
        if ok {
            println!("  PASS {name}");
        }
        else {
            self.failures += 1;
            println!("  FAIL {name}");
        }
    }

    fn evidence(&self, name: &str, matches: bool) {
        let status = if matches { "MATCH" } else { "DIFFERS" };
        println!("  EVIDENCE {name}: {status}");
    }
}

fn section(title: &str) {
    println!();
    println!("== {title} ==");
}

fn progress(name: &str) {
    println!("  RUN  {name}");
    if let Err(error) = std::io::stdout().flush() {
        eprintln!("  WARN could not flush conformance progress: {error}");
    }
}

// ---------------------------------------------------------------- A. valid

fn check_valid_fixtures(spec: &Path, report: &mut Report) {
    section("A. valid fixtures classify valid");
    let dir = spec.join("test-vectors/valid");
    let mut entries = std::fs::read_dir(&dir)
        .expect("valid dir")
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".json"))
        .collect::<Vec<_>>();
    entries.sort();
    for name in entries {
        let bytes = std::fs::read(dir.join(&name)).expect("read fixture");
        report.check(
            format!("valid/{}", name.to_string_lossy()),
            Validator::validate(&bytes).is_ok(),
        );
    }
}

// -------------------------------------------------------------- B. invalid

fn check_invalid_fixtures(spec: &Path, report: &mut Report) {
    section("B. invalid fixtures match stage/code/path");
    let text = std::fs::read_to_string(spec.join("test-vectors/invalid-expectations.json"))
        .expect("expectations");
    let expectations: serde_json::Value = serde_json::from_str(&text).expect("parse expectations");
    let obj = expectations.as_object().expect("expectations object");
    let dir = spec.join("test-vectors/invalid");
    for (name, entry) in obj {
        let bytes = std::fs::read(dir.join(name)).expect("read fixture");
        let wanted = (
            entry["stage"].as_str().expect("stage").to_string(),
            entry["code"].as_str().expect("code").to_string(),
            entry["path"].as_str().expect("path").to_string(),
        );
        let actual: Result<Document, PiccleError> = Validator::validate(&bytes);
        let ok = match actual {
            Ok(_) => false,
            Err(error) => {
                error.stage().to_string() == wanted.0
                    && error.code() == wanted.1
                    && error.path() == wanted.2
            }
        };
        report.check(format!("invalid/{name}"), ok);
    }
}

// ------------------------------------------------------------ C. dsp values

fn check_dsp_values(spec: &Path, report: &mut Report) {
    section("C. dsp-values.json recomputed");
    let text = std::fs::read_to_string(spec.join("test-vectors/numeric/dsp-values.json"))
        .expect("dsp-values");
    let values: serde_json::Value = serde_json::from_str(&text).expect("parse dsp-values");
    let f = |path: &[&str]| {
        let mut v = &values;
        for key in path {
            v = &v[*key];
        }
        v.as_f64().expect("number")
    };

    // PCG32 first words for the three documented seeds.
    for (seed, key) in
        [(0_u32, "seed_0_first_u32"), (1, "seed_1_first_u32"), (u32::MAX, "seed_max_first_u32")]
    {
        let expected = values["pcg32"][key]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_u64().expect("word") as u32)
            .collect::<Vec<_>>();
        let mut rng = Pcg32::new(seed);
        let actual = (0..5).map(|_| rng.next_u32()).collect::<Vec<_>>();
        report.check(format!("pcg32 {key}"), actual == expected);
    }

    // Curve progress at t = 0.5.
    let half = 0.5;
    report.check(
        "curve linear@0.5".to_string(),
        Curve::Linear.value(0.0, 1.0, half) == f(&["curve_progress_at_half", "linear"]),
    );
    report.check(
        "curve easeIn@0.5".to_string(),
        Curve::EaseIn.value(0.0, 1.0, half) == f(&["curve_progress_at_half", "easeIn"]),
    );
    report.check(
        "curve easeOut@0.5".to_string(),
        Curve::EaseOut.value(0.0, 1.0, half) == f(&["curve_progress_at_half", "easeOut"]),
    );
    report.check(
        "curve easeInOut@0.5".to_string(),
        Curve::EaseInOut.value(0.0, 1.0, half) == f(&["curve_progress_at_half", "easeInOut"]),
    );
    let exp = Curve::Exponential.value(0.1, 1.0, half);
    report.check(
        "curve exponential 0.1→1 @0.5".to_string(),
        (exp - f(&["curve_progress_at_half", "exponential_0_1_to_1"])).abs() <= 1e-16,
    );

    // Zero-duration transition chain: last target wins at frame zero.
    let chain_doc = tone_document(vec![
        ContourEntry { target: 0.1, hold_ms: 0, transition_ms: 0, transition_curve: Curve::Linear },
        ContourEntry { target: 0.2, hold_ms: 0, transition_ms: 0, transition_curve: Curve::Linear },
        ContourEntry { target: 0.3, hold_ms: 0, transition_ms: 0, transition_curve: Curve::Linear },
    ]);
    let plan = RenderPlan::compile_validated(&chain_doc, 48_000);
    let SourcePlan::Tone { pitch, .. } = plan.layers()[0].source()
    else {
        panic!("tone");
    };
    let mut cursor = 0;
    report.check(
        "zero-duration chain first emitted".to_string(),
        pitch.value_at(&mut cursor, 0)
            == f(&["zero_duration_transition_chain", "first_emitted_target"]),
    );

    // Oscillator series coefficients.
    let coeffs = [
        ("sine_k1", Waveform::Sine, 1_usize),
        ("square_k1", Waveform::Square, 1),
        ("square_k3", Waveform::Square, 3),
        ("saw_k1", Waveform::Saw, 1),
        ("saw_k2", Waveform::Saw, 2),
        ("triangle_k1", Waveform::Triangle, 1),
        ("triangle_k3", Waveform::Triangle, 3),
    ];
    for (key, wave, k) in coeffs {
        report.check(
            format!("oscillator coefficient {key}"),
            harmonic_coefficient(wave, k) == f(&["oscillator_coefficients", key]),
        );
    }

    // piccle-spec/docs/15-engine-build-guide.md step 3 permits a tightly
    // bounded last-bit variance for the normative sin/cos operations.
    let doc = balanced_document(0.0);
    let plan = RenderPlan::compile_validated(&doc, 48_000);
    let layer = &plan.layers()[0];
    report.check(
        "balance center_left".to_string(),
        transcendental_reference_close(layer.pan_left(), f(&["balance", "center_left"])),
    );
    report.check(
        "balance center_right".to_string(),
        transcendental_reference_close(layer.pan_right(), f(&["balance", "center_right"])),
    );
    let mono = (layer.pan_left() + layer.pan_right()) / 2.0_f64.sqrt();
    report.check(
        "balance center_then_mono".to_string(),
        transcendental_reference_close(mono, f(&["balance", "center_then_mono"])),
    );

    // The canonical biquad uses the same published transcendental tolerance.
    let mut biquad = Biquad::new(FilterType::Lowpass, 0.0, 48_000);
    biquad.set_frequency(1_000.0);
    let c = biquad.coefficients();
    for (i, key) in ["b0", "b1", "b2", "a1", "a2"].iter().enumerate() {
        report.check(
            format!("lowpass coefficient {key}"),
            transcendental_reference_close(c[i], f(&["lowpass_1000_hz_48000_resonance_0", key])),
        );
    }

    // render_frequency_max table.
    for rate in [8_000_u32, 16_000, 22_050, 44_100, 48_000] {
        report.check(
            format!("render_frequency_max {rate}"),
            render_frequency_max(rate) == f(&["render_frequency_max_hz", &rate.to_string()]),
        );
    }

    // Absolute boundary frames at 44.1 kHz.
    report.check(
        "frame(4 ms) @ 44.1k".to_string(),
        frame_at(4, 44_100) == f(&["absolute_boundary_frames_at_44100", "frame_4_ms"]) as u64,
    );
    report.check(
        "frame(8 ms) @ 44.1k".to_string(),
        frame_at(8, 44_100) == f(&["absolute_boundary_frames_at_44100", "frame_8_ms"]) as u64,
    );
    report.check(
        "span 4→8 ms @ 44.1k".to_string(),
        frame_at(8, 44_100) - frame_at(4, 44_100)
            == f(&["absolute_boundary_frames_at_44100", "span_4_to_8_ms"]) as u64,
    );

    // Pitch transform order: contour → cents → clamp.
    let fmax = render_frequency_max(48_000);
    let transformed = (20.0_f64 * 2.0_f64.powf(-1.0)).clamp(20.0, fmax);
    report.check(
        "pitch 20 Hz −1200 cents".to_string(),
        transformed == f(&["pitch_transform_order", "20_hz_minus_1200_cents_canonical"]),
    );
    let transformed = (20_000.0_f64 * 2.0_f64).clamp(20.0, fmax);
    report.check(
        "pitch 20000 Hz +1200 cents".to_string(),
        transformed == f(&["pitch_transform_order", "20000_hz_plus_1200_cents_canonical"]),
    );
    let transformed = (10_000.0_f64).clamp(20.0, render_frequency_max(8_000));
    report.check(
        "pitch 10000 Hz @ 8k".to_string(),
        transformed == f(&["pitch_transform_order", "10000_hz_at_8000_sample_rate"]),
    );

    // Canonical mix order: binary64 sequential accumulation.
    let samples = [1.0_f64, 1.110_223_024_625_156_5e-16, -1.0];
    let sum = samples.iter().fold(0.0_f64, |acc, s| acc + s);
    report.check(
        "canonical mix order".to_string(),
        sum == f(&["canonical_mix_order", "binary64_result"]),
    );

    // Reverb terminal window widths at 48 kHz.
    for (key, tail_ms) in
        [("tail_1_ms", 1_u64), ("tail_10_ms", 10), ("tail_20_ms", 20), ("tail_500_ms", 500)]
    {
        let w = terminal_window_frames(frame_at(tail_ms, 48_000), 48_000);
        report.check(
            format!("terminal window {key}"),
            w == f(&["reverb_terminal_window_frames_at_48000", key]) as u64,
        );
    }

    // 1 ms tail terminal gains: window starts at frame 43, gains 1, .75, .5, .25,
    // 0.
    let output_end = frame_at(1, 48_000);
    let window = terminal_window_frames(output_end, 48_000);
    let window_start = output_end - window;
    report.check(
        "1 ms tail window start".to_string(),
        window_start
            == f(&["reverb_tail_1_ms_terminal_gains", "window_start_frame_in_tail"]) as u64,
    );
    let expected_gains = values["reverb_tail_1_ms_terminal_gains"]["gains"]
        .as_array()
        .expect("gains")
        .iter()
        .map(|v| v.as_f64().expect("gain"))
        .collect::<Vec<_>>();
    let actual_gains = (window_start..output_end)
        .map(|n| terminal_window_gain(n, output_end, window))
        .collect::<Vec<_>>();
    report.check("1 ms tail terminal gains".to_string(), actual_gains == expected_gains);

    // Absolute tail frames at 44.1 kHz (production schedule).
    let mut doc = tone_document(vec![ContourEntry {
        target: 1_000.0,
        hold_ms: 0,
        transition_ms: 0,
        transition_curve: Curve::Linear,
    }]);
    doc.duration_ms = 4;
    doc.spatial_effects.push(SpatialEffect::Reverb(Reverb {
        amount: 0.25,
        tail_ms: 4,
        soften_hz: 4_000.0,
    }));
    let plan = RenderPlan::compile_validated(&doc, 44_100);
    let reverb = plan.reverb().expect("reverb");
    report.check(
        "44.1k dry_end_frame".to_string(),
        plan.dry_end_frame()
            == f(&["reverb_absolute_tail_frames_at_44100", "dry_end_frame"]) as u64,
    );
    report.check(
        "44.1k output_end_frame".to_string(),
        plan.output_frames()
            == f(&["reverb_absolute_tail_frames_at_44100", "output_end_frame"]) as u64,
    );
    report.check(
        "44.1k tail_frames".to_string(),
        reverb.tail_frames() == f(&["reverb_absolute_tail_frames_at_44100", "tail_frames"]) as u64,
    );

    // Reverb baseline delay lengths and direct gains.
    for key in ["tail_1_ms", "tail_20_ms", "tail_220_ms", "tail_500_ms"] {
        let tail_ms: u64 =
            key.trim_start_matches("tail_").trim_end_matches("_ms").parse().expect("tail ms");
        let config = ReverbConfig::new(tail_ms, 4_000.0, 48_000);
        let base = &values["reverb_baseline_at_48000"][key];
        let expected_l = u64_array(&base["allpass_left_frames"]);
        let expected_r = u64_array(&base["allpass_right_frames"]);
        let expected_fdn = u64_array(&base["fdn_frames"]);
        report.check(
            format!("{key} allpass left"),
            config.diffuser_lengths_left().iter().map(|&v| v as u64).collect::<Vec<_>>()
                == expected_l,
        );
        report.check(
            format!("{key} allpass right"),
            config.diffuser_lengths_right().iter().map(|&v| v as u64).collect::<Vec<_>>()
                == expected_r,
        );
        report.check(
            format!("{key} fdn"),
            config.fdn_lengths().iter().map(|&v| v as u64).collect::<Vec<_>>() == expected_fdn,
        );
        report.check(
            format!("{key} direct gain"),
            config.direct_gain() == base["direct_gain"].as_f64().expect("direct"),
        );
    }
}

/// piccle-spec/docs/15-engine-build-guide.md step 3.
fn transcendental_reference_close(actual: f64, reference: f64) -> bool {
    let tolerance = 8.0 * f64::EPSILON * reference.abs().max(1.0);
    (actual - reference).abs() <= tolerance
}

fn u64_array(value: &serde_json::Value) -> Vec<u64> {
    value.as_array().expect("array").iter().map(|v| v.as_u64().expect("u64")).collect()
}

fn tone_document(frequencies: Vec<ContourEntry>) -> Document {
    Document {
        name: None,
        description: None,
        duration_ms: 10,
        master_volume_level: 1.0,
        spatial_effects: Vec::new(),
        layers: vec![Layer {
            id: "a".to_string(),
            start_ms: 0,
            duration_ms: 10,
            source: Source::Tone(ToneSource { wave: Waveform::Sine, frequencies, offset_cents: 0 }),
            volume: VolumeContour::constant(1.0),
            balance: 0.0,
            filters: Vec::new(),
        }],
    }
}

fn balanced_document(balance: f64) -> Document {
    let mut doc = tone_document(vec![ContourEntry {
        target: 440.0,
        hold_ms: 0,
        transition_ms: 0,
        transition_curve: Curve::Linear,
    }]);
    doc.layers[0].balance = balance;
    doc
}

// ---------------------------------------------------------- D. render cases

fn check_render_cases(spec: &Path, report: &mut Report) {
    section("D. behavior render-cases schedules");
    let text = std::fs::read_to_string(spec.join("test-vectors/behavior/render-cases.json"))
        .expect("render-cases");
    let cases: serde_json::Value = serde_json::from_str(&text).expect("parse render-cases");
    for case in cases["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let bytes = std::fs::read(
            spec.join("test-vectors/behavior").join(case["document"].as_str().expect("document")),
        )
        .expect("read case document");
        let sample_rate = case["sample_rate"].as_u64().expect("rate") as u32;
        let document = Validator::validate(&bytes).expect("valid case document");
        let plan = RenderPlan::compile_validated(&document, sample_rate);
        let expected = &case["expected"];

        let max_tail_effect =
            plan.spatial_effects().iter().max_by_key(|effect| effect.tail_frames());
        let mut ok = plan.dry_end_frame() == expected["dry_end_frame"].as_u64().expect("dry")
            && plan.output_frames() == expected["output_end_frame"].as_u64().expect("out")
            && max_tail_effect.map_or(0, SpatialEffectPlan::tail_frames)
                == expected["tail_frames"].as_u64().expect("tail")
            && max_tail_effect.map_or(0, SpatialEffectPlan::window_frames)
                == expected["terminal_window_frames"].as_u64().expect("window")
            && document.duration_ms == expected["document_duration_ms"].as_u64().expect("dur")
            && plan.layers().len() == expected["layers"].as_array().expect("layers").len();
        for (layer, expected_layer) in
            plan.layers().iter().zip(expected["layers"].as_array().expect("layers"))
        {
            let get = |key: &str| expected_layer[key].as_u64().expect("u64");
            ok = ok
                && layer.start_frame() == get("declared_start_frame")
                && layer.declared_end_frame() == get("declared_end_frame")
                && layer.active_end_frame() == get("active_end_frame")
                && layer.envelope().fade_start_frame() == get("fade_start_frame")
                && layer.envelope().fade_frames() == get("fade_frames");
        }
        report.check(format!("render-case {id}"), ok);
    }
}

// -------------------------------------------- E. oscillator spectral purity

fn check_oscillator_spectral_purity(report: &mut Report) {
    section("E. oscillator spectral purity (DFT N = 48000)");
    println!("  (this section is CPU-heavy; use `cargo run --release -p xtask` for speed)");
    let rate = 48_000_u32;
    let n = 48_000_usize;
    let waves = [Waveform::Sine, Waveform::Triangle, Waveform::Square, Waveform::Saw];
    let frequencies = [375.0_f64, 1_000.0, 3_000.0, 8_000.0, 16_000.0];
    for wave in waves {
        for frequency in frequencies {
            progress(&format!("{wave:?} @ {frequency} Hz DFT"));
            let mut oscillator = Oscillator::new(wave, rate);
            oscillator.set_frequency(frequency);
            let samples = (0..n).map(|_| oscillator.next_sample()).collect::<Vec<_>>();
            let bins = dft(&samples);
            let dc = dc_magnitude(&samples);
            let mut ok = dc < 1e-4; // −80 dBFS
            let fundamental_bin = frequency as usize; // coherent: bin = k × f0
            let max_k = ((f64::from(rate) / 2.0) / frequency).ceil() as usize - 1;
            for k in 1..=max_k {
                let bin = k * fundamental_bin;
                let reference = harmonic_coefficient(wave, k);
                if reference.abs() >= 1e-3 {
                    let measured = bins[bin].amplitude();
                    let db_error = 20.0 * (measured / reference.abs()).log10().abs();
                    let reference_phase = if reference < 0.0 { std::f64::consts::PI } else { 0.0 };
                    let phase_error = (bins[bin].phase_from_sine() - reference_phase).abs().min(
                        (bins[bin].phase_from_sine() - reference_phase
                            + 2.0 * std::f64::consts::PI)
                            .abs(),
                    );
                    ok = ok && db_error <= 1.0 && phase_error <= std::f64::consts::PI / 180.0;
                }
            }
            // Every non-target bin must stay below −60 dBFS.
            for (bin, coefficient) in bins.iter().enumerate().take(n / 2).skip(1) {
                let is_target = is_target_harmonic_bin(wave, bin, fundamental_bin, max_k);
                if !is_target {
                    ok = ok && coefficient.amplitude() < 1e-3;
                }
            }
            report.check(format!("{wave:?} @ {frequency} Hz"), ok);
        }
    }
}

fn is_target_harmonic_bin(
    wave: Waveform,
    bin: usize,
    fundamental_bin: usize,
    max_harmonic: usize,
) -> bool {
    if bin % fundamental_bin != 0 {
        return false;
    }
    let harmonic = bin / fundamental_bin;
    harmonic <= max_harmonic && harmonic_coefficient(wave, harmonic).abs() >= 1e-3
}

// --------------------------------------------- F. reverb perceptual
// equivalence

const REVERB_METRIC_MIN_FFT_LENGTH: usize = 65_536;
const MODAL_FLOOR_ABSOLUTE_DB: f64 = -30.0;

#[derive(Clone, Copy, Default)]
struct ComplexBin {
    real: f64,
    imaginary: f64,
}

struct ReverbMetrics {
    rt60_crossing: usize,
    total_energy: f64,
    echo_density: f64,
    modal_floor_db: Option<f64>,
    lr_correlation: f64,
    spectral_centroid_hz: Option<f64>,
    onset_frame: usize,
}

struct MagnitudeSpectrum {
    bins: Vec<f64>,
    fft_length: usize,
}

struct ReverbQualificationCase<'a> {
    label: &'a str,
    tail_ms: u64,
    soften_hz: f64,
    sample_rate: u32,
    reference: &'a serde_json::Value,
}

const REVERB_REFERENCE_METRICS_SCRIPT: &str = r#"
import importlib.util
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1]).resolve()
sys.path.insert(0, str(root / "scripts"))
script_path = root / "scripts" / "generate_reverb_reference_irs.py"
module_spec = importlib.util.spec_from_file_location("piccle_reverb_reference", script_path)
module = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(module)
matrix = json.loads((root / "test-vectors" / "numeric" / "reverb-qualification-matrix.json").read_text())

def metric_row(tail_ms, soften_hz, sample_rate):
    print(
        f"  RUN  normative reference {tail_ms}ms/{soften_hz}Hz/{sample_rate}Hz",
        file=sys.stderr,
        flush=True,
    )
    left, right = module.FDN(tail_ms, soften_hz, sample_rate).generate()
    metrics = module.reverb_metrics.compute_all(left, right, {
        "sample_count": len(left),
        "tail_ms": tail_ms,
        "sample_rate": sample_rate,
    })
    return {"tail_ms": tail_ms, "soften_hz": soften_hz, "sample_rate": sample_rate, "metrics": metrics}

matrix_rows = [
    metric_row(entry["tail_ms"], entry["soften_hz"], entry["sample_rate"])
    for entry in matrix["entries"]
]
additional_profile_rows = [
    metric_row(tail_ms, 4000, sample_rate)
    for sample_rate in matrix["additional_profile_sample_rates"]
    if sample_rate != 48000
    for tail_ms in (1, 10, 20, 220, 500)
]
print(json.dumps({
    "matrix": matrix_rows,
    "additional_profiles": additional_profile_rows,
}, allow_nan=False))
"#;

fn check_reverb_reference_irs(spec: &Path, report: &mut Report) {
    section("F. reverb perceptual equivalence");
    let dir = spec.join("test-vectors/numeric/reverb-reference-irs");
    let manifest_text = std::fs::read_to_string(dir.join("manifest.json")).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse reverb manifest");
    for entry in manifest["fixtures"].as_array().expect("fixture entries") {
        let filename = entry["filename"].as_str().expect("fixture filename");
        progress(&format!("canonical reverb fixture {filename}"));
        let tail_ms = entry["tail_ms"].as_u64().expect("fixture tail_ms");
        let sample_rate = entry["sample_rate"].as_u64().expect("fixture sample_rate") as u32;
        let soften_hz = entry["soften_hz"].as_f64().expect("fixture soften_hz");
        let baseline = &entry["metrics"];
        let bytes = std::fs::read(dir.join(filename)).expect("read fixture");
        let expected = bytes
            .chunks_exact(8)
            .map(|word| f64::from_le_bytes(word.try_into().expect("8 bytes")))
            .collect::<Vec<_>>();
        let (left, right) = generate_reference_ir(tail_ms, soften_hz, sample_rate);
        let actual = left.iter().zip(&right).flat_map(|(&l, &r)| [l, r]).collect::<Vec<_>>();
        let identical = actual.len() == expected.len()
            && actual.iter().zip(&expected).all(|(a, e)| a.to_bits() == e.to_bits());
        report.evidence(&format!("{filename} non-normative bit identity"), identical);

        let metrics = reverb_metrics(&left, &right, tail_ms, sample_rate);
        let tail_frames = frame_at(tail_ms, sample_rate) as usize;
        let minimum_crossing = 1 + (0.9 * tail_frames as f64).floor() as usize;
        report.check(
            format!(
                "{filename} RT60 crossing {} in {minimum_crossing}..={tail_frames}",
                metrics.rt60_crossing
            ),
            (minimum_crossing..=tail_frames).contains(&metrics.rt60_crossing),
        );

        let reference_energy = baseline["total_wet_energy"].as_f64().expect("energy baseline");
        let energy_delta_db = 20.0 * (metrics.total_energy / reference_energy).log10();
        report.check(
            format!("{filename} energy delta {energy_delta_db:.6} dB within ±0.5 dB"),
            energy_delta_db.is_finite() && energy_delta_db.abs() <= 0.5,
        );

        let reference_echo = baseline["echo_density"].as_f64().expect("echo baseline");
        report.check(
            format!(
                "{filename} echo density {:.6} within ±10% of {reference_echo:.6}",
                metrics.echo_density
            ),
            within_relative(metrics.echo_density, reference_echo, 0.1),
        );

        let reference_modal = baseline["modal_resonance_floor_db"].as_f64();
        let modal_ok = match (metrics.modal_floor_db, reference_modal) {
            (_, None) => true,
            (Some(actual), Some(reference)) => modal_floor_within_tolerance(actual, reference),
            (None, Some(_)) => false,
        };
        report.check(
            format!(
                "{filename} modal floor {:?} vs reference {reference_modal:?}",
                metrics.modal_floor_db
            ),
            modal_ok,
        );

        let reference_correlation = baseline["lr_correlation"].as_f64().expect("L/R baseline");
        report.check(
            format!(
                "{filename} L/R correlation {:.6} within ±0.15 of {reference_correlation:.6}",
                metrics.lr_correlation
            ),
            (metrics.lr_correlation - reference_correlation).abs() <= 0.15,
        );

        let reference_centroid =
            baseline["spectral_centroid_hz"].as_f64().expect("centroid baseline");
        report.check(
            format!(
                "{filename} centroid {:?} within ±10% of {reference_centroid:.3} Hz",
                metrics.spectral_centroid_hz
            ),
            metrics
                .spectral_centroid_hz
                .is_some_and(|actual| within_relative(actual, reference_centroid, 0.1)),
        );

        let reference_onset = baseline["onset_frame"].as_u64().expect("onset baseline") as usize;
        report.check(
            format!(
                "{filename} onset {} within ±1 frame of {reference_onset}",
                metrics.onset_frame
            ),
            metrics.onset_frame.abs_diff(reference_onset) <= 1,
        );
    }
    check_reverb_qualification_matrix(spec, report);
}

fn check_reverb_qualification_matrix(spec: &Path, report: &mut Report) {
    let matrix_path = spec.join("test-vectors/numeric/reverb-qualification-matrix.json");
    let matrix_text = std::fs::read_to_string(matrix_path).expect("read qualification matrix");
    let matrix: serde_json::Value =
        serde_json::from_str(&matrix_text).expect("parse qualification matrix");
    let entries = matrix["entries"].as_array().expect("qualification entries");
    progress("generating normative reverb qualification references with Python");
    let references = match normative_reverb_reference_metrics(spec) {
        Ok(value) => value,
        Err(error) => {
            report.check(format!("qualification matrix reference generation: {error}"), false);
            return;
        }
    };
    let reference_rows = references["matrix"].as_array().expect("reference metric rows");
    report.check(
        "qualification matrix reference count".into(),
        entries.len() == reference_rows.len(),
    );

    for (entry, reference_row) in entries.iter().zip(reference_rows) {
        let tail_ms = entry["tail_ms"].as_u64().expect("matrix tail_ms");
        let soften_hz = entry["soften_hz"].as_f64().expect("matrix soften_hz");
        let sample_rate = entry["sample_rate"].as_u64().expect("matrix sample_rate") as u32;
        let label = format!("matrix {tail_ms}ms/{soften_hz}Hz/{sample_rate}Hz");
        progress(&label);
        check_generated_reverb_case(
            &ReverbQualificationCase {
                label: &label,
                tail_ms,
                soften_hz,
                sample_rate,
                reference: &reference_row["metrics"],
            },
            report,
        );
    }

    let profile_rates = matrix["additional_profile_sample_rates"]
        .as_array()
        .expect("additional profile sample rates");
    let expected_profile_count =
        profile_rates.iter().filter(|rate| rate.as_u64() != Some(48_000)).count() * 5;
    let profile_rows =
        references["additional_profiles"].as_array().expect("additional profile metric rows");
    report.check(
        "additional profile reference count".into(),
        profile_rows.len() == expected_profile_count,
    );
    for row in profile_rows {
        let tail_ms = row["tail_ms"].as_u64().expect("profile tail_ms");
        let soften_hz = row["soften_hz"].as_f64().expect("profile soften_hz");
        let sample_rate = row["sample_rate"].as_u64().expect("profile sample_rate") as u32;
        let label = format!("profile {tail_ms}ms/{soften_hz}Hz/{sample_rate}Hz");
        progress(&label);
        check_generated_reverb_case(
            &ReverbQualificationCase {
                label: &label,
                tail_ms,
                soften_hz,
                sample_rate,
                reference: &row["metrics"],
            },
            report,
        );
    }
}

// -------------------------------------------------------- F2. echo reference

fn check_echo_reference(spec: &Path, report: &mut Report) {
    section("F2. canonical echo impulse response");
    let path = spec.join("test-vectors/numeric/echo-impulse-response.json");
    let text = std::fs::read_to_string(path).expect("read echo impulse response");
    let vector: serde_json::Value =
        serde_json::from_str(&text).expect("parse echo impulse response");
    let configuration = &vector["configuration"];
    let derived = &vector["derived"];
    let delay_ms = configuration["delay_ms"].as_u64().expect("echo delay_ms");
    let feedback = configuration["feedback"].as_f64().expect("echo feedback");
    let wet_gain = configuration["wet_gain"].as_f64().expect("echo wet_gain");
    let damp_hz = configuration["damp_hz"].as_f64().expect("echo damp_hz");
    let sample_rate = configuration["sample_rate"].as_u64().expect("echo sample_rate") as u32;
    let repeat_count = echo_repeat_count(feedback).expect("bounded canonical echo");
    let config = EchoConfig::new(delay_ms, feedback, damp_hz, sample_rate);
    let delay_length = config.delay_length() as u64;
    let tail_frames = repeat_count * delay_length;
    let dry_end = derived["dry_end_frame"].as_u64().expect("echo dry end");
    let output_end = dry_end + tail_frames;
    let window = terminal_window_frames(tail_frames, sample_rate);

    report.check(
        "echo derived delay length".into(),
        delay_length == derived["delay_length"].as_u64().expect("derived delay length"),
    );
    report.check(
        "echo derived repeat count".into(),
        repeat_count == derived["N_total"].as_u64().expect("derived repeat count"),
    );
    report.check(
        "echo derived tail frames".into(),
        tail_frames == derived["tail_frames"].as_u64().expect("derived tail frames"),
    );
    report.check(
        "echo derived output end".into(),
        output_end == derived["output_end_frame"].as_u64().expect("derived output end"),
    );
    report.check(
        "echo derived terminal window".into(),
        window == derived["terminal_window_W"].as_u64().expect("derived terminal window"),
    );

    let checkpoints = &vector["checkpoints"];
    let checkpoint_frames = [
        ("frame_0_impulse", 0),
        ("frame_dry_end", dry_end),
        ("frame_delay_length_first_echo", delay_length),
        ("frame_2x_delay_length_second_echo", 2 * delay_length),
        ("frame_3x_delay_length_third_echo", 3 * delay_length),
        ("frame_mid_tail", dry_end + tail_frames / 2),
        ("frame_window_start", output_end - window),
        ("frame_T_minus_1_last", output_end - 1),
    ];
    let impulse = vector["impulse_value"].as_f64().expect("echo impulse");
    let mut actual = [None; 8];
    let mut echo = Echo::new(&config);
    let mut stereo_symmetric = true;
    progress(&format!("rendering {output_end} canonical echo frames"));
    for frame in 0..output_end {
        let dry = if frame == 0 { impulse } else { 0.0 };
        let terminal_gain = terminal_window_gain(frame, output_end, window);
        let (wet_left, wet_right) = echo.process(dry, dry, terminal_gain);
        stereo_symmetric &= wet_left.to_bits() == wet_right.to_bits();
        for (index, (_, checkpoint_frame)) in checkpoint_frames.iter().enumerate() {
            if frame == *checkpoint_frame {
                actual[index] = Some(dry + wet_gain * wet_left);
            }
        }
    }
    report.check("echo preserves symmetric stereo input".into(), stereo_symmetric);
    for (index, (name, _)) in checkpoint_frames.iter().enumerate() {
        let expected = checkpoints[*name].as_f64().expect("echo checkpoint");
        let value = actual[index].expect("captured echo checkpoint");
        let tolerance = 1e-10 * expected.abs().max(1.0);
        report.check(format!("echo checkpoint {name}"), (value - expected).abs() <= tolerance);
    }
}

// ----------------------------------------------- F3. parallel spatial effects

fn check_parallel_spatial_effects(spec: &Path, report: &mut Report) {
    section("F3. parallel spatial-effect order independence");
    let dir = spec.join("test-vectors/valid");
    let forward_bytes = std::fs::read(dir.join("spatial-effects-reverb-then-echo.json"))
        .expect("read forward spatial-effects fixture");
    let reverse_bytes = std::fs::read(dir.join("spatial-effects-echo-then-reverb.json"))
        .expect("read reverse spatial-effects fixture");
    progress("rendering both spatial-effect array orders");
    let forward_plan = piccle::prepare(&forward_bytes).expect("prepare forward spatial effects");
    let reverse_plan = piccle::prepare(&reverse_bytes).expect("prepare reverse spatial effects");
    let forward_output = piccle::Renderer::render_to_vec(&forward_plan).expect("render forward");
    let reverse_output = piccle::Renderer::render_to_vec(&reverse_plan).expect("render reverse");
    report.check(
        "parallel effects produce identical output in either order".into(),
        forward_output == reverse_output,
    );
    report.check(
        "parallel effects produce identical output length in either order".into(),
        forward_plan.output_frames() == reverse_plan.output_frames(),
    );
}

fn check_generated_reverb_case(case: &ReverbQualificationCase<'_>, report: &mut Report) {
    let (left, right) = generate_reference_ir(case.tail_ms, case.soften_hz, case.sample_rate);
    let metrics = reverb_metrics(&left, &right, case.tail_ms, case.sample_rate);
    let tail_frames = frame_at(case.tail_ms, case.sample_rate) as usize;
    let minimum_crossing = 1 + (0.9 * tail_frames as f64).floor() as usize;

    report.check(format!("{} frame count", case.label), left.len() == tail_frames + 1);
    report.check(
        format!("{} RT60 window", case.label),
        (minimum_crossing..=tail_frames).contains(&metrics.rt60_crossing),
    );

    let reference_energy = case.reference["total_wet_energy"].as_f64().expect("reference energy");
    let energy_delta_db = 20.0 * (metrics.total_energy / reference_energy).log10();
    report.check(
        format!("{} energy delta", case.label),
        energy_delta_db.is_finite() && energy_delta_db.abs() <= 0.5,
    );

    let reference_echo = case.reference["echo_density"].as_f64().expect("reference echo density");
    report.check(
        format!("{} echo density", case.label),
        within_relative(metrics.echo_density, reference_echo, 0.1),
    );

    let reference_modal = case.reference["modal_resonance_floor_db"].as_f64();
    let modal_ok = match (metrics.modal_floor_db, reference_modal) {
        (_, None) => true,
        (Some(actual), Some(reference)) => modal_floor_within_tolerance(actual, reference),
        (None, Some(_)) => false,
    };
    report.check(format!("{} modal floor", case.label), modal_ok);

    let reference_correlation =
        case.reference["lr_correlation"].as_f64().expect("reference correlation");
    report.check(
        format!("{} L/R correlation", case.label),
        (metrics.lr_correlation - reference_correlation).abs() <= 0.15,
    );

    let reference_centroid =
        case.reference["spectral_centroid_hz"].as_f64().expect("reference centroid");
    report.check(
        format!("{} spectral centroid", case.label),
        metrics
            .spectral_centroid_hz
            .is_some_and(|actual| within_relative(actual, reference_centroid, 0.1)),
    );

    let reference_onset = case.reference["onset_frame"].as_u64().expect("reference onset") as usize;
    report
        .check(format!("{} onset", case.label), metrics.onset_frame.abs_diff(reference_onset) <= 1);
}

fn normative_reverb_reference_metrics(spec: &Path) -> Result<serde_json::Value, String> {
    let output = Command::new("python3")
        .args(["-c", REVERB_REFERENCE_METRICS_SCRIPT])
        .arg(spec)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("could not launch Python generator: {error}"))?;
    if !output.status.success() {
        return Err(format!("Python generator exited with {}", output.status));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("generator returned invalid metric JSON: {error}"))
}

#[cfg(test)]
pub(super) fn stereo_energy(interleaved: &[f64]) -> f64 {
    interleaved.iter().map(|sample| sample * sample).sum()
}

fn within_relative(actual: f64, reference: f64, tolerance: f64) -> bool {
    if reference == 0.0 {
        return actual == 0.0;
    }
    ((1.0 - tolerance) * reference..=(1.0 + tolerance) * reference).contains(&actual)
}

fn modal_floor_within_tolerance(actual: f64, reference: f64) -> bool {
    let relative_ok = actual <= reference + 6.0;
    let absolute_ok = reference > MODAL_FLOOR_ABSOLUTE_DB || actual <= MODAL_FLOOR_ABSOLUTE_DB;
    relative_ok && absolute_ok
}

fn reverb_metrics(left: &[f64], right: &[f64], tail_ms: u64, sample_rate: u32) -> ReverbMetrics {
    ReverbMetrics {
        rt60_crossing: rt60_crossing(left, right),
        total_energy: left.iter().zip(right).map(|(&l, &r)| l * l + r * r).sum(),
        echo_density: echo_density(left, right, sample_rate),
        modal_floor_db: modal_resonance_floor(left, right, tail_ms, sample_rate),
        lr_correlation: lr_correlation(left, right),
        spectral_centroid_hz: spectral_centroid(left, right, sample_rate),
        onset_frame: onset_frame(left, right),
    }
}

pub(super) fn rt60_crossing(left: &[f64], right: &[f64]) -> usize {
    let mut suffix_energy = vec![0.0; left.len()];
    let mut accumulated = 0.0;
    for index in (0..left.len()).rev() {
        accumulated += left[index] * left[index] + right[index] * right[index];
        suffix_energy[index] = accumulated;
    }
    let threshold = suffix_energy.first().copied().unwrap_or_default() * 1e-6;
    suffix_energy
        .iter()
        .position(|&energy| energy <= threshold)
        .unwrap_or_else(|| left.len().saturating_sub(1))
}

fn echo_density(left: &[f64], right: &[f64], sample_rate: u32) -> f64 {
    let tail_frames = left.len().saturating_sub(1);
    let analysis_frames = tail_frames.min(rounded_frames(50.0, sample_rate));
    if analysis_frames <= 1 {
        return 0.0;
    }

    let mut previous_sign = 0_i8;
    let mut previous_crossing = None;
    let mut interval_count = 0_usize;
    let mut qualifying_count = 0_usize;
    let interval_limit = f64::from(sample_rate) / 1_000.0;

    for index in 0..analysis_frames {
        let mono = (left[index + 1] + right[index + 1]) / 2.0;
        let sign = if mono > 0.0 {
            1
        }
        else if mono < 0.0 {
            -1
        }
        else {
            previous_sign
        };
        if sign == 0 {
            continue;
        }
        if previous_sign != 0 && sign != previous_sign {
            if let Some(previous) = previous_crossing {
                interval_count += 1;
                if ((index - previous) as f64) < interval_limit {
                    qualifying_count += 1;
                }
            }
            previous_crossing = Some(index);
        }
        previous_sign = sign;
    }

    if interval_count == 0 { 0.0 } else { qualifying_count as f64 / interval_count as f64 }
}

fn modal_resonance_floor(
    left: &[f64],
    right: &[f64],
    tail_ms: u64,
    sample_rate: u32,
) -> Option<f64> {
    let mono = left.iter().zip(right).map(|(&l, &r)| (l + r) / 2.0).collect::<Vec<_>>();
    let peak_wet = mono.iter().map(|sample| sample.abs()).fold(0.0_f64, f64::max);
    if peak_wet == 0.0 {
        return None;
    }

    let onset_skip =
        rounded_frames(5.0, sample_rate).max(rounded_frames(0.05 * tail_ms as f64, sample_rate));
    if onset_skip >= mono.len() {
        return None;
    }
    let schroeder_min = rounded_frames(0.15 * tail_ms as f64, sample_rate);
    let total_delay = fdn_total_delay(tail_ms, sample_rate);
    let late_tail = mono.len() - onset_skip;
    let window_frames = late_tail.min(schroeder_min.max(2 * total_delay));
    if window_frames < 2 {
        return None;
    }

    let hop = (window_frames / 4).max(1);
    let mut strongest = 0.0_f64;
    let mut start = onset_skip;
    while start + window_frames <= mono.len() {
        let segment = &mono[start..start + window_frames];
        let mean = segment.iter().sum::<f64>() / window_frames as f64;
        let centered = segment.iter().map(|sample| sample - mean).collect::<Vec<_>>();
        let spectrum = hann_fft_magnitudes(&centered)?;
        let first_audible_bin =
            (20.0 * spectrum.fft_length as f64 / f64::from(sample_rate)).ceil() as usize;
        let window_peak =
            spectrum.bins[first_audible_bin..].iter().copied().fold(0.0_f64, f64::max);
        strongest = strongest.max(4.0 * window_peak / window_frames as f64);
        start += hop;
    }

    if strongest <= 0.0 { None } else { Some(20.0 * (strongest / peak_wet).log10()) }
}

fn lr_correlation(left: &[f64], right: &[f64]) -> f64 {
    if left.len() <= 2 {
        return 0.0;
    }
    let left_tail = &left[1..];
    let right_tail = &right[1..];
    let count = left_tail.len() as f64;
    let mean_left = left_tail.iter().sum::<f64>() / count;
    let mean_right = right_tail.iter().sum::<f64>() / count;
    let mut covariance = 0.0;
    let mut variance_left = 0.0;
    let mut variance_right = 0.0;
    for (&left_sample, &right_sample) in left_tail.iter().zip(right_tail) {
        let centered_left = left_sample - mean_left;
        let centered_right = right_sample - mean_right;
        covariance += centered_left * centered_right;
        variance_left += centered_left * centered_left;
        variance_right += centered_right * centered_right;
    }
    if variance_left == 0.0 || variance_right == 0.0 {
        return 0.0;
    }
    covariance / (variance_left * variance_right).sqrt()
}

fn spectral_centroid(left: &[f64], right: &[f64], sample_rate: u32) -> Option<f64> {
    let mono = left.iter().zip(right).map(|(&l, &r)| (l + r) / 2.0).collect::<Vec<_>>();
    let spectrum = hann_fft_magnitudes(&mono)?;
    let mut weighted_sum = 0.0;
    let mut magnitude_sum = 0.0;
    for (bin, magnitude) in spectrum.bins.iter().copied().enumerate().skip(1) {
        weighted_sum += bin as f64 * magnitude;
        magnitude_sum += magnitude;
    }
    if magnitude_sum == 0.0 {
        return Some(0.0);
    }
    Some(weighted_sum / magnitude_sum * f64::from(sample_rate) / spectrum.fft_length as f64)
}

fn onset_frame(left: &[f64], right: &[f64]) -> usize {
    let peak = left.iter().zip(right).map(|(&l, &r)| l.abs().max(r.abs())).fold(0.0_f64, f64::max);
    if peak == 0.0 {
        return 0;
    }
    let threshold = 0.1 * peak;
    left.iter()
        .zip(right)
        .position(|(&l, &r)| l.abs().max(r.abs()) >= threshold)
        .unwrap_or_else(|| left.len().saturating_sub(1))
}

fn rounded_frames(milliseconds: f64, sample_rate: u32) -> usize {
    (milliseconds * f64::from(sample_rate) / 1_000.0 + 0.5).floor() as usize
}

fn fdn_total_delay(tail_ms: u64, sample_rate: u32) -> usize {
    let tail_frames = frame_at(tail_ms, sample_rate) as usize;
    let ratios = [0.004, 0.006, 0.009, 0.013, 0.019, 0.027, 0.038, 0.053];
    let mut previous = 0_usize;
    let mut total = 0_usize;
    for ratio in ratios {
        let raw = (tail_frames as f64 * ratio + 0.5).floor() as usize;
        let length =
            if previous == 0 { raw.max(1) } else { raw.max(previous + 1).min(tail_frames) };
        previous = length;
        total += length;
    }
    total
}

fn reverb_metric_fft_length(signal_length: usize) -> Option<usize> {
    signal_length.checked_next_power_of_two().map(|length| length.max(REVERB_METRIC_MIN_FFT_LENGTH))
}

fn hann_fft_magnitudes(signal: &[f64]) -> Option<MagnitudeSpectrum> {
    if signal.is_empty() {
        return None;
    }
    let fft_length = reverb_metric_fft_length(signal.len())?;
    let mut bins = vec![ComplexBin::default(); fft_length];
    let divisor = signal.len().saturating_sub(1) as f64;
    for (index, &sample) in signal.iter().enumerate() {
        let gain = if signal.len() == 1 {
            1.0
        }
        else {
            0.5 * (1.0 - (2.0 * std::f64::consts::PI * index as f64 / divisor).cos())
        };
        bins[index].real = sample * gain;
    }

    let mut reversed = 0_usize;
    for index in 1..fft_length {
        let mut bit = fft_length >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            bins.swap(index, reversed);
        }
    }

    let mut length = 2;
    while length <= fft_length {
        let angle = -2.0 * std::f64::consts::PI / length as f64;
        let (twiddle_imaginary, twiddle_real) = angle.sin_cos();
        for start in (0..fft_length).step_by(length) {
            let mut rotation_real = 1.0;
            let mut rotation_imaginary = 0.0;
            for offset in 0..length / 2 {
                let even = bins[start + offset];
                let odd = bins[start + offset + length / 2];
                let rotated_real = odd.real * rotation_real - odd.imaginary * rotation_imaginary;
                let rotated_imaginary =
                    odd.real * rotation_imaginary + odd.imaginary * rotation_real;
                bins[start + offset] = ComplexBin {
                    real: even.real + rotated_real,
                    imaginary: even.imaginary + rotated_imaginary,
                };
                bins[start + offset + length / 2] = ComplexBin {
                    real: even.real - rotated_real,
                    imaginary: even.imaginary - rotated_imaginary,
                };
                let next_real =
                    rotation_real * twiddle_real - rotation_imaginary * twiddle_imaginary;
                rotation_imaginary =
                    rotation_real * twiddle_imaginary + rotation_imaginary * twiddle_real;
                rotation_real = next_real;
            }
        }
        length *= 2;
    }

    Some(MagnitudeSpectrum {
        bins: bins[..=fft_length / 2].iter().map(|bin| bin.real.hypot(bin.imaginary)).collect(),
        fft_length,
    })
}

// --------------------------------------------------------------- G. examples

fn check_examples(spec: &Path, report: &mut Report) {
    section("G. official examples render");
    let dir = spec.join("examples");
    let mut entries = std::fs::read_dir(&dir)
        .expect("examples dir")
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".json"))
        .collect::<Vec<_>>();
    entries.sort();
    for name in entries {
        progress(&format!("rendering examples/{}", name.to_string_lossy()));
        let bytes = std::fs::read(dir.join(&name)).expect("read example");
        let ok = match piccle::prepare(&bytes) {
            Ok(plan) => {
                let document = Validator::validate(&bytes).expect("valid example");
                let tail_frames = document
                    .spatial_effects
                    .iter()
                    .map(|effect| match effect {
                        SpatialEffect::Reverb(reverb) => frame_at(reverb.tail_ms, 48_000),
                        SpatialEffect::Echo(echo) => {
                            let repeat_count = echo_repeat_count(echo.feedback).unwrap_or(0);
                            repeat_count * frame_at(echo.delay_ms, 48_000).max(1)
                        }
                    })
                    .max()
                    .unwrap_or(0);
                let expected_frames = frame_at(document.duration_ms, 48_000) + tail_frames;
                match piccle::Renderer::render_to_vec(&plan) {
                    Ok(output) => {
                        plan.output_frames() == expected_frames
                            && output.len() as u64 == 2 * expected_frames
                            && output.iter().all(|s| s.is_finite())
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        report.check(format!("examples/{}", name.to_string_lossy()), ok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncapped_reference_fdn_has_the_published_220ms_total_delay() {
        assert_eq!(fdn_total_delay(220, 48_000), 1_784);
    }

    #[test]
    fn fft_length_stays_at_the_normative_minimum_on_the_boundary() {
        assert_eq!(reverb_metric_fft_length(65_536), Some(65_536));
    }

    #[test]
    fn fft_length_grows_at_the_first_sample_above_the_boundary() {
        assert_eq!(reverb_metric_fft_length(65_537), Some(131_072));
    }

    #[test]
    fn fft_length_grows_for_long_reverb_signals() {
        assert_eq!(reverb_metric_fft_length(200_000), Some(262_144));
    }

    #[test]
    fn even_square_harmonic_is_checked_as_an_unwanted_component() {
        assert!(!is_target_harmonic_bin(Waveform::Square, 750, 375, 63));
    }

    #[test]
    fn lowpass_reference_accepts_the_published_transcendental_tolerance() {
        assert!(transcendental_reference_close(
            0.003_916_076_683_699_464,
            0.003_916_076_683_699_463_5,
        ));
    }

    #[test]
    fn transcendental_reference_accepts_the_published_tolerance_boundary() {
        let reference = 1.0;
        let tolerance = 8.0 * f64::EPSILON;

        assert!(transcendental_reference_close(reference + tolerance, reference));
    }

    #[test]
    fn transcendental_reference_rejects_the_value_above_the_published_tolerance() {
        let reference = 1.0;
        let above = f64::from_bits((reference + 8.0 * f64::EPSILON).to_bits() + 1);

        assert!(!transcendental_reference_close(above, reference));
    }

    #[test]
    fn modal_floor_requires_the_absolute_gate_when_the_reference_meets_it() {
        assert!(!modal_floor_within_tolerance(-29.9, -32.0));
    }

    #[test]
    fn modal_floor_skips_the_absolute_gate_when_the_reference_exceeds_it() {
        assert!(modal_floor_within_tolerance(-19.8, -19.8));
    }

    #[test]
    fn modal_floor_keeps_the_relative_gate_when_the_absolute_gate_is_not_applicable() {
        assert!(!modal_floor_within_tolerance(-13.7, -19.8));
    }

    #[test]
    fn hann_fft_uses_the_dynamic_length_above_the_boundary() {
        let spectrum = hann_fft_magnitudes(&vec![0.0; 65_537]).expect("non-empty signal");
        assert_eq!(spectrum.fft_length, 131_072);
    }

    #[test]
    fn relative_tolerance_accepts_the_exact_lower_boundary() {
        assert!(within_relative(0.9, 1.0, 0.1));
    }

    #[test]
    fn relative_tolerance_rejects_the_value_below_the_lower_boundary() {
        let below = f64::from_bits((1.0_f64 - 0.1).to_bits() - 1);
        assert!(!within_relative(below, 1.0, 0.1));
    }

    #[test]
    fn relative_tolerance_accepts_the_exact_upper_boundary() {
        assert!(within_relative(1.1, 1.0, 0.1));
    }

    #[test]
    fn relative_tolerance_rejects_the_value_above_the_upper_boundary() {
        let above = f64::from_bits((1.0_f64 + 0.1).to_bits() + 1);
        assert!(!within_relative(above, 1.0, 0.1));
    }

    #[test]
    fn canonical_220ms_response_matches_all_published_metric_baselines() {
        let (left, right) = generate_reference_ir(220, 4_000.0, 48_000);
        let metrics = reverb_metrics(&left, &right, 220, 48_000);
        let modal = metrics.modal_floor_db.expect("220 ms modal analysis is non-degenerate");

        assert!(
            metrics.rt60_crossing == 10_033
                && (metrics.total_energy - 1.0).abs() <= 1e-12
                && (metrics.echo_density - 1.0).abs() <= f64::EPSILON
                && (modal - -39.9).abs() <= 0.1
                && (metrics.lr_correlation - 0.094_577_218_459_210_65).abs() <= 1e-12
                && (metrics.spectral_centroid_hz.expect("non-zero spectrum")
                    - 9_024.985_902_550_228)
                    .abs()
                    <= 1e-6
                && metrics.onset_frame == 0,
            "rt60={}, energy={}, echo={}, modal={}, correlation={}, centroid={:?}, onset={}",
            metrics.rt60_crossing,
            metrics.total_energy,
            metrics.echo_density,
            modal,
            metrics.lr_correlation,
            metrics.spectral_centroid_hz,
            metrics.onset_frame,
        );
    }
}
