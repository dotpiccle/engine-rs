//! Standalone on-device performance probe for the Piccle render path.
//!
//! Build and run through `cargo xtask device-bench`. The binary deliberately
//! uses only the engine crates and the Rust standard library so it can be
//! pushed directly to an Android device without an APK or audio integration.

#![forbid(unsafe_code)]
#![deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use piccle_core::curve::Curve;
use piccle_core::error::PiccleError;
use piccle_core::model::{
    ContourEntry, Document, Filter, FilterType, Layer, Reverb, Source, SpatialEffect, ToneSource,
    VolumeContour, Waveform,
};
use piccle_render::{RenderPlan, Renderer as LowLevelRenderer};

const SAMPLE_RATE: u32 = 48_000;
const CALLBACK_FRAMES: usize = 128;

struct BenchCase {
    name: &'static str,
    document: Document,
    measured_frames: u64,
}

struct BenchResult {
    name: &'static str,
    prepare: Duration,
    renderer_init: Duration,
    render: Duration,
    maximum_callback: Duration,
    callback_time: Duration,
    callback_count: u64,
    measured_frames: u64,
    checksum: f64,
    peak_rss_kib: Option<u64>,
}

impl BenchResult {
    fn frames_per_second(&self) -> f64 {
        self.measured_frames as f64 / self.render.as_secs_f64()
    }

    fn real_time_factor(&self) -> f64 {
        self.frames_per_second() / f64::from(SAMPLE_RATE)
    }

    fn maximum_callback_over_average(&self) -> f64 {
        let average = self.callback_time.as_secs_f64() / self.callback_count as f64;
        self.maximum_callback.as_secs_f64() / average
    }
}

struct RenderStats {
    maximum_callback: Duration,
    callback_time: Duration,
    callback_count: u64,
    checksum: f64,
}

trait StreamingRenderer {
    fn render_into_device_buffer(&mut self, output: &mut [f32]) -> Result<usize, PiccleError>;
}

impl StreamingRenderer for LowLevelRenderer<'_> {
    fn render_into_device_buffer(&mut self, output: &mut [f32]) -> Result<usize, PiccleError> {
        self.render_into(output)
    }
}

impl StreamingRenderer for piccle::Renderer<'_> {
    fn render_into_device_buffer(&mut self, output: &mut [f32]) -> Result<usize, PiccleError> {
        self.render_into(output)
    }
}

struct ToneLayerParams {
    id: String,
    duration_ms: u64,
    wave: Waveform,
    hz: f64,
}

struct DocumentParams {
    layers: Vec<Layer>,
    duration_ms: u64,
    spatial_effects: Vec<SpatialEffect>,
}

fn held_hz(hz: f64) -> ContourEntry {
    ContourEntry { target: hz, hold_ms: 0, transition_ms: 0, transition_curve: Curve::Linear }
}

fn moving_hz(duration_ms: u64) -> Vec<ContourEntry> {
    vec![
        ContourEntry {
            target: 500.0,
            hold_ms: 0,
            transition_ms: duration_ms,
            transition_curve: Curve::Linear,
        },
        held_hz(8_000.0),
    ]
}

fn tone_layer(params: ToneLayerParams) -> Layer {
    Layer {
        id: params.id,
        start_ms: 0,
        duration_ms: params.duration_ms,
        source: Source::Tone(ToneSource {
            wave: params.wave,
            frequencies: vec![held_hz(params.hz)],
            offset_cents: 0,
        }),
        volume: VolumeContour::constant(0.25),
        balance: 0.0,
        filters: Vec::new(),
    }
}

fn document(params: DocumentParams) -> Document {
    Document {
        name: Some("device benchmark".to_owned()),
        description: None,
        duration_ms: params.duration_ms,
        master_volume_level: 1.0,
        spatial_effects: params.spatial_effects,
        layers: params.layers,
    }
}

fn oscillator_case() -> BenchCase {
    let duration_ms = 2_000;
    BenchCase {
        name: "one_20hz_saw",
        document: document(DocumentParams {
            layers: vec![tone_layer(ToneLayerParams {
                id: "saw".to_owned(),
                duration_ms,
                wave: Waveform::Saw,
                hz: 20.0,
            })],
            duration_ms,
            spatial_effects: Vec::new(),
        }),
        measured_frames: 48_000,
    }
}

