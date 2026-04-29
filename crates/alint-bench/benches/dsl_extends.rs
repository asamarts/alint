//! DSL load + extends-chain throughput.
//!
//! Extends resolution is the perf-sensitive cold path the v0.5
//! cuts didn't bench: each extends entry is loaded, parsed, and
//! merged in turn. Chain depth N produces N file reads + N
//! parses + N merges.
//!
//! No HTTPS extends here — those are I/O-bound by network, not
//! CPU. Local-file extends only.

use std::io::Write;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

/// Materialise N chained config files in a tempdir:
///   .alint.yml extends ./l1.yml
///   l1.yml      extends ./l2.yml
///   ...
///   l(N-1).yml  extends ./l(N).yml
///   l(N).yml    has the actual rules
fn build_chain(depth: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-extends-")
        .tempdir()
        .expect("tempdir");
    let leaf = tmp.path().join(format!("l{depth}.yml"));
    let mut f = std::fs::File::create(&leaf).expect("create");
    f.write_all(
        b"version: 1\n\
          rules:\n  \
            - id: leaf-rule\n    \
              kind: file_exists\n    \
              paths: README.md\n    \
              level: warning\n",
    )
    .expect("write leaf");
    for level in (0..depth).rev() {
        let path = if level == 0 {
            tmp.path().join(".alint.yml")
        } else {
            tmp.path().join(format!("l{level}.yml"))
        };
        let next = format!("l{}.yml", level + 1);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(
            format!(
                "version: 1\n\
                 extends:\n  \
                   - \"./{next}\"\n\
                 rules: []\n",
            )
            .as_bytes(),
        )
        .expect("write");
    }
    tmp
}

fn bench_extends_chain_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("dsl_extends/chain_depth");
    for &depth in &[1usize, 4, 16] {
        let tmp = build_chain(depth);
        let root = tmp.path().join(".alint.yml");
        group.throughput(Throughput::Elements(depth as u64));
        group.bench_with_input(BenchmarkId::from_parameter(depth), &root, |b, p| {
            b.iter(|| alint_dsl::load(p).expect("load chain"));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_extends_chain_depth);
criterion_main!(benches);
