# AGENTS.md — Piccle Engine (`engine-rs`)

## 0. Mission

You are working on the **Piccle reference engine** written in Rust — the production implementation
of the Piccle micro-audio format at https://github.com/dotpiccle/spec.

Piccle is a declarative format for short, one-shot procedural UI sounds: button presses, toggles,
confirmations, errors, notifications, and navigation transitions. A Piccle asset contains structured
synthesis instructions (tones, deterministic noise, filters, reverb) rather than recorded audio.

This engine parses Piccle documents, validates them, and renders audio according to the normative
specification. It must be:

- **Spec-conformant** above all. Every normative requirement in `piccle-spec/docs/` (all chapters)
  is a hard contract. When this file and the spec disagree, the spec wins.
- **Secure.** Piccle documents are untrusted input per `piccle-spec/docs/11-engine-safety.md`.
- **Deterministic.** Same document + same render profile = same output (within the spec's
  determinism classes).
- **Extremely performant.** The render path MUST NOT allocate memory, parse JSON, walk schemas, sort
  events, or construct tables. Steady render cost scales with active voices; reverb is constant work
  per frame.
- **Portable.** No platform-specific concepts in the core library. Platform audio I/O is an
  integration concern. The engine targets application processors across Linux (x86_64, aarch64,
  armv7), macOS, Windows, WASM, Android, and iOS — see `deny.toml` `[graph] targets` for the
  canonical list and §5.10–5.11 for the portability matrix and future `#![no_std]` MCU path.

This repository does **not** define the Piccle format — that is the spec repository's job. If you
find ambiguity, do not invent behavior; report it as a specification defect.

## 1. Repository authority

When files disagree: spec normative docs > spec JSON Schema > spec test-vectors > spec examples >
engine docs > engine tests. Engine tests are conformance evidence, not authority.

## 2. What this repository owns

**In scope:** parsing Piccle JSON, validation (malformed/schema/semantic/unsupported/internal),
default resolution into an immutable render plan, canonical 48 kHz stereo binary64 rendering,
engine-specific public APIs, benchmarks, resource limits, conformance evidence.

**Out of scope:** the Piccle format itself (lives in `dotpiccle/spec`), JSON Schema authoring,
platform audio I/O as a normative concern, looping/modulation/gesture control (deferred beyond v1 by
the spec).

A user-facing `piccle-cli` binary is deferred — `xtask` covers the developer-facing conformance
runner.

## 3. Repository layout

```
engine-rs/
├── Cargo.toml                        (workspace root)
├── crates/
│   ├── piccle/                       umbrella library; re-exports public API
│   ├── piccle-core/                  document model, errors, curve primitives
│   ├── piccle-validate/              JSON parse + schema + semantic validation
│   ├── piccle-dsp/                   PCG32, oscillators, biquads, FDN reverb
│   ├── piccle-render/                boundary schedule + production render loop
│   ├── piccle-fuzz/                  cargo-fuzz targets
│   └── xtask/                        automation: setup, conformance, sync-spec, bench
├── piccle-spec/                      (git submodule, pinned)
├── .github/workflows/                CI/CD
├── .cargo/config.toml                cargo aliases (t, tcov, ci, setup, xtask)
├── deny.toml                         cargo-deny config
├── clippy.toml / rustfmt.toml        linter/formatter config
├── dprint.json                       Rust-native formatter (markdown/yaml/toml/json)
├── .cargo-husky/hooks/               project-local git hooks (pre-commit, pre-push, commit-msg)
└── .agents/skills/                   AI agent skills
```

**Spec-mandated invariant:** `piccle-render` MUST NOT depend on `piccle-validate` or any JSON/schema
crate. The render path consumes an immutable `RenderPlan`; it never sees raw JSON.

## 4. Spec conformance contract

### 4.1 Failure categories

Defined in [`piccle-spec/docs/14-conformance.md`](piccle-spec/docs/14-conformance.md) §Validation
stages. The error string codes in
[`piccle-spec/test-vectors/invalid-expectations.json`](piccle-spec/test-vectors/invalid-expectations.json)
are the canonical contract — they MUST NOT be renamed without a SemVer-major bump.

