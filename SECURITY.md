# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in this engine, please report it privately by emailing
engineering@dotpiccle.com.

**Do not file a public issue.** The engine treats every Piccle document as untrusted input per
`spec/docs/11-engine-safety.md`. Malformed JSON, schema-invalid, semantically invalid, unsupported,
and internal failures are reported as distinct outcomes.

## Scope

- Parser: JSON parsing, UTF-8 decoding, duplicate member detection
- Validator: schema validation (Draft 2019-09), semantic validation
- Render path: DSP, reverb, output clipping (must not produce NaN/inf)
- Supply chain: dependency advisories (cargo-audit), license/bans/sources (cargo-deny)

## Security posture

- `#![forbid(unsafe_code)]` in every library crate (no `unsafe` can be added even with `#[allow]`)
- Supply-chain gates: cargo-deny (every PR) + cargo-audit (nightly) + dependabot (weekly)
- Fuzzing: cargo-fuzz targets in `crates/piccle-fuzz/`
- Secrets scanning: gitleaks runs on every PR via GitHub Action
