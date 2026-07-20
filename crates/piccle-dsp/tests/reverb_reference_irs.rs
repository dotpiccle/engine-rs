//! Non-normative stronger check against the published reference IR renders.
//!
//! The specification requires perceptual equivalence because platform
//! transcendental implementations may differ. On a matching host `libm`,
//! bit identity provides useful regression evidence for arithmetic ordering.

use std::path::PathBuf;

use piccle_dsp::reverb::generate_reference_ir;

fn spec_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PICCLE_SPEC_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../piccle-spec")
}

fn read_fixture(filename: &str) -> Vec<f64> {
    let bytes =
        std::fs::read(spec_dir().join("test-vectors/numeric/reverb-reference-irs").join(filename))
            .expect("read fixture");
    bytes
        .chunks_exact(8)
        .map(|word| f64::from_le_bytes(word.try_into().expect("8 bytes")))
        .collect()
}

fn interleave(left: &[f64], right: &[f64]) -> Vec<f64> {
    left.iter().zip(right).flat_map(|(&l, &r)| [l, r]).collect()
}

fn check_bit_identity(filename: &str, tail_ms: u64, soften_hz: f64) {
    let expected = read_fixture(filename);
    let (left, right) = generate_reference_ir(tail_ms, soften_hz, 48_000);
    let actual = interleave(&left, &right);
    let mismatch = actual.iter().zip(&expected).position(|(a, e)| a.to_bits() != e.to_bits());
    assert_eq!((actual.len(), mismatch), (expected.len(), None), "{filename}: output mismatch");
}

#[test]
fn tail_1_ms_matches_reference_ir_bit_for_bit() {
    check_bit_identity("tail_001_ms_soften_4000_hz_at_48000.bin", 1, 4_000.0);
}

#[test]
fn tail_10_ms_matches_reference_ir_bit_for_bit() {
    check_bit_identity("tail_010_ms_soften_4000_hz_at_48000.bin", 10, 4_000.0);
}

#[test]
fn tail_20_ms_matches_reference_ir_bit_for_bit() {
    check_bit_identity("tail_020_ms_soften_4000_hz_at_48000.bin", 20, 4_000.0);
}

#[test]
fn tail_220_ms_matches_reference_ir_bit_for_bit() {
    check_bit_identity("tail_220_ms_soften_4000_hz_at_48000.bin", 220, 4_000.0);
}

#[test]
fn tail_500_ms_matches_reference_ir_bit_for_bit() {
    check_bit_identity("tail_500_ms_soften_4000_hz_at_48000.bin", 500, 4_000.0);
}
