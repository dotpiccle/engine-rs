# Benchmarks

Run with: `cargo bench --workspace`

## Performance invariants

The Piccle spec mandates (`docs/15-engine-build-guide.md` §9):

> "steady render cost should scale with active voices and their declared filters. Reverb should add
> constant work per frame rather than work proportional to `tail_ms`." "Verify that steady rendering
> performs no memory allocation and has no cost spike when a contour boundary is crossed."

## Benchmarks to add (implementation phase)

- Per-voice render cost (mono layer with N filters)
- Filter cost vs cutoff-sweep rate
- Reverb cost per frame at `tail_ms` ∈ {1, 20, 220, 500}
- Full-document render time for each `spec/examples/*.json`
- Validator throughput (bytes/second for valid and invalid input)
- Counting allocator test: "render hot loop allocated 0 times"

## Profiling

- `cargo flamegraph --bench render` for flamegraph visualization
- `samply` for modern Linux/macOS profiling (release builds with `debug = "line-tables-only"`)
