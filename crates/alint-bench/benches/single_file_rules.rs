//! Per-rule-kind isolated micro-benches for the single-file
//! family. Each rule kind that today lives only inside the
//! aggregate `rule_engine.rs` bench gets its own group here so
//! per-rule perf regressions show up cleanly on the diff.
//!
//! Feature-gated `fs-benches` because content rules read files
//! from disk — `tempfile` materialisation noise is unavoidable.

use std::io::Write;
use std::path::Path;

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
    let entries: Vec<alint_core::RuleEntry> = config
        .rules
        .into_iter()
        .map(|spec| {
            let rule = registry.build(&spec).expect("bench rule builds");
            alint_core::RuleEntry::new(rule).with_spec(std::sync::Arc::new(spec))
        })
        .collect();
    Engine::from_entries(entries, registry)
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

fn pinned_references(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_file/pinned_references");
    for &references in &[1_000usize, 10_000] {
        let tmp = tempfile::Builder::new()
            .prefix("alint-bench-pinned-references-")
            .tempdir()
            .expect("tempdir");
        let path = tmp.path().join(".github/workflows/ci.yml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut source = String::with_capacity(64 + references * 72);
        source.push_str("jobs:\n  build:\n    steps:\n");
        for _ in 0..references {
            source.push_str("      - uses: owner/repo@0123456789abcdef0123456789abcdef01234567\n");
        }
        std::fs::write(&path, source).unwrap();
        let index = walk(tmp.path(), &WalkOptions::default()).expect("walk");
        let engine = build_engine(
            r#"
version: 1
rules:
  - id: immutable-build-inputs
    kind: pinned_references
    paths: ".github/workflows/*.yml"
    format: github_actions
    level: warning
"#,
        );
        group.throughput(Throughput::Elements(references as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(references),
            &index,
            |b, index| b.iter(|| engine.run(tmp.path(), index).unwrap()),
        );
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

/// A realistic multi-rule per-file config: four content rules sharing
/// one `src/**/*.rs` scope. Used by the reeval-vs-full comparison so the
/// per-call cost reflects more than a single rule.
const MULTI_RULE_YAML: &str = r#"
version: 1
rules:
  - id: requires-spdx
    kind: file_content_matches
    paths: "src/**/*.rs"
    pattern: "SPDX-License-Identifier"
    level: warning
  - id: no-todo
    kind: file_content_forbidden
    paths: "src/**/*.rs"
    pattern: '\bTODO\b'
    level: warning
  - id: clean-tail
    kind: no_trailing_whitespace
    paths: "src/**/*.rs"
    level: warning
  - id: trailing-nl
    kind: final_newline
    paths: "src/**/*.rs"
    level: warning
"#;

/// The single-file hot path (`Engine::run_for_file`, the v0.11 LSP
/// per-keystroke path) vs a full `Engine::run`, at growing workspace
/// sizes. `run_for_file` should stay roughly flat as the tree grows
/// (it evaluates one file) while the full run scales with file count —
/// this group makes that ratio visible on the diff.
fn reeval_vs_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_file/reeval_vs_full");
    let target = Path::new("src/m0/file_0.rs");
    for &n in &[1_000usize, 10_000] {
        let tmp = make_tree(n, FIXTURE_RS);
        let index = walk(tmp.path(), &WalkOptions::default()).expect("walk");
        let engine = build_engine(MULTI_RULE_YAML);

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("full_run", n), &index, |b, idx| {
            b.iter(|| engine.run(tmp.path(), idx).unwrap());
        });
        // One file regardless of `n` — the whole point of the hot path.
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("run_for_file", n), &index, |b, idx| {
            b.iter(|| {
                engine
                    .run_for_file(tmp.path(), idx, target, FIXTURE_RS)
                    .unwrap()
            });
        });
    }
    group.finish();
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
    reeval_vs_full,
    pinned_references,
);
criterion_main!(benches);
