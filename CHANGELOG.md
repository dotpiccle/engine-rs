# Changelog

All notable changes to the piccle engine are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial workspace scaffolding with `piccle`, `piccle-core`, `piccle-validate`, `piccle-dsp`,
  `piccle-render`, `piccle-fuzz`, and `xtask` crates (7 total).
- Spec submodule pinned at `main` HEAD (commit `d079d11`; no tags exist yet on the spec repo).
- CI pipeline: fmt, clippy (feature powerset), test (3 OSes), MSRV, cross-check (Linux ARM64/ARMv7,
  WASM, Android ARM64/ARMv7, iOS ARM64/sim), audit, docs, typos, dprint (format check), gitleaks
  (secrets scanning), conformance, fuzz-smoke.
- `deny.toml` targets list covers all application-processor platforms supported (Linux
  x86_64/aarch64/armv7, macOS, Windows, WASM, Android, iOS). MCU-class `#![no_std]` targets are
  documented as future work.
- `cargo setup` alias for project onboarding (installs hooks via cargo-husky, auto-installs missing
  Rust tools via `cargo install`, syncs spec submodule).
- Configuration: `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.cargo/config.toml`, `.typos.toml`,
  `dprint.json` (Rust-native formatter for markdown/yaml/toml/json).
- Documentation: `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, `RELEASE_CHECKLIST.md`,
  `BENCHMARKS.md`.
- Project-local git hooks via cargo-husky (pre-commit, pre-push, commit-msg).

### Security

- `#![forbid(unsafe_code)]` enforced in every library crate via `[workspace.lints.rust]`.
- Supply-chain gates: cargo-deny (advisories, licenses, bans, sources) and cargo-audit (RUSTSEC) run
  in CI.
- gitleaks runs in CI via GitHub Action (secrets scanning).
- `#![forbid(unsafe_code)]` in every library crate.
