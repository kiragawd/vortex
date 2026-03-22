use std::time::Instant;

fn run_samples(n: usize) -> Vec<u128> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        // Deterministic CPU work as placeholder benchmark sample.
        let mut acc = 0u64;
        for i in 0..50_000u64 {
            acc = acc.wrapping_add(i.rotate_left(3));
        }
        assert!(acc > 0);
        out.push(start.elapsed().as_micros());
    }
    out
}

#[test]
fn benchmark_harness_generates_statistics() {
    let samples = run_samples(10);
    assert_eq!(samples.len(), 10);

    let mut sorted = samples.clone();
    sorted.sort();
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
    assert!(p95 >= p50);
}
