# Changelog

All notable changes to the piccle engine are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

No unreleased changes.

## [1.0.0] - 2026-07-21

### Added

- Initial workspace scaffolding with `piccle`, `piccle-core`, `piccle-validate`, `piccle-dsp`,
  `piccle-render`, `piccle-fuzz`, and `xtask` crates (7 total).
- Specification submodule pinned to the stable `v1.0.1` tag at commit `b8797cd` with no
  branch-tracking hint.
- CI pipeline: fmt, clippy (feature powerset), test (3 OSes), MSRV, cross-check (Linux ARM64/ARMv7,
  WASM, Android ARM64/ARMv7, iOS ARM64/sim), audit, docs, typos, dprint (format check), gitleaks
  (secrets scanning), conformance, fuzz-smoke.
- `deny.toml` targets list covers all application-processor platforms supported (Linux
  x86_64/aarch64/armv7, macOS, Windows, WASM, Android, iOS). MCU-class `#![no_std]` targets are
  documented as future work.
- `cargo setup` alias for deterministic project onboarding: configures repository-owned hooks,
  checks prerequisites and targets, and syncs the pinned spec submodule.
- Configuration: `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.cargo/config.toml`, `.typos.toml`,
  `dprint.json` (Rust-native formatter for markdown/yaml/toml/json).
