//! Every official example renders with the exact scheduled frame count and
//! finite output (piccle-spec/docs/15-engine-build-guide.md).

use std::path::PathBuf;

use piccle::{CANONICAL_SAMPLE_RATE, Renderer};
use piccle_core::schedule::frame_at;

fn spec_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PICCLE_SPEC_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../piccle-spec")
}

fn examples() -> Vec<(String, Vec<u8>)> {
    let mut names = std::fs::read_dir(spec_dir().join("examples"))
        .expect("examples dir")
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".json"))
        .collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let bytes =
                std::fs::read(spec_dir().join("examples").join(&name)).expect("read example");
            (name, bytes)
        })
        .collect()
}

#[test]
fn official_examples_output_frames_match_absolute_schedule() {
    for (name, bytes) in examples() {
        let plan = piccle::prepare(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let document = piccle_validate::Validator::validate(&bytes).expect("valid example");
        let tail_ms = document.reverb.map_or(0, |reverb| reverb.tail_ms);
        let expected = frame_at(document.duration_ms + tail_ms, CANONICAL_SAMPLE_RATE);
        assert_eq!(plan.output_frames(), expected, "{name}");
    }
}

#[test]
fn official_examples_render_exact_sample_count() {
    for (name, bytes) in examples() {
        let plan = piccle::prepare(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let output = Renderer::render_to_vec(&plan).expect("render example");
        assert_eq!(output.len() as u64, 2 * plan.output_frames(), "{name}");
    }
}

#[test]
fn official_examples_render_finite_output() {
    for (name, bytes) in examples() {
        let plan = piccle::prepare(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let output = Renderer::render_to_vec(&plan).expect("render example");
        assert!(output.iter().all(|sample| sample.is_finite()), "{name}: non-finite sample");
    }
}
