# hypobench

Statistically rigorous A/B testing of benchmarks across commits.

## Overview

hypobench compares benchmark performance between two commits (baseline vs candidate) using interleaved execution and hypothesis testing to determine if changes are statistically significant.

### Key Features

- **Interleaved execution** - Reduces environmental noise by running baseline and candidate sequentially in alternating order (not in parallel)
- **Statistical rigor** - Uses Welch's t-test to determine if performance differences are significant
- **Minimal migration** - Works with existing criterion benchmarks without code changes
- **Git integration** - Automatically manages worktrees for baseline and candidate commits

## Installation

```bash
cargo install hypobench
```

## Usage

### Automatic Mode (Git + Build)

Run from a git repository containing criterion benchmarks:

```bash
# Compare two branches
hypobench --baseline main --candidate feature-branch

# Compare specific commits
hypobench --baseline v1.0.0 --candidate HEAD

# For monorepos, specify the project subdirectory
hypobench --baseline main --candidate HEAD --project-path examples/char-counter

# Enable harness output for debugging
hypobench --baseline main --candidate HEAD --harness-output
```

### Manual Mode (Pre-running Harnesses)

For debugging or when you want more control, you can start harnesses manually and connect to them:

```bash
# Terminal 1: Build and start baseline harness
cd examples/char-counter
cargo build --release --bench char_bench
HYPOBENCH_PORT=9100 ./target/release/deps/char_bench-*

# Terminal 2: Start candidate harness (same binary for testing, or different version)
HYPOBENCH_PORT=9101 ./target/release/deps/char_bench-*

# Terminal 3: Run comparison against pre-running harnesses
hypobench \
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
      --config <PATH>                    Path to config file [default: .hypobench.toml]
      --format <FORMAT>                  Report format [default: terminal] [possible: terminal, markdown, json]
  -v, --verbose                          Verbose output
  -h, --help                             Print help
  -V, --version                          Print version
```

### Reporting

By default, hypobench writes a terminal-friendly table to stdout. Pass `--format json` to emit a versioned, machine-readable `Report` (schema version 1) with run metadata plus per-benchmark comparisons — suitable for archiving as a CI artifact or feeding a dashboard.

```bash
# Run once, produce JSON
hypobench --baseline main --candidate HEAD --format json > report.json

# Re-render without re-running
hypobench report --in report.json --format markdown > pr-comment.md
hypobench report --in report.json --format terminal
hypobench report --in - --format markdown < report.json   # stdin also accepted
```

The `hypobench report` subcommand consumes a JSON report and renders it in any supported format — useful for CI pipelines that want to produce both a step-summary table and a PR comment from a single benchmark run.

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
git clone https://github.com/anthropics/hypobench
cd hypobench

# Run a comparison between two commits (uses the char-counter example)
cargo run -p hypobench --release -- \
  --baseline HEAD~1 \
  --candidate HEAD \
  --project-path examples/char-counter \
  --sample-size 20

# With harness debug output enabled
cargo run -p hypobench --release -- \
  --baseline HEAD~1 \
  --candidate HEAD \
  --project-path examples/char-counter \
  --sample-size 20 \
  --harness-output
```

## Configuration

Create a `.hypobench.toml` file in your repository:

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

### Harness Protocol

The orchestrator communicates with harnesses via HTTP:

- `GET /health` - Health check
- `GET /benchmarks` - List available benchmarks
- `POST /run` - Run a single benchmark iteration
- `POST /claim` - Claim exclusive access (prevents concurrent orchestrators)
- `POST /release` - Release the claim
- `POST /shutdown` - Graceful shutdown

When an orchestrator claims a harness, all subsequent requests must include the claim nonce in the `X-Harness-Claim` header. This prevents accidentally running two orchestrators against the same harness.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    hypobench CLI                      │
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
