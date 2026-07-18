## Summary

## Spec reference

## Conformance

- [ ] `cargo nextest run --workspace --all-features` passes
- [ ] `cargo hack clippy --feature-powerset --no-dev-deps -- -D warnings` passes
- [ ] `cargo +nightly fmt --all -- --check` passes
- [ ] `cargo xtask conformance --spec spec` passes (if validation/render changed)
- [ ] `cargo deny check` passes
- [ ] CHANGELOG entry added (if user-visible)
- [ ] Tests added/updated (one assertion per test; regression tests for bug fixes)
- [ ] Public API docs updated (rustdoc `///` on every public item)
- [ ] No `unwrap`/`expect`/`panic!` added outside test code
- [ ] No new `unsafe` added (workspace is `#![forbid(unsafe_code)]`)
