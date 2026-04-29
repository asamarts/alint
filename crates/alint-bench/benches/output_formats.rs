//! Output-formatter throughput across all 8 formats.
//!
//! Builds a synthetic `Report` with N violations spread across
//! a few rules and renders it through every `Format` variant.
//! Surfaces format-specific perf regressions — SARIF's verbose
//! schema, `JUnit`'s XML escaping, GitLab's per-violation hash
//! computation, etc. all have very different scaling profiles.
//!
//! Pure CPU; no filesystem I/O.

use alint_core::{Level, Report, RuleResult, Violation};
use alint_output::Format;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn build_report(n_violations: usize) -> Report {
    // Spread across 5 rules of varying severity and locality.
    // 5 rules × N/5 violations each, with realistic shape:
    // path + line + column + multi-word message.
    let per_rule = n_violations / 5;
    let mut results = Vec::with_capacity(5);
    for (i, level) in [
        Level::Error,
        Level::Warning,
        Level::Warning,
        Level::Info,
        Level::Error,
    ]
    .iter()
    .enumerate()
    {
        let rule_id = format!("synthetic-rule-{i}");
        let violations = (0..per_rule)
            .map(|j| {
                Violation::new(format!(
                    "synthetic violation {j} for rule {i} at this offset",
                ))
                .with_path(format!("src/path/segment_{}/file_{j}.rs", j % 32))
                .with_location(j + 1, (j % 80) + 1)
            })
            .collect();
        results.push(RuleResult {
            rule_id,
            level: *level,
            policy_url: Some(format!("https://example.com/rules/{i}")),
            violations,
            is_fixable: i % 2 == 0,
        });
    }
    Report { results }
}

fn bench_formats(c: &mut Criterion) {
    let formats = [
        (Format::Human, "human"),
        (Format::Json, "json"),
        (Format::Sarif, "sarif"),
        (Format::Github, "github"),
        (Format::Markdown, "markdown"),
        (Format::Junit, "junit"),
        (Format::Gitlab, "gitlab"),
        (Format::Agent, "agent"),
    ];

    for &n in &[100usize, 1_000, 10_000] {
        let report = build_report(n);
        let mut group = c.benchmark_group(format!("output_formats/{n}_violations"));
        group.throughput(Throughput::Elements(n as u64));
        for (format, name) in &formats {
            group.bench_with_input(BenchmarkId::from_parameter(name), &report, |b, rep| {
                b.iter(|| {
                    let mut buf = Vec::with_capacity(64 * 1024);
                    format.write(rep, &mut buf).expect("formatter ok");
                    buf
                });
            });
        }
        group.finish();
    }
}

criterion_group!(benches, bench_formats);
criterion_main!(benches);
