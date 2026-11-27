//! Fix-application throughput across fix-op variants.
//!
//! Fix throughput scales non-linearly with file count + edit
//! size — file-lock contention, disk-write back-pressure, and
//! per-op state machines all kick in. Three representative ops
//! covered: `file_create` (path-only), `file_remove` (path-only),
//! `file_trim_trailing_whitespace` (read-modify-write).

use std::io::Write;
use std::sync::Arc;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn build_engine(yaml: &str) -> Engine {
    let config = alint_dsl::parse(yaml).expect("bench config parses");
    let registry = alint_rules::builtin_registry();
    let entries: Vec<RuleEntry> = config
        .rules
        .into_iter()
        .map(|spec| {
            let rule = registry.build(&spec).expect("bench rule builds");
            RuleEntry::new(rule).with_spec(Arc::new(spec))
        })
        .collect();
    Engine::from_entries(entries, registry)
}

fn make_tree(n: usize, content: &[u8]) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-fix-")
        .tempdir()
        .expect("tempdir");
    for i in 0..n {
        let path = tmp.path().join(format!("src/m{}/file_{i}.rs", i % 16));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content).expect("write");
    }
    tmp
}

fn bench_trim_trailing_whitespace(c: &mut Criterion) {
    let yaml = r#"
version: 1
rules:
  - id: trim-tws
    kind: no_trailing_whitespace
    paths: "src/**/*.rs"
    level: warning
    fix:
      file_trim_trailing_whitespace: {}
"#;
    // Content with intentional trailing whitespace on every
    // line so the fixer has something to do per file.
    let content = b"fn one() {} \nfn two() {}  \nfn three() {}\t\n";
    let mut group = c.benchmark_group("fix_throughput/trim_trailing_whitespace");
    for &n in &[100usize, 1_000] {
        let engine = build_engine(yaml);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            // Re-materialise and walk the tree in setup, time only real
            // read-modify-write fixing (`dry_run = false`), and return `fresh`
            // so `TempDir` teardown runs outside the timed routine.
            b.iter_batched(
                || {
                    let fresh = make_tree(n, content);
                    let fresh_idx = walk(fresh.path(), &WalkOptions::default()).expect("walk");
                    (fresh, fresh_idx)
                },
                |(fresh, fresh_idx)| {
                    let report = engine
                        .fix(
                            fresh.path(),
                            &fresh_idx,
                            &WalkOptions::default(),
                            /* dry_run */ false,
                            alint_core::Applicability::Safe,
                        )
                        .expect("fix");
                    assert_eq!(report.applied(), n);
                    (fresh, report)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn bench_dry_run_only(c: &mut Criterion) {
    // Pure dry-run: catches the path-only ops' bookkeeping cost
    // without any disk-write back-pressure muddying the signal.
    //
    // `r##"..."##` again — `"# missing` would close r#"..."# early.
    let yaml = r##"
version: 1
rules:
  - id: should-have
    kind: file_exists
    paths: "MISSING.md"
    level: warning
    fix:
      file_create:
        content: "# missing\n"
"##;
    let mut group = c.benchmark_group("fix_throughput/dry_run_only");
    for &n in &[10usize, 100] {
        let tmp = make_tree(n, b"// stub\n");
        let walk_opts = WalkOptions::default();
        let index = walk(tmp.path(), &walk_opts).expect("walk");
        let engine = build_engine(yaml);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
            b.iter(|| {
                engine
                    .fix(
                        tmp.path(),
                        idx,
                        &WalkOptions::default(),
                        true,
                        alint_core::Applicability::Safe,
                    )
                    .expect("fix")
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_trim_trailing_whitespace, bench_dry_run_only);
criterion_main!(benches);