fn representative_case() -> BenchCase {
    let duration_ms = 2_000;
    let layers = (0..4)
        .map(|index| {
            let mut layer = tone_layer(ToneLayerParams {
                id: format!("representative-{index}"),
                duration_ms,
                wave: Waveform::Sine,
                hz: 220.0 + 110.0 * index as f64,
            });
            layer.filters.push(Filter {
                filter_type: FilterType::Lowpass,
                frequencies: moving_hz(duration_ms),
                resonance: 0.5,
            });
            layer
        })
        .collect();
    BenchCase {
        name: "representative_4_voices_1_moving_filter",
        document: document(DocumentParams {
            layers,
            duration_ms,
            spatial_effects: vec![SpatialEffect::Reverb(Reverb {
                amount: 0.3,
                tail_ms: 500,
                soften_hz: 4_000.0,
            })],
        }),
        measured_frames: 48_000,
    }
}

fn maximum_case() -> BenchCase {
    let duration_ms = 100;
    let filters = (0..16)
        .map(|index| Filter {
            filter_type: match index % 3 {
                0 => FilterType::Lowpass,
                1 => FilterType::Highpass,
                _ => FilterType::Bandpass,
            },
            frequencies: vec![held_hz(500.0 + 500.0 * index as f64)],
            resonance: 0.5,
        })
        .collect::<Vec<_>>();
    let layers = (0..128)
        .map(|index| {
            let mut layer = tone_layer(ToneLayerParams {
                id: format!("maximum-{index}"),
                duration_ms,
                wave: Waveform::Saw,
                hz: 20.0,
            });
            layer.filters = filters.clone();
            layer
        })
        .collect();
    BenchCase {
        name: "maximum_128_voices_16_filters_reverb",
        document: document(DocumentParams {
            layers,
            duration_ms,
            spatial_effects: vec![SpatialEffect::Reverb(Reverb {
                amount: 0.5,
                tail_ms: 500,
                soften_hz: 4_000.0,
            })],
        }),
        measured_frames: 4_096,
    }
}

fn render_frames(
    renderer: &mut impl StreamingRenderer,
    target_frames: u64,
) -> Result<RenderStats, PiccleError> {
    let mut output = [0.0_f32; CALLBACK_FRAMES * 2];
    let mut rendered = 0_u64;
    let mut checksum = 0.0_f64;
    let mut maximum_callback = Duration::ZERO;
    let mut callback_time = Duration::ZERO;
    let mut callback_count = 0_u64;
    while rendered < target_frames {
        let remaining = usize::try_from(target_frames - rendered).unwrap_or(usize::MAX);
        let requested = remaining.min(CALLBACK_FRAMES);
        let callback_started = Instant::now();
        let written = renderer.render_into_device_buffer(&mut output[..requested * 2])?;
        let callback_elapsed = callback_started.elapsed();
        maximum_callback = maximum_callback.max(callback_elapsed);
        callback_time += callback_elapsed;
        callback_count += 1;
        if written == 0 {
            break;
        }
        checksum +=
            output[..written * 2].iter().map(|&sample| f64::from(sample).abs()).sum::<f64>();
        rendered += written as u64;
    }
    black_box(checksum);
    Ok(RenderStats { maximum_callback, callback_time, callback_count, checksum })
}

fn run_case(case: BenchCase) -> Result<BenchResult, PiccleError> {
    let prepare_started = Instant::now();
    let plan = RenderPlan::compile_validated(&case.document, SAMPLE_RATE);
    let prepare = prepare_started.elapsed();
    let measured_frames = case.measured_frames.min(plan.output_frames());

    let mut warmup = LowLevelRenderer::new(&plan);
    render_frames(&mut warmup, measured_frames.min(4_096))?;

    let renderer_started = Instant::now();
    let mut renderer = LowLevelRenderer::new(&plan);
    let renderer_init = renderer_started.elapsed();
    let render_started = Instant::now();
    let render_stats = render_frames(&mut renderer, measured_frames)?;
    let render = render_started.elapsed();

    Ok(BenchResult {
        name: case.name,
        prepare,
        renderer_init,
        render,
        maximum_callback: render_stats.maximum_callback,
        callback_time: render_stats.callback_time,
        callback_count: render_stats.callback_count,
        measured_frames,
        checksum: render_stats.checksum,
        peak_rss_kib: peak_rss_kib(),
    })
}

