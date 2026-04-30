//! Cross-file rule throughput at varying tree shapes.
//!
//! Per-file rules scale linearly with the file count; cross-file
//! rules (`pair`, `unique_by`, `every_matching_has`,
//! `for_each_dir`, `dir_contains`, `dir_only_contains`) have
//! non-linear scaling — `pair` is O(n²) worst-case for a naive
//! implementation; `unique_by` is hash-table-bound; `for_each_dir`
//! synthesises a sub-rule per matched directory and re-walks the
//! index per iteration.
//!
//! Bench shape: pure-CPU, in-memory `FileIndex`. No filesystem
//! I/O. Sizes: 1k / 10k file trees with realistic
//! `src/<module>/file.rs + src/<module>/file.h` pairing.

use std::path::Path;

use alint_core::{Engine, FileEntry, FileIndex, Rule};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn build_paired_index(n_modules: usize) -> FileIndex {
    let mut entries = Vec::with_capacity(n_modules * 2);
    for i in 0..n_modules {
        entries.push(FileEntry {
            path: std::path::PathBuf::from(format!("src/m{i}/widget.c")).into(),
            is_dir: false,
            size: 1024,
        });
        // Half the modules also have a matching header — exercises
        // the `pair` rule's "missing partner" branch on the other half.
        if i % 2 == 0 {
            entries.push(FileEntry {
                path: std::path::PathBuf::from(format!("src/m{i}/widget.h")).into(),
                is_dir: false,
                size: 256,
            });
        }
    }
    FileIndex { entries }
}

fn build_workspace_index(n_packages: usize) -> FileIndex {
    let mut entries = Vec::with_capacity(n_packages * 4 + 1);
    entries.push(FileEntry {
        path: std::path::PathBuf::from("packages").into(),
        is_dir: true,
        size: 0,
    });
    for i in 0..n_packages {
        entries.push(FileEntry {
            path: std::path::PathBuf::from(format!("packages/p{i}")).into(),
            is_dir: true,
            size: 0,
        });
        entries.push(FileEntry {
            path: std::path::PathBuf::from(format!("packages/p{i}/package.json")).into(),
            is_dir: false,
            size: 200,
        });
        // Half the packages have a README — exercises
        // every_matching_has's miss branch on the other half.
        if i % 2 == 0 {
            entries.push(FileEntry {
                path: std::path::PathBuf::from(format!("packages/p{i}/README.md")).into(),
                is_dir: false,
                size: 512,
            });
        }
    }
    FileIndex { entries }
}

fn build_unique_by_index(n_files: usize) -> FileIndex {
    // Half the files share stems → unique_by fires on collisions.
    let mut entries = Vec::with_capacity(n_files);
    for i in 0..n_files {
        let stem = if i % 2 == 0 { i / 2 } else { (i / 2) + 10000 };
        entries.push(FileEntry {
            path: std::path::PathBuf::from(format!("src/dir_{}/widget_{stem}.rs", i % 32)).into(),
            is_dir: false,
            size: 256,
        });
    }
    FileIndex { entries }
}

fn build_engine(yaml: &str) -> Engine {
    let config = alint_dsl::parse(yaml).expect("bench config parses");
    let registry = alint_rules::builtin_registry();
    let rules: Vec<Box<dyn Rule>> = config
        .rules
        .iter()
        .map(|spec| registry.build(spec).expect("bench rule builds"))
        .collect();
    Engine::new(rules, alint_rules::builtin_registry())
}

fn bench_pair(c: &mut Criterion) {
    let yaml = r#"
version: 1
rules:
  - id: c-needs-h
    kind: pair
    primary: "**/*.c"
    partner: "{dir}/{stem}.h"
    level: warning
"#;
    let engine = build_engine(yaml);
    let mut group = c.benchmark_group("cross_file/pair");
    let fake_root = Path::new("/bench/root");
    for &n in &[100usize, 1_000, 10_000] {
        let index = build_paired_index(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(fake_root, idx).unwrap());
        });
    }
    group.finish();
}

fn bench_unique_by(c: &mut Criterion) {
    let yaml = r#"
version: 1
rules:
  - id: unique-stems
    kind: unique_by
    select: "src/**/*.rs"
    key: "{stem}"
    level: warning
"#;
    let engine = build_engine(yaml);
    let mut group = c.benchmark_group("cross_file/unique_by");
    let fake_root = Path::new("/bench/root");
    for &n in &[1_000usize, 10_000, 100_000] {
        let index = build_unique_by_index(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(fake_root, idx).unwrap());
        });
    }
    group.finish();
}

fn bench_every_matching_has(c: &mut Criterion) {
    let yaml = r#"
version: 1
rules:
  - id: every-pkg-has-readme
    kind: every_matching_has
    select: "packages/*"
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: warning
"#;
    let engine = build_engine(yaml);
    let mut group = c.benchmark_group("cross_file/every_matching_has");
    let fake_root = Path::new("/bench/root");
    for &n in &[100usize, 1_000, 5_000] {
        let index = build_workspace_index(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(fake_root, idx).unwrap());
        });
    }
    group.finish();
}

fn bench_for_each_dir(c: &mut Criterion) {
    let yaml = r#"
version: 1
rules:
  - id: every-pkg-has-package-json
    kind: for_each_dir
    select: "packages/*"
    require:
      - kind: file_exists
        paths: "{path}/package.json"
    level: warning
"#;
    let engine = build_engine(yaml);
    let mut group = c.benchmark_group("cross_file/for_each_dir");
    let fake_root = Path::new("/bench/root");
    for &n in &[100usize, 1_000, 5_000] {
        let index = build_workspace_index(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(fake_root, idx).unwrap());
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_pair,
    bench_unique_by,
    bench_every_matching_has,
    bench_for_each_dir,
);
criterion_main!(benches);
