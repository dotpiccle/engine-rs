# Release Checklist

Use this checklist on the exact commit that will be tagged. A green command from another revision is
not release evidence. Do not tag while any applicable item remains unchecked.

## Specification and conformance

- [ ] Record the pinned specification commit in `CONFORMANCE.md` and confirm it matches
      `git ls-files -s piccle-spec`.
- [ ] Run the specification validator from the submodule: `python3 piccle-spec/scripts/validate.py`.
- [ ] Run `cargo xtask conformance --piccle-spec piccle-spec`.
- [ ] Confirm every valid and invalid fixture is discovered dynamically; do not hard-code inventory
      counts as test selection.
- [ ] Render every official example in canonical mode with exact frame counts and finite output.
- [ ] Confirm no open normative specification defect affects the claimed engine profiles.
- [ ] Update `CONFORMANCE.md` with the final command results and open-issue state.

## Rust quality and portability

- [ ] `cargo nextest run --workspace --all-features`
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

- [ ] Run `cargo xtask bench` in the release profile and preserve the Criterion comparison data.
- [ ] Connect the lowest supported ARMv7 Android device and run `cargo xtask device-bench`.
- [ ] Preserve device model, Android version, CPU/RAM context, preparation latency, process peak
      RSS, throughput, real-time factor, maximum 128-frame callback latency, and callback-spike
      ratio.
- [ ] Profile all 14 official examples, the 20 Hz oscillator risk case, the moving-filter case, and
      the published maximum workload on that device.
- [ ] State explicitly which workloads are live, ahead-of-playback, cached, or offline; resource
      acceptance ceilings are not live-real-time promises.
- [ ] Listen on neutral headphones, full-range speakers, a small-device speaker, and the
      lowest-bandwidth supported output path.
- [ ] Check recognizability, onset/ending clicks, clipping, loudness consistency, oscillator
      aliasing, filter stability, and reverb cutoff.
- [ ] A/B check wet onset, echo density, early/late energy, stereo decorrelation, brightness, decay,
      metallic ringing, and discrete echoes.

## Package and publication integrity

- [ ] Update `CHANGELOG.md`, `README.md`, `BENCHMARKS.md`, `SECURITY.md`, and `CONFORMANCE.md` for
      the release behavior and evidence.
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
- [ ] Create a signed tag from the already validated commit; never repair code after tagging.
- [ ] Let the tag workflow publish crates in dependency order and verify each version becomes
      visible through the crates.io API.
- [ ] Verify docs.rs builds and create the GitHub Release with the conformance/device/listening
      evidence.
