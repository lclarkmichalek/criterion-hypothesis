use char_counter::count_char;
use criterion_hypothesis_harness::{run_harness, BenchmarkRegistry};
use std::time::Instant;

fn main() {
    let port: u16 = std::env::var("CH_PORT")
        .expect("CH_PORT environment variable must be set")
        .parse()
        .expect("CH_PORT must be a valid port number");

    let mut registry = BenchmarkRegistry::new();

    // Register benchmarks for different input sizes
    for size in [100, 1000, 10000] {
        let input: String = "a".repeat(size);
        let name = format!("char_counting/count_char/{}", size);

        registry.register(name, move || {
            let start = Instant::now();
            let _ = count_char(&input, 'a');
            start.elapsed()
        });
    }

    run_harness(registry, port).expect("Failed to run harness");
}
