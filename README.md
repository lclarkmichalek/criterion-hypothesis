# criterion-hypothesis

Statistically rigorous A/B testing of benchmarks across commits.

## Overview

criterion-hypothesis compares benchmark performance between two commits
(baseline vs. candidate) using interleaved execution and Welch's t-test to
determine whether observed differences are statistically significant.

Each sample is a batch of inner iterations; the orchestrator calibrates the
batch size per benchmark so per-sample elapsed amortises clock-read overhead,
then feeds the resulting per-iteration means to the t-test.

### Key features

- **Interleaved execution** — baseline and candidate samples alternate to
  reduce environmental drift.
- **Automatic iteration-count calibration** — each sample runs enough inner
  iterations to take at least `target_sample_ms` (default 10 ms), keeping
  variance estimates meaningful for fast benchmarks.
- **Protocol versioning** — orchestrator and harness negotiate a wire version
  at connect time; mismatches fail with a clear upgrade instruction instead of
  a cryptic JSON parse error.
- **Git integration** — worktrees for baseline and candidate commits are
  managed automatically.

---

## Setting up a project

criterion-hypothesis uses its own harness; it does **not** use criterion's
default runner. You write benchmarks as small binaries that register closures
with a `BenchmarkRegistry` and call `run_harness`.

### 1. Add the harness as a dev-dependency

In your project's `Cargo.toml`:

```toml
[dev-dependencies]
criterion-hypothesis-harness = "0.3"

[[bench]]
name = "my_bench"   # matches benches/my_bench.rs
harness = false     # REQUIRED — tells cargo not to use libtest's harness
```

Every bench file needs its own `[[bench]]` entry with `harness = false`.

### 2. Write a benchmark file

Create `benches/my_bench.rs`:

```rust
use criterion_hypothesis_harness::{run_harness, BenchmarkRegistry};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    // The orchestrator passes the port via CH_PORT.
    let port: u16 = std::env::var("CH_PORT")
        .expect("CH_PORT must be set by the orchestrator")
        .parse()
        .expect("CH_PORT must be a valid port");

    let mut registry = BenchmarkRegistry::new();

    // Pre-build any input data once, outside the closure.
    let input: Vec<u64> = (0..10_000).collect();

    // Register each benchmark. The closure receives an iteration count `n`
    // and MUST loop its work `n` times before returning elapsed.
    registry.register("my_bench/baseline", move |n| {
        let start = Instant::now();
        for _ in 0..n {
            black_box(my_function(black_box(&input)));
        }
        start.elapsed()
    });

    run_harness(registry, port).expect("harness failed");
}

fn my_function(input: &[u64]) -> u64 {
    input.iter().sum()
}
```

### Why the closure takes `n`

The orchestrator calibrates an iteration count `n` per benchmark so that one
sample (n inner iterations) takes at least `target_sample_ms`. Each collected
sample is then `elapsed / n` — a per-iteration mean, which is the statistical
unit Welch's t-test expects. Without this loop, sub-µs benchmarks would be
dominated by clock-read overhead (`Instant::now` is ~20–50 ns) and produce
spurious significance hits on A/A tests.

Design implications for benchmark authors:

- **State accumulates across iterations**. If your function mutates a buffer
  passed in, iterations 2..n operate on an already-dirty buffer. This is the
  steady-state regime being measured; if you want per-iteration state reset,
  reset it outside the timed region.
- **Keep the closure body the work you want timed**. The `for _ in 0..n` loop
  overhead is one increment and one branch — negligible compared to anything
  you'd reasonably benchmark.
- **Use `black_box` on inputs and outputs** to prevent the optimiser from
  hoisting your function out of the loop or dead-code-eliminating its result.

### 3. (Optional) Project config

Create `.criterion-hypothesis.toml` at the repo root to override defaults:

```toml
[hypothesis]
confidence_level = 0.95      # Welch's t-test significance threshold
minimum_effect_size = 1.0    # Minimum % difference to flag (post-hoc filter)

[orchestration]
sample_size = 100            # Samples collected per benchmark (after calibration)
interleave_interval_ms = 100 # Delay between baseline/candidate samples
target_sample_ms = 10        # Calibration target: each sample runs >= this long
max_calibration_iters = 1_000_000_000  # Safety cap on calibrated n

[build]
profile = "release"
cargo_flags = []
# Optional: restrict which bench targets are built.
# bench_targets = ["my_bench"]

[network]
base_port = 9100             # baseline uses base_port, candidate uses base_port + 1
harness_timeout_ms = 30000
```

