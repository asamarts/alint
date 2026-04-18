//! Measures one full filesystem walk of a synthetic tree.
//!
//! Feature-gated behind `fs-benches` because filesystem benchmarks are
//! inherently noisier than pure-CPU ones (page cache state, syscall cost,
//! thread scheduling). Useful for characterizing the `ignore`-crate walk,
//! not for tight regression gating.
//!
//! Run with:
//!   cargo bench -p alint-bench --features fs-benches --bench walker

#[cfg(feature = "fs-benches")]
mod inner {
    use alint_core::{WalkOptions, walk};
    use criterion::{BenchmarkId, Criterion, Throughput};

    pub fn bench_walker(c: &mut Criterion) {
        let mut group = c.benchmark_group("walker");
        for &n in &[100usize, 1_000, 10_000] {
            let tree = alint_bench::tree::generate_tree(n, 4, 42)
                .expect("tree generates");
            let opts = WalkOptions::default();
            group.throughput(Throughput::Elements(n as u64));
            group.bench_with_input(BenchmarkId::from_parameter(n), &tree, |b, t| {
                b.iter(|| walk(t.root(), &opts).unwrap());
            });
        }
        group.finish();
    }
}

#[cfg(feature = "fs-benches")]
use inner::bench_walker;

#[cfg(feature = "fs-benches")]
criterion::criterion_group!(benches, bench_walker);

#[cfg(feature = "fs-benches")]
criterion::criterion_main!(benches);

#[cfg(not(feature = "fs-benches"))]
fn main() {
    eprintln!("walker bench requires `--features fs-benches`");
}