- Documentation: `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, `RELEASE_CHECKLIST.md`,
  `BENCHMARKS.md`.
- Project-local Git hooks via `core.hooksPath` (pre-commit, pre-push, commit-msg).
- `cargo xtask device-bench` builds a standalone API-21 `armeabi-v7a` probe, deploys it through ADB,
  and profiles all official examples plus synthetic risk/ceiling workloads, reporting preparation,
  throughput, callback-spike, and process peak-RSS evidence.
- `CONFORMANCE.md` records the pinned specification revision, automated evidence, published profile,
  resource contract, and external release gates.
- An isolated worst-case contour-boundary benchmark compares simultaneous cursor advancement across
  128 active voices with the following steady frame.
- Conformance checks apply the specification's exact scaled-epsilon bound to `sin`/`cos`-derived
  balance and filter aids.
- Root-level parallel `spatial_effects` with normative additive reverb and lowpass-feedback echo,
  deterministic echo-tail scheduling, validation fixtures, and canonical impulse checkpoints.
- Conformance progress output for every fixture and explicit `RUN` markers before CPU-heavy DFT,
  reference-generation, spatial-effect, and example-render phases.
- Deterministic seed-0 reverb differential qualification across 100 randomly sampled valid
  configurations, as required by the stable specification.

### Changed

- The `piccle` umbrella API now exposes an opaque validated `RenderPlan`; low-level plan compilation
  is explicitly named `compile_validated`.
- Added a 192 kHz render-profile ceiling and a 64 MiB `render_to_vec` allocation ceiling. Longer
  timelines remain available through allocation-free streaming.
- Release workflows pin third-party actions to immutable commits and publish the workspace in
  dependency order with restart-safe crates.io visibility checks.
- Reverb now follows the updated reference topology: uncapped proportional FDN delays and a
  configuration-seeded dense orthogonal feedback matrix.
- Reverb configuration seeds now use the language-neutral wrapping formula from the normative FDN.
- Reverb conformance now covers the mandatory 10-case qualification matrix and all 40 noncanonical
  combinations of the five canonical tails with the representative 8–192 kHz profile rates, plus the
  stable specification's mandatory 100-case property pass.
- Directional fade conformance now recomputes every published half-duration fade-in and fade-out
  checkpoint, protecting the specified endpoint order for all five curves.
- Spatial effects are compiled into a canonical order so parallel output is independent of JSON
  array order, including binary32 storage results.

### Fixed

- Accept both tab-separated and space-aligned `adb devices -l` output in the on-device benchmark
  runner, including the format emitted by Android platform-tools 37.
- Make the packaged reverb-matrix drift test compare parsed JSON so Windows checkout line endings
  cannot produce a false conformance failure.
- Install `cargo-deny` and `cargo-audit` independently in CI and release jobs, and replace the
  organization-licensed Gitleaks action with a checksum-pinned official Gitleaks binary.
- Retain the normative triangle oscillator's 27th harmonic.
- Classify finite integer literals beyond `u64` at the schema stage instead of as parse overflow.
- Distinguish misspelled non-finite tokens from exact `NaN` and infinity tokens.
- Stop rendering when a non-finite post-master sample occurs, including documents without active
  reverb.
- Handle degenerate low-level terminal-window arguments without producing `NaN`.
- Correct reverb decay calibration for the updated spec and regenerated canonical IR fixtures.
- Make the conformance runner enforce the normative reverb RT60 window and distinguish optional
  same-platform bit-identity evidence from the cross-platform perceptual-equivalence contract.
- Make oscillator conformance classify absent waveform harmonics as unwanted components instead of
  exempting every integer multiple of the fundamental.
- Implement and enforce all seven normative reverb perceptual-equivalence metrics, including the
  corrected `2 × M` modal-analysis window and reference-qualified `-30 dB` quality gate.
- Scale reverb metric FFTs to the next power of two for responses longer than 65,536 frames, and
  exercise the spec generator's noncanonical 37 ms / 8 kHz / 44.1 kHz configuration.
- Verify the normative reverb seed, PCG32 stream, source matrix, and feedback matrix against the
  published noncanonical matrix vector.
- Make every published crate archive self-contained: ship per-crate license files, embed the
  normative reverb matrix vector used by unit tests, and exclude repository-only integration tests
  that require the pinned specification submodule.
- Make an unconfigured low-level oscillator emit silence instead of panicking in debug builds, and
  remove the unreachable panic branch from curve evaluation.
- Remove automatic specification-submodule updates so a stable engine release cannot drift from the
  explicitly reviewed specification tag.
- Make the pre-push test fallback depend on nextest availability rather than a failed nextest run,
  so a real test failure cannot be retried under a different runner and accidentally hidden.
- Correct the non-additive 44.1 kHz reverb-tail aid and compare Gram-Schmidt matrix normalization
  within the published cross-platform binary64 tolerance.

### Performance

- Replace per-sample harmonic-series evaluation with exact-harmonic-count band-limited table banks.
  Steady oscillator work is now constant per frame; the 20 Hz saw improved by roughly 80× on the
  audit host while remaining inside every normative spectral tolerance. Tables are initialized per
  waveform family and use less than 5 MiB each.
- Render only active layers using a pre-sorted boundary schedule while preserving document-order
  summation.
- Remove modulo division from reverb circular-buffer advancement and flush subnormal recursive state
  portably at a bounded maintenance cadence.
- Reuse one frame-energy buffer during reverb calibration, reducing production preparation scratch
  memory from three binary64 values per harness frame to one.
- Skip wet-path calibration and state construction when the declared reverb amount is zero while
  preserving the spec-defined output timeline.
- Remove JSON and serialization dependencies from the render-side crate graph.
- Keep echo delay work constant per frame across representative 20–2,000 ms delay lengths and box
  only the large reverb plan/state variants to avoid inflating echo-only plan memory.

### Security

- `#![forbid(unsafe_code)]` enforced in every library crate via `[workspace.lints.rust]`.
- Bound nonzero reverb preparation to 2,880,000 tail frames. This preserves a 60-second canonical
  tail while rejecting high-rate configurations before they multiply calibration memory and CPU; an
  amount-zero reverb remains timeline-only and does not consume the wet-state budget.
- Bound documents to 16 parallel spatial effects, 2,000 ms per echo delay line, and a 60-second
  effective echo tail after format validation.
- Require at least 90% library line coverage in CI; the release audit measured 95.19%.
- Harden tagged releases by rejecting commits outside `main`, verifying the complete multi-crate
  archive set, serializing same-tag runs, and isolating GitHub write access to the final
  release-creation job. The `v1.0.0` tag creates a source release without crates.io credentials;
  registry publication remains a separate future operation.
- Supply-chain gates: cargo-deny (advisories, licenses, bans, sources) and cargo-audit (RUSTSEC) run
  in CI.
- A checksum-pinned Gitleaks binary scans the complete history in CI.
- Parser fuzzing completed more than 7.9 million seeded executions without a crash during the
  release audit.

[Unreleased]: https://github.com/dotpiccle/engine-rs/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/dotpiccle/engine-rs/releases/tag/v1.0.0
