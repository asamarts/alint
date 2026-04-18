//! Measures the full rule-engine pass over an in-memory `FileIndex`.
//! No filesystem I/O — isolates engine overhead (glob matching, rayon fanout,
//! result aggregation) from walk costs.

use std::path::{Path, PathBuf};

use alint_core::{Engine, FileEntry, FileIndex, Rule};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const CONFIG_YAML: &str = r#"
version: 1
rules:
  - id: rs-snake
    kind: filename_case
    paths: "**/*.rs"
    case: snake
    level: warning
  - id: tsx-pascal
    kind: filename_case
    paths: "**/*.tsx"
    case: pascal
    level: warning
  - id: no-bak
    kind: file_absent
    paths: "**/*.bak"
    level: error
  - id: readme
    kind: file_exists
    paths: "README.md"
    root_only: true
    level: warning
  - id: docs-present
    kind: dir_exists
    paths: "docs/**"
    level: info
  - id: md-names
    kind: filename_regex
    paths: "**/*.md"
    pattern: "[a-zA-Z0-9_.-]+"
    level: info
  - id: no-huge
    kind: file_max_size
    paths: "**"
    max_bytes: 10485760
    level: warning
"#;

fn build_rules() -> Vec<Box<dyn Rule>> {
    let config = alint_dsl::parse(CONFIG_YAML).expect("bench config parses");
    let registry = alint_rules::builtin_registry();
    config
        .rules
        .iter()
        .map(|spec| registry.build(spec).expect("bench rule builds"))
        .collect()
}

fn build_index(n: usize) -> FileIndex {
    let mut entries = Vec::with_capacity(n + 1);
    entries.push(FileEntry {
        path: PathBuf::from("README.md"),
        is_dir: false,
        size: 2048,
    });
    entries.push(FileEntry {
        path: PathBuf::from("docs"),
        is_dir: true,
        size: 0,
    });
    for i in 0..n {
        let (path, is_dir, size) = match i % 5 {
            0 => (format!("src/mod_{}/file_{i}.rs", i % 16), false, 1024u64),
            1 => (format!("components/Widget{i}.tsx"), false, 2048),
            2 => (format!("docs/page_{i}.md"), false, 512),
            3 => (format!("tests/test_{i}.rs"), false, 800),
            _ => (format!("misc/data_{i}.yaml"), false, 256),
        };
        entries.push(FileEntry {
            path: PathBuf::from(path),
            is_dir,
            size,
        });
    }
    FileIndex { entries }
}

fn bench_rule_engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_engine");
    let fake_root = Path::new("/bench/root");

    for &n in &[1_000usize, 10_000, 100_000] {
        let index = build_index(n);
        let rules = build_rules();
        let engine = Engine::new(rules);

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(fake_root, idx).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_rule_engine);
criterion_main!(benches);
