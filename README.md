# Piccle Engine

The production Rust reference engine for the [Piccle](https://github.com/dotpiccle/spec) procedural
UI-audio format.

Piccle assets describe short one-shot sounds as deterministic synthesis instructions rather than
recorded samples. This engine applies parser resource limits, validates the document, resolves an
immutable render plan, and emits clipped interleaved stereo `f32` samples. Platform audio I/O is
deliberately outside the core library.

## Use

Depend on the umbrella crate. Its opaque `RenderPlan` can only be obtained through the complete
untrusted-input boundary:

```rust
use piccle::Renderer;

let bytes = br#"{
  "piccle": "1.0",
  "layers": [{
    "id": "tap",
    "duration_ms": 30,
    "source": {
      "type": "tone",
      "wave": "sine",
      "pitch": { "frequencies": [{ "hz": 880 }] }
    }
  }]
}"#;

let plan = piccle::prepare(bytes)?;
let mut renderer = Renderer::new(&plan);
let mut block = [0.0_f32; 512 * 2];

while !renderer.is_finished() {
    let frames = renderer.render_into(&mut block)?;
    let stereo_samples = &block[..frames * 2];
    // Send stereo_samples to the host audio API.
}
# Ok::<(), piccle::PiccleError>(())
```

`Renderer::render_into` performs no allocation. `Renderer::render_to_vec` is a convenience for short
assets and refuses allocations above 64 MiB; stream longer timelines in fixed-size blocks.

## Guarantees

- `#![forbid(unsafe_code)]` in every shipped library crate.
- Parser caps of 1 MiB and 64 nesting levels before document-model construction.
- Stable validation stages and error codes from the pinned specification fixtures.
- Immutable preparation output; no JSON, schema traversal, sorting, or allocation while rendering.
- Canonical 48 kHz stereo binary64 DSP with deterministic PCG32 noise and mandatory final clipping.
- Portable subnormal flushing for bounded DSP cost on older application processors.
- Published engine ceilings for duration, layers, filters, contour entries, spatial effects,
  reverb/echo tails, echo delay state, and sample rate, including a combined wet-tail frame budget
  that bounds high-rate reverb preparation cost.

The renderer is portable library code, not a real-time audio thread or platform playback API. A
prepared plan may be shared, but each playback owns a separate mutable `Renderer`.

## Supported render profiles and limits

The supported application API always renders interleaved stereo. DSP arithmetic is binary64 through
the final hard clip; samples are converted to binary32 only for returned storage. Mono adaptation,
sample-rate conversion, device routing, and playback scheduling belong to the host and must happen
after Piccle clipping.

| Property                    | Published support                                                   |
| --------------------------- | ------------------------------------------------------------------- |
| Canonical profile           | 48 kHz, stereo, binary64 DSP, interleaved `f32` output              |
| Additional sample rates     | Every integer rate from 8,000 through 192,000 Hz                    |
| Output bandwidth            | 64,000 through 1,536,000 bytes/s before host/container overhead     |
| Document duration           | At most 600,000 ms                                                  |
| Layers / filters            | 128 layers; 16 serial filters per layer                             |
| Contour size                | 1,024 entries per individual contour                                |
| Spatial effects             | At most 16 parallel effects per document                            |
| Declared reverb tail        | At most 60,000 ms                                                   |
| Echo delay / effective tail | At most 2,000 ms per delay; at most 60,000 ms effective tail        |
| Nonzero wet preparation     | At most 2,880,000 tail frames (60 seconds at the canonical profile) |
| One-shot convenience output | At most 64 MiB through `Renderer::render_to_vec`                    |
| Streaming output            | Bounded caller-owned blocks through `Renderer::render_into`         |

These are acceptance and memory-safety ceilings, not a promise that the maximum document renders
live on every device. Hosts may render live, ahead of playback, cache output, or render offline.
Low-end-device live limits must be established from the workload and callback size; see the
[benchmark notes](https://github.com/dotpiccle/engine-rs/blob/main/BENCHMARKS.md).

## Workspace

| Crate             | Role                                                          |
| ----------------- | ------------------------------------------------------------- |
| `piccle`          | Supported application API and validation boundary             |
| `piccle-core`     | Document model, typed errors, curves, and frame rules         |
| `piccle-validate` | Strict JSON parser plus structural and semantic validation    |
| `piccle-dsp`      | Oscillators, deterministic noise, biquads, FDN reverb, echo   |
| `piccle-render`   | Immutable schedule and allocation-free production render loop |
| `piccle-fuzz`     | Detached libFuzzer targets for arbitrary untrusted bytes      |
| `xtask`           | Setup, conformance, benchmark, and spec-pin automation        |

The lower-level crates are published so the umbrella crate can depend on them, but direct use must
preserve the validation and engine-limit boundary documented by their APIs.

## Development and release gates

```bash
git clone --recurse-submodules https://github.com/dotpiccle/engine-rs
cd engine-rs
cargo setup
cargo nextest run --workspace --all-features
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
cargo audit
cargo xtask conformance --piccle-spec piccle-spec
```

See the repository's
[contribution guide](https://github.com/dotpiccle/engine-rs/blob/main/CONTRIBUTING.md),
[benchmark notes](https://github.com/dotpiccle/engine-rs/blob/main/BENCHMARKS.md), and
[release checklist](https://github.com/dotpiccle/engine-rs/blob/main/RELEASE_CHECKLIST.md). The
[conformance report](https://github.com/dotpiccle/engine-rs/blob/main/CONFORMANCE.md) records the
pinned specification and current evidence. Automated canonical conformance checks run in CI;
perceptual listening review and representative low-end Android measurements remain explicit release
activities.

For the ARMv7 release probe, connect one authorized Android device and run
`cargo xtask device-bench`; see the
[benchmark notes](https://github.com/dotpiccle/engine-rs/blob/main/BENCHMARKS.md) for prerequisites
and interpretation.

## Compatibility

Minimum Supported Rust Version: **1.85** (Rust 2024 edition). MSRV bumps are SemVer-minor breaking.

## License

MIT. See the [engine license](https://github.com/dotpiccle/engine-rs/blob/main/LICENSE) and
[specification license](https://github.com/dotpiccle/spec/blob/main/LICENSE).
