# criterion-hypothesis

Statistically rigorous A/B testing of benchmarks across commits.

## Overview

criterion-hypothesis compares benchmark performance between two commits (baseline vs candidate) using interleaved execution and hypothesis testing to determine if changes are statistically significant.

### Key Features

- **Interleaved execution** - Reduces environmental noise by alternating between baseline and candidate runs
- **Statistical rigor** - Uses Welch's t-test to determine if performance differences are significant
- **Minimal migration** - Works with existing criterion benchmarks without code changes
- **Git integration** - Automatically manages worktrees for baseline and candidate commits

## Installation

```bash
cargo install criterion-hypothesis
```

## Usage

### Automatic Mode (Git + Build)

Run from a git repository containing criterion benchmarks:

```bash
# Compare two branches
criterion-hypothesis --baseline main --candidate feature-branch

# Compare specific commits
criterion-hypothesis --baseline v1.0.0 --candidate HEAD

# For monorepos, specify the project subdirectory
criterion-hypothesis --baseline main --candidate HEAD --project-path examples/char-counter

# Enable harness output for debugging
criterion-hypothesis --baseline main --candidate HEAD --harness-output
```

### Manual Mode (Pre-running Harnesses)

For debugging or when you want more control, you can start harnesses manually and connect to them:

```bash
# Terminal 1: Build and start baseline harness
cd examples/char-counter
cargo build --release --bench char_bench
CH_PORT=9100 ./target/release/deps/char_bench-*

# Terminal 2: Start candidate harness (same binary for testing, or different version)
CH_PORT=9101 ./target/release/deps/char_bench-*

# Terminal 3: Run comparison against pre-running harnesses
criterion-hypothesis \
  --baseline-url http://localhost:9100 \
  --candidate-url http://localhost:9101 \
  --sample-size 30
```

### Options

```
Options:
  -b, --baseline <BASELINE>              Baseline commit/branch to compare against
  -c, --candidate <CANDIDATE>            Candidate commit/branch to test
      --baseline-url <URL>               URL of already-running baseline harness (manual mode)
      --candidate-url <URL>              URL of already-running candidate harness (manual mode)
      --project-path <PATH>              Path to project within repo (for monorepos)
      --harness-output                   Print harness stdout/stderr for debugging
      --confidence-level <LEVEL>         Confidence level for statistical tests (0.0-1.0)
      --sample-size <SIZE>               Number of sample iterations per benchmark
      --warmup-iterations <N>            Number of warmup iterations
      --config <PATH>                    Path to config file [default: .criterion-hypothesis.toml]
  -v, --verbose                          Verbose output
  -h, --help                             Print help
  -V, --version                          Print version
```

### Example Output

```
Benchmark Results
─────────────────────────────────────────────────────────────────────────────
Benchmark          Baseline         Candidate        Change    p-value  Result
─────────────────────────────────────────────────────────────────────────────
parse_json         125.3 µs ±2.1    118.7 µs ±1.9    -5.3%     0.001    faster
serialize          89.2 µs ±1.5     91.1 µs ±1.8     +2.1%     0.142    -
validate           45.6 µs ±0.8     44.9 µs ±0.9     -1.5%     0.089    -
─────────────────────────────────────────────────────────────────────────────
Summary: 1 faster, 0 slower, 2 inconclusive
```

## Quick Start Example

The repository includes a `char-counter` example you can use to test:

```bash
# Clone and enter the repository
git clone https://github.com/anthropics/criterion-hypothesis
cd criterion-hypothesis

# Run a comparison between two commits (uses the char-counter example)
cargo run -p criterion-hypothesis --release -- \
  --baseline HEAD~1 \
  --candidate HEAD \
  --project-path examples/char-counter \
  --sample-size 20

# With harness debug output enabled
cargo run -p criterion-hypothesis --release -- \
  --baseline HEAD~1 \
  --candidate HEAD \
  --project-path examples/char-counter \
  --sample-size 20 \
  --harness-output
```

## Configuration

Create a `.criterion-hypothesis.toml` file in your repository:

```toml
[hypothesis]
confidence_level = 0.95      # Statistical confidence level
minimum_effect_size = 1.0    # Minimum % difference to report

[orchestration]
interleave_interval_ms = 100 # Delay between interleaved runs
warmup_iterations = 3        # Warmup iterations (discarded)
sample_size = 100            # Number of samples per benchmark

[build]
profile = "release"          # Cargo build profile
cargo_flags = []             # Additional cargo flags

[network]
base_port = 9100             # Base port for harness communication
harness_timeout_ms = 30000   # Timeout for harness startup
```

CLI flags override config file values.

## How It Works

1. **Source Preparation** - Creates git worktrees for baseline and candidate commits
2. **Build** - Compiles benchmark binaries for both versions
3. **Orchestration** - Spawns harness processes and collects interleaved samples
4. **Analysis** - Runs Welch's t-test on collected samples
5. **Reporting** - Displays results with statistical significance

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    criterion-hypothesis CLI                      │
├─────────────────────────────────────────────────────────────────┤
│  Source Provider  │  Build Manager  │  Config Loader            │
└─────────┬─────────┴────────┬────────┴───────────────────────────┘
          │                  │
          ▼                  ▼
   ┌──────────────┐   ┌──────────────┐
   │ Git Worktree │   │ Cargo Build  │
   │   Baseline   │   │   Process    │
   └──────────────┘   └──────────────┘
   ┌──────────────┐
   │ Git Worktree │
   │  Candidate   │
   └──────────────┘
                             │
                             ▼
          ┌─────────────────────────────────────┐
          │          Test Orchestrator          │
          │  (HTTP client, interleaving logic)  │
          └──────────────┬──────────────────────┘
                         │
            ┌────────────┴────────────┐
            ▼                         ▼
   ┌─────────────────┐      ┌─────────────────┐
   │   Harness A     │      │   Harness B     │
   │ (HTTP server)   │      │ (HTTP server)   │
   │ baseline build  │      │ candidate build │
   └─────────────────┘      └─────────────────┘
            │                         │
            └────────────┬────────────┘
                         ▼
          ┌─────────────────────────────────────┐
          │        Statistical Engine           │
          │       (Welch's t-test impl)         │
          └──────────────┬──────────────────────┘
                         ▼
          ┌─────────────────────────────────────┐
          │         Terminal Reporter           │
          └─────────────────────────────────────┘
```

## Requirements

- Rust 1.70+
- Git
- Existing criterion benchmarks in your project

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
