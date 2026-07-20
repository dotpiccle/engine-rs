//! Preparation-time reverb calibration benchmarks.
//!
//! Calibration is outside the real-time render path, but its time and scratch
//! memory scale with `tail_ms`. These cases cover the normal micro-audio range;
//! the 60-second resource ceiling is measured separately because running it
//! repeatedly would make the Criterion suite impractical.

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use piccle_dsp::reverb::ReverbConfig;

const SAMPLE_RATE: u32 = 48_000;
const SOFTEN_HZ: f64 = 8_000.0;

fn bench_reverb_preparation(c: &mut Criterion) {
    let mut group = c.benchmark_group("reverb_preparation");
    for tail_ms in [1_u64, 20, 220, 500] {
        group.bench_function(format!("tail_{tail_ms}ms"), |b| {
            b.iter(|| ReverbConfig::new(tail_ms, SOFTEN_HZ, SAMPLE_RATE));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_reverb_preparation);
criterion_main!(benches);
