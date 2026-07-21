# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 1.x     | Yes       |
| < 1.0   | No        |

Security fixes are applied to the latest `1.x` release. Until crates.io publication is separately
announced, use the corresponding immutable Git tag as the dependency source.

## Reporting a vulnerability

If you discover a security vulnerability in this engine, please report it privately by emailing
engineering@dotpiccle.com.

**Do not file a public issue.** The engine treats every Piccle document as untrusted input per
`piccle-spec/docs/11-engine-safety.md`. Malformed JSON, schema-invalid, semantically invalid,
unsupported, and internal failures are reported as distinct outcomes.

## Scope

- Parser: JSON parsing, UTF-8 decoding, duplicate member detection
- Validator: v1 JSON Schema-equivalent structural validation, semantic validation
- Render path: DSP, reverb, output clipping (must not produce NaN/inf)
- Supply chain: dependency advisories (cargo-audit), license/bans/sources (cargo-deny)

## Security posture

- `#![forbid(unsafe_code)]` in every library crate (no `unsafe` can be added even with `#[allow]`)
- Preparation limits are checked after format validation and before render-resource allocation;
  nonzero reverb is capped at 2,880,000 prepared tail frames so high sample rates cannot multiply
  calibration scratch and CPU cost beyond the canonical 60-second budget
- Supply-chain gates: cargo-deny (every PR) + cargo-audit (nightly) + dependabot (weekly)
- Fuzzing: cargo-fuzz targets in `crates/piccle-fuzz/`
- Secrets scanning: gitleaks runs on every PR via GitHub Action
