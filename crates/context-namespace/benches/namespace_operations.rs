use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(_c: &mut Criterion) {
    // Benchmark targets will be added as modules are implemented.
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