### 4.2 Canonical conformance profile

See [`piccle-spec/docs/11-engine-safety.md`](piccle-spec/docs/11-engine-safety.md) §Canonical
conformance profile for the canonical mode properties (sample rate, output channels, DSP precision,
output storage, and frame formula).

### 4.3 Required verification

The complete verification checklist is defined in
[`piccle-spec/docs/15-engine-build-guide.md`](piccle-spec/docs/15-engine-build-guide.md) §Engine
conformance verification (9 steps covering valid fixtures, invalid fixtures, DSP reference values,
oscillator spectral purity, control surface extremes, reverb cross-engine equivalence, finite
output, official examples, and profiling).

## 5. Engine design principles

### 5.1 Spec-conformant above all

Every normative requirement in the spec is a hard contract. Do not invent behavior. Do not optimize
away a requirement. If a normative rule is unclear, report it as a spec defect.

### 5.2 The render path is allocation-free

Per spec `docs/13-implementer-notes.md` §Render-loop discipline, during rendering the engine MUST
NOT parse JSON, walk the schema, sort events, search contour arrays from the beginning, allocate
memory, construct oscillator tables, or measure a reverb impulse response. All of that belongs in
the preparation phase.

### 5.3 Validation is the security boundary

The spec's [`piccle-spec/docs/11-engine-safety.md`](piccle-spec/docs/11-engine-safety.md) §Untrusted
input defines a 6-step pre-render pipeline that the engine MUST follow.
`piccle::prepare(&bytes) -> Result<RenderPlan, PiccleError>` is the ONLY way to reach the render
path. The render path takes a `&RenderPlan` and cannot fail validation.

### 5.4 DSP matches normative formulas exactly

The spec publishes exact formulas for PCG32, character filters, biquads, FDN reverb, oscillator
harmonic series, and curve functions. Implement them verbatim. Do not substitute `rand_pcg` for the
spec's exact PCG32 init sequence. Do not substitute a different reverb topology without passing the
spec's qualification matrix.

### 5.5 Portability across target platforms

| Crate             | Targets                     | std/no_std                                |
| ----------------- | --------------------------- | ----------------------------------------- |
| `piccle-core`     | all                         | std                                       |
| `piccle-validate` | application processors only | std (serde_json, jsonschema)              |
| `piccle-dsp`      | all                         | std; target #![no_std] in future refactor |
| `piccle-render`   | all                         | std; target #![no_std] in future refactor |

See `deny.toml` `[graph] targets` for the application-processor target list.

## 6. Coding conventions

These conventions adapt the best practices of engineering to Rust code.

### 6.1 Clarity over cleverness

Code must be boring, explicit, and easy to review. Use descriptive names. Avoid cleverness.

```rust
// Good
let render_plan = Validator::new().prepare(&document_bytes)?;

// Bad
let p = validate(b)?;
```

### 6.2 Named parameters for >2 arguments

If a function receives more than two arguments, use a struct parameter.

```rust
// Good
pub fn render_layer(&mut self, ctx: &RenderContext, params: &LayerRenderParams) -> Result<()>

// Bad
pub fn render_layer(&mut self, sample_rate: u32, layer: &Layer, start: u64, dur: u64) -> Result<()>
```

### 6.3 Guard clauses

Prefer early returns and guard clauses. Avoid deeply nested `if/else`.

```rust
fn frame_at(&self, time_ms: i64) -> Option<Frame> {
    if time_ms < 0 {
        return None;
    }

    if time_ms > self.duration_ms {
        return None;
    }

    Some(self.frame_offset(time_ms))
}
```

### 6.4 Type safety

- No `unsafe` (`#![forbid(unsafe_code)]` enforced via `[workspace.lints.rust]`)
- No `unwrap`/`expect`/`panic!` outside test code (enforced via clippy lints)
- Use `Result<T, E>` for fallible operations; propagate with `?`
- Use `thiserror` for library errors, `anyhow` for the CLI binary only

### 6.5 No dead code

