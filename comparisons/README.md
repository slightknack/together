# CRDT Comparisons

Benchmarks comparing Together against other CRDT libraries.

## Rust Multi-Library Comparison

Compares: Together, diamond-types, Cola, Loro, Yrs, Automerge

```bash
cd rust
cargo build --release
./target/release/bench_all
```

## Rust Criterion Benchmarks

Detailed criterion benchmarks comparing Together vs diamond-types:

```bash
cd criterion
cargo bench
```

Or quick single-run:
```bash
cargo run --release --bin quick_bench
cargo run --release --bin profile_bench
```

## JavaScript Benchmarks

Compares: json-joy

```bash
cd js
npm install
node bench.js
```

## Results

See `research/27-crdt-benchmark-summary.md` for full results.

**Summary**: Together is fastest on all traces, 1.2-3.4x faster than diamond-types.
