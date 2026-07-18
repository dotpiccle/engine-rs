# Piccle Engine (engine-rs)

A portable, secure, deterministic Rust implementation of the
[Piccle](https://github.com/dotpiccle/spec) micro-audio format.

Piccle is a declarative format for short, one-shot procedural UI sounds, button presses, toggles,
confirmations, notifications, and navigation transitions. This engine parses Piccle documents,
validates them, and renders audio according to the normative specification.

## Quick start

```bash
git clone --recurse-submodules https://github.com/dotpiccle/engine-rs
cd engine-rs
cargo setup
cargo test --workspace
cargo xtask conformance
```

## Crate layout

| Crate             | Description                                            |
| ----------------- | ------------------------------------------------------ |
| `piccle`          | Umbrella library; re-exports public API                |
| `piccle-core`     | Document model, errors, curve primitives               |
| `piccle-validate` | JSON parsing, schema + semantic validation             |
| `piccle-dsp`      | PCG32 noise, oscillators, biquads, FDN reverb          |
| `piccle-render`   | Boundary schedule, render plan, production render loop |
| `piccle-fuzz`     | cargo-fuzz targets for security testing                |
| `xtask`           | Internal automation (setup, conformance, bench)        |

## MSRV

Minimum Supported Rust Version: **1.85** (edition 2024). MSRV bumps are SemVer-minor breaking.

## License

MIT — see [LICENSE](LICENSE) and [piccle-spec/LICENSE](piccle-spec/LICENSE).
