//! Measures `Scope::from_paths_spec` cost as a function of pattern count.
//!
//! The hot path is `globset::GlobSetBuilder::build` compiling a set of
//! `Glob`s into an aho-corasick-backed matcher.

use alint_core::{PathsSpec, Scope};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

fn bench_glob_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_compile");
    for &n in &[10usize, 100, 1_000] {
        let patterns: Vec<String> = (0..n)
            .map(|i| match i % 4 {
                0 => format!("src/**/module_{i}/*.rs"),
                1 => format!("components/**/Widget{i}.tsx"),
                2 => format!("docs/**/guide-{i}.md"),
                _ => format!("tests/test_{i}_*.rs"),
            })
            .collect();
        let spec = PathsSpec::Many(patterns);
        group.bench_with_input(BenchmarkId::from_parameter(n), &spec, |b, spec| {
            b.iter(|| Scope::from_paths_spec(spec).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_glob_compile);
criterion_main!(benches);
