# Release Checklist

- [ ] Run conformance suite against pinned spec commit: `cargo xtask conformance --spec spec`
- [ ] Render every official example in canonical mode
- [ ] Verify no RUSTSEC advisories: `cargo audit`
- [ ] Verify cargo-deny clean: `cargo deny check`
- [ ] Render official examples and complete listening review per spec `docs/RELEASE_CHECKLIST.md`
- [ ] Update CHANGELOG
- [ ] Bump versions via `cargo release`
- [ ] Tag, publish to crates.io, create GitHub Release
- [ ] Update docs.rs
