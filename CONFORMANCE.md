# Conformance Report

This report records the `v1.0.0` release evidence for the Piccle Rust reference engine. The engine
revision is the commit containing this file.

## Specification under test

- Repository: `https://github.com/dotpiccle/spec`
- Tag: `v1.0.1`
- Pinned commit: `b8797cd459c743daadb746ab904903f714af27f3`
- Schema SHA-256: `58bbd0946fa5c8e7175866f7a48b4afcd5ef00b1f3c9b29ee8197b396f55ceb4`
- Audit date: 2026-07-21
- Canonical profile: 48 kHz, stereo, binary64 DSP, interleaved binary32 output after clipping

The submodule commit is the conformance contract. Release automation checks out submodules
recursively and runs `cargo conformance --piccle-spec piccle-spec` before creating the GitHub
Release. The `v1.0.0` source release does not publish crates to crates.io.

## Automated evidence

The audited worktree produced the following results:

| Gate                            | Evidence                                                                                                                                                                                                                                                        |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Specification repository        | `scripts/validate.py` passed 55 accepted and 64 rejected documents plus numeric, documentation, inventory, canonical-JSON, anchor, and link checks; Python 3.9, 3.12, and 3.14 hosted validation passed on tagged commit `b8797cd`                              |
| Valid documents                 | Every one of the 40 files currently in `test-vectors/valid/` accepted before engine limits                                                                                                                                                                      |
| Invalid documents               | All 64 fixtures matched their exact stage, code, and JSON path                                                                                                                                                                                                  |
| Numeric and schedule references | PCG32, curves, balance, filters, frame boundaries, FDN construction, and behavior schedules passed                                                                                                                                                              |
| Oscillators                     | All four waveforms passed the normative 48,000-frame DFT checks at 375, 1,000, 3,000, 8,000, and 16,000 Hz                                                                                                                                                      |
| Reverb                          | All seven perceptual metrics passed for 5 canonical IRs, 10 qualification cases, 40 additional-profile cases spanning 8–192 kHz, and a deterministic 100-case property pass using PCG32 seed 0; canonical IRs were additionally bit-identical on the audit host |
| Echo and parallel effects       | All canonical echo schedule/checkpoint tolerances passed; reversed reverb/echo fixture orders produced identical PCM and output length                                                                                                                          |
| Matrix construction             | The noncanonical 37 ms / 8 kHz-soften matrix seed, PCG32 stream, source matrix, and feedback matrix matched the published exact-or-tolerant contract                                                                                                            |
| Official examples               | All 15 examples prepared and rendered with exact frame counts and finite output                                                                                                                                                                                 |
| Render safety                   | Determinism, clipping, finite output, bounded streaming, and zero allocation/reallocation/deallocation in the render loop passed                                                                                                                                |
| Workspace tests                 | 221 tests passed; one explicit long-running release-ceiling test remained ignored by default                                                                                                                                                                    |
| Static quality                  | Rustfmt, dprint, typos, strict Clippy, every feature power set, Rust 1.85 MSRV, rustdoc, and doctests passed; library line coverage measured 95.19%                                                                                                             |
| Targets                         | Linux x86-64/aarch64/armv7, Android aarch64/armv7, macOS, Windows, WASM, and iOS targets are represented in policy; available cross-target checks passed                                                                                                        |
| Android linkage                 | The API-21 `armeabi-v7a` device probe linked as a 32-bit ARM EABI5 PIE executable                                                                                                                                                                               |
| Representative Android probe    | The ARMv7/API-21 probe completed on a Galaxy S20 FE: all 15 official examples sustained 78.825–396.664× real time at 8.3–8.6 MiB RSS; the maximum accepted workload was offline-only at 0.536×                                                                  |
| Supply chain                    | `cargo deny check` passed and `cargo audit` found no vulnerability in 110 locked dependencies against 1,166 advisories                                                                                                                                          |
| Fuzzing                         | The parser completed more than 7.9 million seeded libFuzzer executions without a crash during this audit                                                                                                                                                        |
| Packaging                       | Every published crate archive contains its license and README, excludes submodule-dependent integration tests, and includes required runtime test data                                                                                                          |

The conformance runner reported 1,472 successful automated checks. Repository tests supplement that
runner with public-boundary, property, allocation, packaging, and regression coverage.

## Engine profile and capacity contract

The umbrella `piccle` crate is the supported application boundary. It accepts canonical 48 kHz and
additional integer sample rates from 8 kHz through 192 kHz. All profiles use stereo binary64 DSP and
convert to interleaved `f32` only after the final hard clip.

Engine-specific limits are applied only after format validation and are returned as `Unsupported`:

- 1 MiB input and 64 JSON nesting levels at the parser boundary;
- 600,000 ms document duration;
- 128 layers and 16 filters per layer;
- 1,024 entries per contour;
- 16 parallel spatial effects;
- 60,000 ms declared reverb tail;
- 2,000 ms echo delay and 60,000 ms effective echo tail;
- 2,880,000 prepared tail frames for a nonzero wet path;
- 8,000 through 192,000 Hz render rates; and
- 64 MiB for the optional one-shot `render_to_vec` allocation.

The combined wet-tail frame ceiling prevents high sample rates from multiplying reverb calibration
scratch and CPU cost. It preserves the 60-second ceiling at 48 kHz and leaves amount-zero reverb
timelines streamable without constructing wet state.

These limits bound acceptance; they do not declare the maximum document live-real-time on every
processor. Hosts may render live, ahead of playback, cache, or render offline. The engine does not
own platform audio I/O, resampling, channel routing, or mono downmix.

## Qualification limitations and external evidence

The automated implementation evidence does not substitute for the following external qualification:

- run `cargo xtask device-bench` on each available representative Android device used for release
  claims, including every official example and the maximum workload;
- preserve throughput, 128-frame callback latency, callback-spike ratio, and process peak RSS;
- perform listening review on neutral headphones, full-range speakers, a small-device speaker, and
  every output profile for which a perceptual claim is made;
- A/B review wet onset, echo density, early/late energy, stereo decorrelation, brightness, decay,
  metallic ringing, and discrete echoes; and
- recheck the specification issue tracker immediately before tagging.

The connected Galaxy S20 FE is the available representative mobile deployment probe. No Galaxy J5 or
equivalent low-end device was available during this release audit, so no performance claim is made
for that hardware. The published maximum accepted workload remains classified as offline; resource
acceptance is not a promise of live rendering.

Specification issue [dotpiccle/spec#25](https://github.com/dotpiccle/spec/issues/25) was resolved in
`v1.0.1`: the two `pow`-derived exponential fade checkpoints use the same tight `8 × epsilon` scaled
tolerance as the other permitted transcendentals. The engine's formula and conformance gate now
match the tagged qualification contract without deviation.