Do not leave unused code, commented-out blocks, unused imports, or speculative abstractions. Every
line must exist for the current task.

### 6.6 Constants over magic numbers

Avoid magic numbers and repeated domain strings. Create named constants for sample rates, frequency
limits, PCG32 init values, FDN delay caps, resource limits, and error codes.

Spec-provided constants (PCG32 multiplier, FDN delay caps, character filter corner frequencies) MUST
keep their full binary64 precision — do not round digits.

### 6.7 Module organization

One file per module. Module files use the `module.rs` form (Rust 2024 edition), not `module/mod.rs`.
The umbrella `piccle` crate re-exports all library sub-crates.

### 6.8 Comment policy

- `///` doc comments on every public item. Missing docs are warned at compile time.
- Use inline `//` comments to explain _why_, not _what_ — the code already says what.
- Reference the spec section when implementing a normative formula:
  ```rust
  // piccle-spec/docs/11-engine-safety.md: frame(m) = floor(m × r / 1000 + 0.5)
  ```

## 7. Error handling

- Typed domain errors via `thiserror` with stable error codes matching
  `piccle-spec/test-vectors/invalid-expectations.json`
- Six variants: `ResourceRejected`, `Malformed`, `SchemaInvalid`, `SemanticInvalid`, `Unsupported`,
  `Internal`
- Do not leak internals (no stack traces, no internal IDs)
- Consistent shape: `{ code, path, msg }`

## 8. Testing protocol

### 8.1 One assertion per test

Every test must contain exactly one `assert_*` call. If a behavior requires multiple assertions,
split it into multiple tests with descriptive names.

### 8.2 Test-driven fix workflow

Write or update the test first. Confirm it fails for the expected reason. Implement the smallest
correct solution. Run the affected test suite. Run lint and typecheck.

### 8.3 Bug fixes require regression tests

Every bug fix must include a regression test that reproduces the bug, fails before the fix, passes
after the fix, and is named after the scenario.

### 8.4 Conformance tests

Drive every fixture in `piccle-spec/test-vectors/valid/` and `piccle-spec/test-vectors/invalid/`,
asserting stage/code/path against `piccle-spec/test-vectors/invalid-expectations.json`. Run with
`cargo xtask conformance`.

### 8.5 Snapshot tests (insta)

Snapshot the resolved document model for each `piccle-spec/examples/*.json` and the error report
(stage/code/path) for each invalid fixture.

### 8.6 Property tests (proptest)

Generate random valid documents and assert render invariants: finite output, exact frame count, zero
allocations in the render loop.

### 8.7 DSP measurement tests

Per spec `docs/03-sources.md`, test every oscillator at canonical measurement frequencies (375,
1000, 3000, 8000, 16000 Hz) using the spec's DFT procedure (rectangular N=48000 window).

## 9. Performance rules

- **No-alloc render path** — enforce with a counting allocator test (`#[global_allocator]` wrapper
  that asserts zero `alloc` calls during rendering)
- **Constant reverb work** — FDN baseline has ~1,570 samples total delay regardless of `tail_ms`.
  Bench `tail_ms` ∈ {1, 10, 20, 220, 500} — per-frame cost must be flat
- **No cost spike at contour boundaries** — contour cursors advance forward, no array searching
- **Benchmarks** — `criterion` with statistical regression detection; profile with `samply` or
  `cargo flamegraph` on release builds
- **Release profile** — `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`

## 10. Security, trust & safety

### 10.1 `#![forbid(unsafe_code)]`

Every library crate has `#![forbid(unsafe_code)]` — NOT `deny`, `forbid`. This means `unsafe` cannot
be added even with `#[allow]`; it's a hard compile error.

### 10.2 Supply chain

- `cargo-deny` runs on every PR (advisories, licenses, bans, sources)
- `cargo-audit` runs nightly (RUSTSEC advisories)
- `dependabot` opens weekly PRs (cargo, github-actions, submodule)
- `gitleaks` runs on every PR via GitHub Action (secrets scanning)

### 10.3 Fuzzing

