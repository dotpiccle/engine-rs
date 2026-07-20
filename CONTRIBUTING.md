# Contributing

## Quick start

```bash
git clone --recurse-submodules https://github.com/dotpiccle/engine-rs
cd engine-rs
cargo setup
```

`cargo setup` configures repository-owned git hooks, checks required tools and cross-compilation
targets, and syncs the pinned spec submodule. It reports every missing prerequisite and exits
non-zero instead of changing the global Rust installation.

## Building

```bash
cargo check --workspace
cargo build --workspace
```

## Testing

```bash
cargo nextest run --workspace --all-features
cargo test --doc --workspace
```

## Linting

```bash
cargo +nightly fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
typos
dprint check
```

## Supply chain

```bash
cargo deny check
cargo audit
```

## Spec conformance

```bash
cargo xtask conformance --piccle-spec piccle-spec
```

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/), enforced by `committed`:

- `feat(validate): reject duplicate member names with stable code`
- `fix(render): clear reverb state at declared end`
- `docs(agents): clarify no-alloc render invariant`
- `chore(deps): bump serde_json to 1.0.128`

## Git hooks

This project stores hooks in `.cargo-husky/hooks`. `cargo setup` activates them through the
repository-local `core.hooksPath` Git setting; builds and tests never modify `.git/hooks`:

- `pre-commit`: `cargo +nightly fmt --check`, `cargo clippy`, `typos`, `dprint check`
- `pre-push`: `cargo deny check`, `cargo nextest run`
- `commit-msg`: `committed`

## MSRV policy

This project supports Rust 1.85+. MSRV bumps are SemVer-minor breaking and require a CHANGELOG
entry. The MSRV must be at least 6 months old when bumped.

## AI policy

Before implementing, check `.agents/skills/` for relevant skills. See `AGENTS.md` for the
authoritative agent guide.
