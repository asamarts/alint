//! Measures per-byte throughput of a regex content check — the inner loop
//! of `file_content_matches` / `file_content_forbidden` / `file_header`.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use regex::Regex;

fn bench_regex_content(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_content");

    let re = Regex::new(r"(?im)^\s*copyright\s*\(c\)\s*\d{4}").unwrap();

    for &size in &[1_024usize, 65_536, 1_048_576] {
        // A realistic-ish buffer: one license-shaped line at the top, then
        // padding. Regex succeeds; we're measuring successful-match cost.
        let mut content = String::from("Copyright (C) 2026 Example\n");
        while content.len() < size {
            content.push_str(
                "Lorem ipsum dolor sit amet consectetur adipiscing elit \
                 sed do eiusmod tempor incididunt ut labore et dolore.\n",
            );
        }
        content.truncate(size);

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &content, |b, c| {
            b.iter(|| re.is_match(c));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_regex_content);
criterion_main!(benches);
