//! Measures match throughput of a pre-compiled `Scope` against a large batch
//! of paths, across a mix of matches/non-matches that defeats globset's
//! literal fast-path in some cases.

use std::path::PathBuf;

use alint_core::{PathsSpec, Scope};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn bench_glob_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_match");

    let scope = Scope::from_paths_spec(&PathsSpec::Many(vec![
        "src/**/*.rs".into(),
        "!src/**/testdata/**".into(),
        "components/**/*.tsx".into(),
        "docs/**/*.md".into(),
        "**/Cargo.toml".into(),
    ]))
    .unwrap();

    for &n in &[1_000usize, 10_000, 100_000] {
        let paths: Vec<PathBuf> = (0..n)
            .map(|i| match i % 6 {
                0 => PathBuf::from(format!("src/mod_{}/file_{i}.rs", i % 16)),
                1 => PathBuf::from(format!("components/Widget{i}.tsx")),
                2 => PathBuf::from(format!("docs/chapter_{}/page_{i}.md", i % 4)),
                3 => PathBuf::from(format!("src/testdata/{i}.rs")),
                4 => PathBuf::from(format!("misc/file_{i}.txt")),
                _ => PathBuf::from(format!("target/debug/{i}.rlib")),
            })
            .collect();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &paths, |b, paths| {
            b.iter(|| {
                let mut hits = 0usize;
                for p in paths {
                    if scope.matches(p) {
                        hits += 1;
                    }
                }
                hits
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_glob_match);
criterion_main!(benches);
