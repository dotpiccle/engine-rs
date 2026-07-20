# Benchmarks

Run with: `cargo xtask bench`

Run the release-profile ARMv7 probe on one authorized Android device with:

```bash
cargo xtask device-bench
```

This requires `cargo-ndk`, ADB, Android NDK API 21 support, and exactly one connected device. It
builds an `armeabi-v7a` executable, pushes it to `/data/local/tmp`, and reports preparation time,
renderer construction time, throughput, real-time factor, worst 128-frame callback latency and its
ratio to average callback time, and process peak RSS. It profiles all 15 pinned official examples,
the 20 Hz oscillator risk case, a moving-filter workload, and the published maximum workload. The
header records the device serial, model, Android version, ABI, and total RAM so results retain their
hardware context.

## Performance invariants

The Piccle spec mandates (`piccle-spec/docs/15-engine-build-guide.md` §9):

> "steady render cost should scale with active voices and their declared filters. Reverb should add
> constant work per frame rather than work proportional to `tail_ms`." "Verify that steady rendering
> performs no memory allocation and has no cost spike when a contour boundary is crossed."

## Current benchmark groups

- Active-voice scaling at 1, 8, 32, and 64 simultaneous voices
- Inactive-voice gating against a document with 64 future layers
- Simultaneous pitch-contour advancement for all 128 active voices, comparing the exact boundary
  frame with the immediately following steady frame
- Reverb cost per frame at `tail_ms` ∈ {1, 10, 20, 220, 500}
- Echo cost per frame at `delay_ms` ∈ {20, 90, 200, 2000}
- Reverb preparation at `tail_ms` ∈ {1, 20, 220, 500}
- Oscillator harmonic load, including the 20 Hz saw worst case
- Maximum accepted steady workload: 128 voices × 16 filters plus reverb

Allocation and reallocation checks are deterministic tests in
`crates/piccle-render/tests/no_alloc.rs`, including maximum supported voices and filters.

### 2026-07-19 audit snapshot

Criterion medians on the local arm64 macOS audit host, rendering 4,096-frame chunks:

| Case                              | Throughput           |
| --------------------------------- | -------------------- |
| No active voices                  | 514.2 Mframe/s       |
| 64 future inactive voices         | 530.7 Mframe/s       |
| One 440 Hz sine                   | 118.1 Mframe/s       |
| One 440 Hz saw                    | 102.3 Mframe/s       |
| One 20 Hz saw (worst case)        | 102.7 Mframe/s       |
| One 20 Hz square / triangle       | 101.6–102.4 Mframe/s |
| Reverb, `tail_ms` 1 through 500   | 21.1–21.8 Mframe/s   |
| Echo, `delay_ms` 20 through 2,000 | 88.0–89.6 Mframe/s   |
| 128 simultaneous contour advances | 1.060 µs/frame       |
| Same 128 voices, following frame  | 0.998 µs/frame       |
| 128 voices × 16 filters + reverb  | 75.4 Kframe/s        |

Reverb preparation medians on the same host are 48.8 µs (1 ms), 633 µs (20 ms), 6.58 ms (220 ms),
and 14.8 ms (500 ms). Production preparation retains one binary64 energy value per harness frame:
about 22 MiB at the 60-second resource ceiling, down from three frame-sized binary64 buffers (about
66 MiB). The uncapped FDN additionally retains delay state proportional to `tail_ms` (about 3.7 MiB
at 60 seconds and 48 kHz); its dense 8-by-8 feedback multiply has constant per-frame work. The
public 2,880,000-tail-frame preparation ceiling keeps these frame-derived maxima constant at higher
sample rates instead of allowing a 192 kHz profile to require four times the scratch/state.
Reference-IR generation deliberately allocates stereo capture buffers in addition to preparation
state; it is conformance tooling, not the application path. The ignored release-profile ceiling gate
prepared a 60-second tail in 1.77 seconds on this host.

The dense orthogonal matrix required by the updated spec reduces local reverb throughput by roughly
one third versus the previous Walsh-Hadamard topology, while remaining about 400 times the 48 kHz
real-time rate on this host. Exact-harmonic-count saw and square tables make oscillator work
independent of retained harmonic count: the 20 Hz saw improved by roughly 80× from the prior 1.28
Mframe/s result. Each waveform family initializes independently and occupies less than 5 MiB. These
tables all initialized together in about 20 ms in the release-profile audit test. Advancing one
pitch-contour segment on all 128 active voices measured 1.060 µs versus 0.998 µs on the immediately
following frame in the isolated confirmation run: about 0.062 µs (6.2%) bounded overhead, with no
allocation or unbounded search. These numbers demonstrate inactive-voice gating, bounded boundary
work, and tail-independent steady reverb and oscillator arithmetic. They are not a substitute for
the required ARMv7 device measurements. The maximum accepted workload is only about 1.57× the 48 kHz
real-time rate even on this host. A 128-frame callback probe measured about 1.45× aggregate real
time but a 3.83 ms worst callback, which exceeds the 2.67 ms callback period. The published resource
ceilings are therefore safe offline acceptance limits, not a live low-end-device guarantee. The
official examples are far lighter (at most four layers and one filter per layer), but still require
device evidence.

Still required before the first production performance claim: run the complete probe on the actual
lowest supported Android ARMv7 device and preserve its results with the release evidence.

### 2026-07-20 secondary Android snapshot

The release ARMv7/API-21 probe also ran successfully on a Galaxy S20 FE (`SM-G780F`, Android 13,
5,590,964 KiB RAM). The phone reports an `arm64-v8a` primary ABI, while the deployed executable is
the same 32-bit `armeabi-v7a` artifact intended for the minimum Android profile.

| Workload                                    | Preparation     | Real-time factor | Worst 128-frame callback | Peak RSS        |
| ------------------------------------------- | --------------- | ---------------- | ------------------------ | --------------- |
| One 20 Hz saw                               | 58.727 ms       | 346.104×         | 6.885 µs                 | 8,144 KiB       |
| Four voices and one moving filter           | 53.447 ms       | 36.933×          | 90.231 µs                | 8,424 KiB       |
| Fourteen pre-echo examples (observed range) | 0.042–42.630 ms | 79.197–294.702×  | 10.500–73.538 µs         | 8,424–8,672 KiB |
| 128 voices × 16 filters plus reverb         | 53.179 ms       | 0.536×           | 5,024.269 µs             | 9,084 KiB       |

Every official example and the representative moving-filter workload rendered comfortably ahead of
real time. The intentionally maximal accepted workload did not sustain live real time, confirming
that the resource ceiling is an offline/ahead-of-playback acceptance limit. This modern phone is
useful deployment-path evidence, but it does not satisfy the lowest-supported-device gate or justify
a Galaxy J5 performance claim. This snapshot predates the echo example; rerun the 15-example probe
before making Android echo-performance claims.

## Profiling

- `cargo flamegraph --bench render` for flamegraph visualization
- `samply` for modern Linux/macOS profiling (release builds with `debug = "line-tables-only"`)
