# Piccle Engine v1.0.0

The first stable source release of the Piccle Rust reference engine, pinned to the Piccle
specification's immutable `v1.0.1` tag (`b8797cd`).

## Integration

- Pass untrusted Piccle JSON bytes to `piccle::prepare` or `piccle::prepare_with_rate`.
- Stream raw normalized interleaved stereo `f32` PCM with `Renderer::render_into`, or use the
  bounded `Renderer::render_to_vec` convenience API.
- Keep preparation and renderer construction outside the real-time callback; steady streaming is
  allocation-free.
- Platform SDKs own their native ABI, audio queue, device routing, resampling, and integer-PCM
  conversion.

This GitHub release is the installation source. The workspace crates are versioned `1.0.0` but are
intentionally not published to crates.io yet.

## Qualification

- 1,472 engine conformance checks cover every valid/invalid fixture, numeric and schedule aids,
  oscillator DFTs, canonical echo, parallel effects, all official examples, five canonical reverb
  fixtures, ten qualification configurations, forty additional-profile configurations, and one
  deterministic 100-case PCG32 seed-0 reverb differential pass.
- The specification schema SHA-256 is
  `58bbd0946fa5c8e7175866f7a48b4afcd5ef00b1f3c9b29ee8197b396f55ceb4`.
- See `CONFORMANCE.md` and `BENCHMARKS.md` for exact evidence, resource ceilings, device results,
  and workload classifications.

## Specification clarification

[dotpiccle/spec#25](https://github.com/dotpiccle/spec/issues/25) was resolved in `v1.0.1`:
exponential fade checkpoints are generated with platform-dependent `pow` and now use the
specification's existing tight `8 × epsilon` scaled tolerance. This changes no document format,
formula, or rendered intent.

The connected Galaxy S20 FE validates the ARMv7/API-21 deployment path. No Galaxy J5 performance
claim is made. Resource acceptance ceilings remain offline/ahead-of-playback limits unless a
workload is explicitly measured live.