`piccle-fuzz` contains cargo-fuzz targets for the parser. Feed arbitrary bytes to
`piccle_validate::Validator::check(bytes)` and assert it returns `Ok` or `Err` — never panics, never
hangs, never allocates unbounded memory.

### 10.4 Untrusted input

Defined in [`piccle-spec/docs/11-engine-safety.md`](piccle-spec/docs/11-engine-safety.md) §Untrusted
input (6-step pre-render pipeline). Validation produces a `RenderPlan`. The render path takes a
`&RenderPlan` and cannot fail.

### 10.5 Denormal protection

Prevent subnormal values from causing unbounded DSP cost. Flush-to-zero, explicit state floors, or
equivalent. Must not produce output above −180 dBFS and must not change a declared timeline
boundary.

### 10.6 Finite output and clipping

Every DSP stage MUST produce finite samples. NaN/inf stops rendering and reports
`PiccleError::Internal`. The final hard clipper (spec `docs/08-output.md`) is mandatory and is the
last normative DSP stage.

## 11. Git & change discipline

### 11.1 Before making changes

0. If a fresh clone, run `cargo setup` first.
1. Understand the requested change. Ask clarifying questions if ambiguous (see
   `.agents/skills/ask-questions-if-underspecified`).
2. Read the relevant spec section in `piccle-spec/docs/`.
3. Inspect existing files and patterns.
4. Check `.agents/skills/` for relevant skills.
5. Make the smallest safe change.

### 11.2 During changes

- Keep changes focused. Do not refactor unrelated code.
- Do not add speculative abstractions.
- Do not silently change public API contracts.
- Update tests and docs with code changes.
- Follow the test-driven workflow (§8.2).
- One assertion per test (§8.1).

### 11.3 Before finishing

```bash
cargo nextest run -p <affected-crate>
cargo nextest run --workspace --all-features
cargo +nightly fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo deny check
cargo audit
typos
cargo xtask conformance --piccle-spec piccle-spec    # if validation/render changed
```

## 12. Open source readiness

Open-source from day one. No secrets, no embarrassing shortcuts, clear docs, clean architecture,
meaningful tests, no hidden local assumptions. Treat anti-malicious-input heuristics and security
controls as especially sensitive.

## 13. Language & naming

- Product: **Piccle**. Use the brand name in human-facing docs.
- Domain language: use spec terms verbatim (document, layer, source, tone, noise, filter, reverb,
  balance, volume, contour, frame, boundary, pitch, frequency, character, seed, offset_cents,
  master_volume_level, soften_hz, tail_ms).
- Rust naming: `UpperCamelCase` for types/traits/enums, `snake_case` for
  functions/methods/variables/modules, `SCREAMING_SNAKE_CASE` for constants.
- Error code strings match `piccle-spec/test-vectors/invalid-expectations.json` exactly, including
  case. They are part of the public API.

## 14. Agent behavior rules

1. Follow this file over generic preferences.
2. Prefer simple, boring, production-quality code.
3. Ask clarifying questions only when truly ambiguous.
4. Do not invent product requirements.
5. Do not add dependencies without a clear reason. Every dep is a security surface; run
   `cargo deny check` after adding.
6. Do not change architecture casually — the crate graph enforces spec boundaries.
7. Do not bypass tests to make a task appear complete.
8. Do not hide failures — fix the code or fix the test.
9. If something cannot be done safely, explain the tradeoff.
10. Optimize for: conformant behavior, fast rendering, safe input handling, clear error reporting,
    portability, low latency.
11. Always update this file when modifying external-facing behavior.

## 16. Subdirectory instructions

Subdirectories may contain their own `AGENTS.md` files when they develop specialized workflows.
Apply root instructions first, then the nearest relevant subdirectory instructions. Do not copy
large sections of this file into nested instruction files.

## 17. Final principle

> Conformant rendering, safe validation, portable playback, and perceptually expressive micro-audio.

Every technical decision should support one of these outcomes. When forced between a clever
optimization and a spec-conformant implementation, choose the spec. When forced between a clever API
and a clear API, choose the clear one.
