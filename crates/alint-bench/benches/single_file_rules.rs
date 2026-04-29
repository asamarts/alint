//! Per-rule-kind isolated micro-benches for the single-file
//! family. Each rule kind that today lives only inside the
//! aggregate `rule_engine.rs` bench gets its own group here so
//! per-rule perf regressions show up cleanly on the diff.
//!
//! Feature-gated `fs-benches` because content rules read files
//! from disk — `tempfile` materialisation noise is unavoidable.

use std::io::Write;

use alint_core::{Engine, Rule, WalkOptions, walk};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn make_tree(n_files: usize, content: &[u8]) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-sfr-")
        .tempdir()
        .expect("tempdir");
    for i in 0..n_files {
        let path = tmp.path().join(format!("src/m{}/file_{i}.rs", i % 16));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content).expect("write");
    }
    tmp
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

const FIXTURE_RS: &[u8] =
    b"// SPDX-License-Identifier: Apache-2.0\n// Copyright 2026 alint authors\n\nfn main() { println!(\"hello\"); }\n";

fn bench_rule(c: &mut Criterion, group_name: &str, yaml: &str) {
    let mut group = c.benchmark_group(group_name);
    for &n in &[100usize, 1_000] {
        let tmp = make_tree(n, FIXTURE_RS);
        let walk_opts = WalkOptions::default();
        let index = walk(tmp.path(), &walk_opts).expect("walk");
        let engine = build_engine(yaml);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| engine.run(tmp.path(), idx).unwrap());
        });
    }
    group.finish();
}

fn file_content_matches(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_content_matches",
        r#"
version: 1
rules:
  - id: requires-spdx
    kind: file_content_matches
    paths: "src/**/*.rs"
    pattern: "SPDX-License-Identifier"
    level: warning
"#,
    );
}

fn file_content_forbidden(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_content_forbidden",
        r#"
version: 1
rules:
  - id: no-todo
    kind: file_content_forbidden
    paths: "src/**/*.rs"
    pattern: '\bTODO\b'
    level: warning
"#,
    );
}

fn file_header(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_header",
        r#"
version: 1
rules:
  - id: spdx-header
    kind: file_header
    paths: "src/**/*.rs"
    pattern: "^// SPDX-License-Identifier:"
    level: warning
"#,
    );
}

fn file_starts_with(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_starts_with",
        r#"
version: 1
rules:
  - id: spdx-prefix
    kind: file_starts_with
    paths: "src/**/*.rs"
    prefix: "// SPDX-License-Identifier"
    level: warning
"#,
    );
}

fn file_hash(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_hash",
        r#"
version: 1
rules:
  - id: hash-frozen
    kind: file_hash
    paths: "src/**/*.rs"
    sha256: "0000000000000000000000000000000000000000000000000000000000000000"
    level: warning
"#,
    );
}

fn file_is_text(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/file_is_text",
        r#"
version: 1
rules:
  - id: must-be-text
    kind: file_is_text
    paths: "src/**/*.rs"
    level: warning
"#,
    );
}

fn no_trailing_whitespace(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/no_trailing_whitespace",
        r#"
version: 1
rules:
  - id: clean-tail
    kind: no_trailing_whitespace
    paths: "src/**/*.rs"
    level: warning
"#,
    );
}

fn final_newline(c: &mut Criterion) {
    bench_rule(
        c,
        "single_file/final_newline",
        r#"
version: 1
rules:
  - id: trailing-nl
    kind: final_newline
    paths: "src/**/*.rs"
    level: warning
"#,
    );
}

criterion_group!(
    benches,
    file_content_matches,
    file_content_forbidden,
    file_header,
    file_starts_with,
    file_hash,
    file_is_text,
    no_trailing_whitespace,
    final_newline,
);
criterion_main!(benches);
