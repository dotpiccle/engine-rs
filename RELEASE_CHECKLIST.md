# Release Checklist

Use this checklist on the exact commit that will be tagged. A green command from another revision is
not release evidence. Do not tag while any applicable item for the selected release scope remains
unchecked. A GitHub source release and crates.io publication are separate operations; `v1.0.0` is a
GitHub source release only.

## Specification and conformance

- [ ] Record the pinned specification commit in `CONFORMANCE.md` and confirm it matches
      `git ls-files -s piccle-spec`.
- [ ] Run the specification validator from the submodule: `python3 piccle-spec/scripts/validate.py`.
- [ ] Run `cargo conformance --piccle-spec piccle-spec` (release-profile xtask).
- [ ] Confirm every valid and invalid fixture is discovered dynamically; do not hard-code inventory
      counts as test selection.
- [ ] Render every official example in canonical mode with exact frame counts and finite output.
- [ ] Confirm no open normative specification defect affects the claimed engine profiles.
- [ ] Update `CONFORMANCE.md` with the final command results and open-issue state.

## Rust quality and portability

- [ ] `cargo nextest run --workspace --all-features`
- [ ] Confirm library line coverage remains above the CI floor with
      `cargo llvm-cov nextest --workspace --all-features --ignore-filename-regex 'xtask/'
      --fail-under-lines 90`.
- [ ] `cargo test --doc --workspace --all-features`
- [ ] `cargo +nightly fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo hack clippy --feature-powerset --no-dev-deps -- -D warnings`
- [ ] `cargo +1.85 check --workspace --all-features`
- [ ] `RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc -Zunstable-options --no-deps
      --workspace --all-features`
- [ ] Run `dprint check`, `typos`, and `git diff --check`.
- [ ] Confirm CI passes on Linux, macOS, and Windows for the release commit.
- [ ] Confirm cross-target checks pass for Linux aarch64/armv7, Android aarch64/armv7, iOS
      aarch64/x86_64, and the WASM core/render crates.
- [ ] Link the API-21 ARMv7 probe:
      `cargo ndk -t armeabi-v7a -p 21 build --release -p xtask --bin piccle-device-bench`.

## Security and supply chain

- [ ] Protect `main` with a branch ruleset that requires the full CI workflow and pull requests, and
      blocks force pushes and deletion.
- [ ] Protect release tags and configure a `release` environment with deployment approval before
      granting crates.io publication credentials.
- [ ] Enable Dependabot security updates, secret scanning, secret-scanning push protection, and the
      repository policy requiring Actions to be pinned to full commit SHAs.
- [ ] `cargo deny check`
- [ ] `cargo audit`
- [ ] Compile the detached fuzz crate: `cargo check --manifest-path crates/piccle-fuzz/Cargo.toml`.
- [ ] Run the seeded parser fuzz target for the release campaign and record executions/crashes in
      `CONFORMANCE.md`.
- [ ] Confirm gitleaks passes on the complete release history.
- [ ] Confirm production library crates contain no `unsafe`, panic, unwrap, or expect paths.
- [ ] Confirm parser limits, engine limits, and the combined wet-tail frame limit reject input
      before render-resource allocation.
- [ ] Confirm `Renderer::render_into` performs zero allocation, reallocation, and deallocation in
      normal, maximum-workload, maximum-tail, and error paths.

## Performance and perceptual qualification

Device and listening items apply to the hardware or perceptual profiles explicitly claimed by the
release. A portable source release may record them as not applicable, but must not turn missing
evidence into a device-specific or perceptual claim.

- [ ] Run `cargo xtask bench` in the release profile and preserve the Criterion comparison data.
- [ ] Connect an available representative Android device and run `cargo xtask device-bench`.
- [ ] Preserve device model, Android version, CPU/RAM context, preparation latency, process peak
      RSS, throughput, real-time factor, maximum 128-frame callback latency, and callback-spike
      ratio.
- [ ] Profile all official examples, the 20 Hz oscillator risk case, the moving-filter case, and the
      published maximum workload on that device.
- [ ] State explicitly which workloads are live, ahead-of-playback, cached, or offline; resource
      acceptance ceilings are not live-real-time promises.
- [ ] Listen on neutral headphones, full-range speakers, a small-device speaker, and the
      lowest-bandwidth supported output path.
- [ ] Check recognizability, onset/ending clicks, clipping, loudness consistency, oscillator
      aliasing, filter stability, reverb cutoff, and echo timing/damping.
- [ ] A/B check wet onset, echo density, early/late energy, stereo decorrelation, brightness, decay,
      metallic ringing, and discrete echoes.

## Package and publication integrity

- [ ] Update `CHANGELOG.md`, `README.md`, `RELEASE_NOTES.md`, `BENCHMARKS.md`, `SECURITY.md`, and
      `CONFORMANCE.md` for the release behavior and evidence.
- [ ] Confirm every public item has accurate rustdoc and every public limit is exported by the
      umbrella crate.
- [ ] Run `cargo package --locked -p <crate> --list` for all five published crates; confirm each
      archive includes `LICENSE` and `README.md`, contains required runtime/test data, and excludes
      submodule-dependent integration tests.
- [ ] Verify each package in dependency order when its internal dependencies are available from the
      registry: `piccle-core`, `piccle-dsp`, `piccle-validate`, `piccle-render`, `piccle`.
- [ ] Confirm all workspace crate versions and internal dependency requirements match the release
      version.
- [ ] Confirm the tag exactly matches `v<workspace-version>`.
- [ ] Create an annotated, protected tag from the already validated commit; never repair code after
      tagging. Sign it when a project signing identity is configured.
- [ ] Confirm the tag workflow has no crates.io credential or publication step for the GitHub-only
      `v1.0.0` release.
- [ ] When registry publication is separately authorized, publish crates in dependency order and
      verify each version becomes visible through the crates.io API.
- [ ] For the first crates.io publication, use crates.io Trusted Publishing (OIDC) scoped to the
      protected `release` environment; do not restore a long-lived registry token.
- [ ] From v0.1.1 onward, run `cargo-semver-checks` against the latest published baseline before
      tagging.
- [ ] Create the GitHub Release with the conformance/device/listening evidence. Verify docs.rs only
      when crates.io publication is authorized.
