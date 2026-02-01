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

Run from a git repository containing criterion benchmarks:

```bash
criterion-hypothesis --baseline main --candidate feature-branch
```

### Options

```
Options:
  -b, --baseline <BASELINE>        Baseline commit/branch to compare against
  -c, --candidate <CANDIDATE>      Candidate commit/branch to test
      --confidence-level <LEVEL>   Confidence level for statistical tests (0.0-1.0)
      --sample-size <SIZE>         Number of sample iterations per benchmark
      --warmup-iterations <N>      Number of warmup iterations
      --config <PATH>              Path to config file [default: .criterion-hypothesis.toml]
  -v, --verbose                    Verbose output
  -h, --help                       Print help
  -V, --version                    Print version
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
