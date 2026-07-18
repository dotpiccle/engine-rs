# Contributing

## Quick start

```bash
git clone --recurse-submodules https://github.com/dotpiccle/engine-rs
cd engine-rs
cargo setup
```

`cargo setup` installs git hooks (via cargo-husky), checks required tools, and syncs the spec
submodule. Run it once after cloning.

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
dprint fmt --check
```

## Supply chain

```bash
cargo deny check
cargo audit
```

## Spec conformance

```bash
cargo xtask conformance --spec spec
```

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/), enforced by `committed`:

- `feat(validate): reject duplicate member names with stable code`
- `fix(render): clear reverb state at declared end`
- `docs(agents): clarify no-alloc render invariant`
- `chore(deps): bump serde_json to 1.0.128`

## Git hooks

This project uses cargo-husky for project-local git hooks. They auto-install on `cargo setup`
(primary) or `cargo test` (fallback):

- `pre-commit`: `cargo +nightly fmt --check`, `cargo clippy`, `typos`, `dprint fmt --check`
- `pre-push`: `cargo deny check`, `cargo nextest run`
- `commit-msg`: `committed`

## MSRV policy

This project supports Rust 1.85+. MSRV bumps are SemVer-minor breaking and require a CHANGELOG
entry. The MSRV must be at least 6 months old when bumped.

## AI policy

Before implementing, check `.agents/skills/` for relevant skills. See `AGENTS.md` for the
authoritative agent guide.