fn run_official_example(name: &'static str, bytes: &[u8]) -> Result<BenchResult, PiccleError> {
    let prepare_started = Instant::now();
    let plan = piccle::prepare(bytes)?;
    let prepare = prepare_started.elapsed();
    let measured_frames = plan.output_frames();

    let mut warmup = piccle::Renderer::new(&plan);
    render_frames(&mut warmup, measured_frames.min(4_096))?;

    let renderer_started = Instant::now();
    let mut renderer = piccle::Renderer::new(&plan);
    let renderer_init = renderer_started.elapsed();
    let render_started = Instant::now();
    let render_stats = render_frames(&mut renderer, measured_frames)?;
    let render = render_started.elapsed();

    Ok(BenchResult {
        name,
        prepare,
        renderer_init,
        render,
        maximum_callback: render_stats.maximum_callback,
        callback_time: render_stats.callback_time,
        callback_count: render_stats.callback_count,
        measured_frames,
        checksum: render_stats.checksum,
        peak_rss_kib: peak_rss_kib(),
    })
}

fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let value = status.lines().find_map(|line| line.strip_prefix("VmHWM:"))?;
    value.split_whitespace().next()?.parse().ok()
}

fn print_result(result: &BenchResult) {
    println!(
        "{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.2}\t{:.1}\t{:.3}\t{:.6}\t{}",
        result.name,
        result.prepare.as_secs_f64() * 1_000.0,
        result.renderer_init.as_secs_f64() * 1_000.0,
        result.render.as_secs_f64() * 1_000.0,
        result.maximum_callback.as_secs_f64() * 1_000_000.0,
        result.maximum_callback_over_average(),
        result.frames_per_second(),
        result.real_time_factor(),
        result.checksum,
        result.peak_rss_kib.map_or_else(|| "unavailable".to_owned(), |value| value.to_string()),
    );
}

const OFFICIAL_EXAMPLES: [(&str, &[u8]); 15] = [
    ("example_bloom", include_bytes!("../../../../piccle-spec/examples/bloom.json")),
    ("example_button_click", include_bytes!("../../../../piccle-spec/examples/button-click.json")),
    ("example_droplet", include_bytes!("../../../../piccle-spec/examples/droplet.json")),
    ("example_echo", include_bytes!("../../../../piccle-spec/examples/echo.json")),
    ("example_error", include_bytes!("../../../../piccle-spec/examples/error.json")),
    ("example_loading", include_bytes!("../../../../piccle-spec/examples/loading.json")),
    ("example_notification", include_bytes!("../../../../piccle-spec/examples/notification.json")),
    ("example_page", include_bytes!("../../../../piccle-spec/examples/page.json")),
    ("example_ready", include_bytes!("../../../../piccle-spec/examples/ready.json")),
    ("example_sparkle", include_bytes!("../../../../piccle-spec/examples/sparkle.json")),
    ("example_success", include_bytes!("../../../../piccle-spec/examples/success.json")),
    ("example_toggle_off", include_bytes!("../../../../piccle-spec/examples/toggle-off.json")),
    ("example_toggle_on", include_bytes!("../../../../piccle-spec/examples/toggle-on.json")),
    ("example_transition", include_bytes!("../../../../piccle-spec/examples/transition.json")),
    ("example_whisper", include_bytes!("../../../../piccle-spec/examples/whisper.json")),
];

fn main() -> Result<(), PiccleError> {
    println!(
        "case\tprepare_ms\trenderer_init_ms\trender_ms\tmax_callback_us\tmax_callback_over_average\tframes_per_second\treal_time_factor\tchecksum\tprocess_peak_rss_kib"
    );
    for case in [oscillator_case(), representative_case()] {
        print_result(&run_case(case)?);
    }
    for (name, bytes) in OFFICIAL_EXAMPLES {
        print_result(&run_official_example(name, bytes)?);
    }
    print_result(&run_case(maximum_case())?);
    Ok(())
}
