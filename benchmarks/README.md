# CRDT Benchmarks

Benchmark harnesses comparing Together against other CRDT libraries.

## Rust Benchmarks

Compares: Together, diamond-types, Cola, Loro, Yrs, Automerge

```bash
cd rust
cargo build --release
./target/release/bench_all
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

**Summary**: Together is fastest on all traces, 1.2-3.4x faster than diamond-types (the next fastest Rust library).