CLI flags override config file values.

---

## Running comparisons

### Automatic mode (git + build)

```bash
# Compare two refs
criterion-hypothesis --baseline main --candidate feature-branch

# Compare a tag against HEAD
criterion-hypothesis --baseline v1.0.0 --candidate HEAD

# Run only specific bench targets
criterion-hypothesis --baseline main --candidate HEAD --bench my_bench

# Monorepo: point at the subdirectory containing the Cargo.toml with benches
criterion-hypothesis --baseline main --candidate HEAD --project-path crates/hot-path

# Show harness stdout/stderr (useful when a bench panics)
criterion-hypothesis --baseline main --candidate HEAD --harness-output
```

### Manual mode (pre-running harnesses)

For debugging or when you want to drive the harness from custom tooling:

```bash
# Build and start the baseline harness
cargo build --release --bench my_bench
CH_PORT=9100 ./target/release/deps/my_bench-* &

# Start the candidate harness (different build, same port+1)
CH_PORT=9101 ./target/release/deps/my_bench-* &

# Run the comparison against the pre-running harnesses
criterion-hypothesis \
  --baseline-url http://localhost:9100 \
  --candidate-url http://localhost:9101 \
  --sample-size 30
```

### CLI options

```
Options:
  -b, --baseline <BASELINE>      Baseline commit/branch/tag to compare against
  -c, --candidate <CANDIDATE>    Candidate commit/branch/tag to test
      --baseline-url <URL>       URL of already-running baseline harness (manual mode)
      --candidate-url <URL>      URL of already-running candidate harness (manual mode)
      --bench <NAME>             Specific bench target to build and run (repeatable)
      --project-path <PATH>      Path to project within repo (for monorepos)
      --harness-output           Print harness stdout/stderr for debugging
      --confidence-level <N>     Welch's t-test confidence level (0.0–1.0)
      --sample-size <N>          Number of samples per benchmark (after calibration)
      --target-sample-ms <N>     Calibration target per sample, in milliseconds
      --config <PATH>            Path to config file [default: .criterion-hypothesis.toml]
  -v, --verbose                  Verbose output
  -h, --help                     Print help
  -V, --version                  Print version
```

### Example output

```
Benchmark                              Baseline              Candidate         Change   p-value       Result
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
parse_json/small      125.3 us (+/- 2.1 us)  118.7 us (+/- 1.9 us)    -5.27%    0.0008       faster
serialize/small        89.2 us (+/- 1.5 us)   91.1 us (+/- 1.8 us)    +2.13%    0.1416 inconclusive
validate/small         45.6 us (+/- 0.8 us)   44.9 us (+/- 0.9 us)    -1.54%    0.0892 inconclusive
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
Summary: 1 faster, 0 slower, 2 inconclusive
```

---

## How it works

1. **Source preparation** — creates git worktrees for baseline and candidate.
2. **Build** — compiles each worktree's bench binaries with the configured
   cargo profile.
3. **Calibration** — for each benchmark, the orchestrator runs the baseline
   harness with geometrically increasing `n` until one call returns elapsed
   ≥ `target_sample_ms`. That `n` is reused for candidate samples so per-iter
   means are directly comparable.
4. **Sampling** — collects `sample_size` samples per benchmark, alternating
   which side runs first. Each sample is `elapsed / n` stored as a `Duration`.
5. **Analysis** — Welch's t-test on the two sample sets; effect size reported
   as percentage difference.
6. **Reporting** — classifies each benchmark as faster / slower / inconclusive
   and prints a summary.

### Harness protocol (v2)

The orchestrator communicates with harnesses via HTTP:

- `GET /health` — returns `{ "status": "healthy", "protocol_version": 2 }`.
  The orchestrator verifies `protocol_version` at connect time and refuses to
  proceed on mismatch.
- `GET /benchmarks` — list available benchmark IDs.
- `POST /run` — body `{ "benchmark_id": "...", "iterations": N }`. Returns
  `{ "success": true, "iterations": N, "duration_ns": ... }`. The harness
  invokes the registered closure with `N` and returns whatever `Duration` it
  produces.
- `POST /claim` / `POST /release` — exclusive-access lock so two orchestrators
  don't step on each other.
- `POST /shutdown` — graceful exit.

Claimed harnesses require the nonce in the `X-Harness-Claim` header on all
subsequent requests.

---

## Requirements

- Rust 1.70+
- Git
- Benchmarks written against `criterion-hypothesis-harness` (see *Setting up
  a project*).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
