# Conformance Report

This report records the release-candidate evidence for the Piccle Rust reference engine. The engine
revision is the commit containing this file.

## Specification under test

- Repository: `https://github.com/dotpiccle/spec`
- Pinned commit: `e49c5edff3c102ed665fc8e7eb011a1d98eafbc2`
- Audit date: 2026-07-20
- Canonical profile: 48 kHz, stereo, binary64 DSP, interleaved binary32 output after clipping

The submodule commit is the conformance contract. Release automation checks out submodules
recursively and runs `cargo xtask conformance --piccle-spec piccle-spec` before publishing.

## Automated evidence

The audited worktree produced the following results:

| Gate                            | Evidence                                                                                                                                                                                             |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Specification repository        | `scripts/validate.py` passed 55 accepted and 64 rejected documents plus numeric, documentation, inventory, canonical-JSON, anchor, and link checks                                                   |
| Valid documents                 | Every one of the 40 files currently in `test-vectors/valid/` accepted before engine limits                                                                                                           |
| Invalid documents               | All 64 fixtures matched their exact stage, code, and JSON path                                                                                                                                       |
| Numeric and schedule references | PCG32, curves, balance, filters, frame boundaries, FDN construction, and behavior schedules passed                                                                                                   |
| Oscillators                     | All four waveforms passed the normative 48,000-frame DFT checks at 375, 1,000, 3,000, 8,000, and 16,000 Hz                                                                                           |
| Reverb                          | All seven perceptual metrics passed for 5 canonical IRs, 10 qualification cases, and 40 additional-profile cases spanning 8–192 kHz; canonical IRs were additionally bit-identical on the audit host |
| Echo and parallel effects       | All canonical echo schedule/checkpoint tolerances passed; reversed reverb/echo fixture orders produced identical PCM and output length                                                               |
| Matrix construction             | The noncanonical 37 ms / 8 kHz-soften matrix seed, PCG32 stream, source matrix, and feedback matrix matched the published exact-or-tolerant contract                                                 |
| Official examples               | All 15 examples prepared and rendered with exact frame counts and finite output                                                                                                                      |
| Render safety                   | Determinism, clipping, finite output, bounded streaming, and zero allocation/reallocation/deallocation in the render loop passed                                                                     |
| Workspace tests                 | 217 tests passed; one explicit long-running release-ceiling test remained ignored by default                                                                                                         |
| Static quality                  | Rustfmt, dprint, typos, strict Clippy, every feature power set, Rust 1.85 MSRV, rustdoc, and doctests passed; library line coverage measured 95.19%                                                  |
| Targets                         | Linux x86-64/aarch64/armv7, Android aarch64/armv7, macOS, Windows, WASM, and iOS targets are represented in policy; available cross-target checks passed                                             |
| Android linkage                 | The API-21 `armeabi-v7a` device probe linked as a 32-bit ARM EABI5 PIE executable                                                                                                                    |
| Secondary Android probe         | The ARMv7/API-21 probe completed on a Galaxy S20 FE: official examples sustained 79.197–294.702× real time at 8.2–8.5 MiB RSS; the maximum accepted workload was offline-only at 0.536×              |
| Supply chain                    | `cargo deny check` passed and `cargo audit` found no vulnerability in 112 locked dependencies against 1,166 advisories                                                                               |
| Fuzzing                         | The parser completed more than 7.9 million seeded libFuzzer executions without a crash during this audit                                                                                             |
| Packaging                       | Every published crate archive contains its license and README, excludes submodule-dependent integration tests, and includes required runtime test data                                               |

The conformance runner reported 659 successful automated checks. Repository tests supplement that
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

## Release gates that require external evidence

The automated implementation is a release candidate, not yet a completed stable release, until all
of the following are recorded:

- run `cargo xtask device-bench` on the lowest supported ARMv7 Android device, including every
  official example and the maximum workload;
- preserve throughput, 128-frame callback latency, callback-spike ratio, and process peak RSS;
- perform listening review on neutral headphones, full-range speakers, a small-device speaker, and
  the lowest-bandwidth supported output path;
- A/B review wet onset, echo density, early/late energy, stereo decorrelation, brightness, decay,
  metallic ringing, and discrete echoes; and
- recheck the specification issue tracker immediately before tagging. No normative specification
  issues were open at the time of this report.

Do not tag or publish a stable release while any applicable item in `RELEASE_CHECKLIST.md` remains
unchecked.
