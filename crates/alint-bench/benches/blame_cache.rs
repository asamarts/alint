//! `BlameCache` cold / warm / miss-rate characterization.
//!
//! v0.7.3 introduced a per-run `git blame` cache so multiple
//! `git_blame_age` rules over overlapping `paths:` re-use the
//! parsed result. This bench measures three relevant points on
//! the cache curve:
//!
//!   - **cold**: first lookup, dominated by the `git blame
//!     --porcelain` shell-out (the cost we want to amortise).
//!   - **warm**: subsequent lookups hit the in-memory cache and
//!     drop to mutex-protected hashmap lookup.
//!   - **miss**: lookup against a path with no blame history
//!     (untracked file, no repo at all). Documents the cost of
//!     the negative-cache short-circuit.
//!
//! Feature-gated `fs-benches` because the cold path requires a
//! real git repo + commits; tempfile materialisation noise is
//! unavoidable.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use alint_core::git::BlameCache;
use criterion::{Criterion, criterion_group, criterion_main};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git invocation");
    if !out.status.success() {
        panic!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Build a tempdir-backed git repo with `n` files, one commit.
/// Returns (TempDir, list of relative paths).
fn build_repo(n_files: usize) -> Option<(tempfile::TempDir, Vec<PathBuf>)> {
    if !git_available() {
        return None;
    }
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-blame-")
        .tempdir()
        .ok()?;
    let root = tmp.path();
    run_git(root, &["init", "-q", "-b", "main"]);
    run_git(root, &["config", "user.name", "alint bench"]);
    run_git(root, &["config", "user.email", "bench@alint.test"]);
    let mut paths = Vec::with_capacity(n_files);
    for i in 0..n_files {
        let rel = PathBuf::from(format!("src/m{i}/file.rs"));
        let abs = root.join(&rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        let body = format!(
            "// SPDX-License-Identifier: Apache-2.0\nfn main_{i}() {{\n    println!(\"hi\");\n}}\n",
        );
        std::fs::File::create(&abs)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        paths.push(rel);
    }
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-q", "-m", "init"]);
    Some((tmp, paths))
}

fn bench_cold_first_lookup(c: &mut Criterion) {
    // Each iteration: build a fresh cache, hit one file once.
    // Dominated by `git blame` shell-out cost. Slow but it's
    // the single number the v0.9 engine cut needs to know.
    let Some((tmp, paths)) = build_repo(50) else {
        eprintln!("git unavailable; skipping cold bench");
        return;
    };
    let root = tmp.path().to_path_buf();
    let path = paths[0].clone();
    let mut g = c.benchmark_group("blame_cache/cold_first_lookup");
    g.sample_size(20); // git blame is slow; reduce sample to keep wall time sane
    g.bench_function("one_file", |b| {
        b.iter(|| {
            let cache = BlameCache::new(root.clone());
            let _ = cache.get(&path);
        });
    });
    g.finish();
}

fn bench_warm_lookup(c: &mut Criterion) {
    // One cache, populated up-front; bench loops over the same
    // hit set. Measures HashMap + Arc::clone overhead, not
    // shell-out.
    let Some((tmp, paths)) = build_repo(50) else {
        eprintln!("git unavailable; skipping warm bench");
        return;
    };
    let cache = BlameCache::new(tmp.path().to_path_buf());
    // Prime the cache.
    for p in &paths {
        let _ = cache.get(p);
    }
    let mut g = c.benchmark_group("blame_cache/warm_lookup");
    g.bench_function("hit_50_paths", |b| {
        b.iter(|| {
            for p in &paths {
                let _ = cache.get(p);
            }
        });
    });
    g.finish();
}

fn bench_miss_lookup(c: &mut Criterion) {
    // No git repo at all. Every lookup goes through the
    // negative-cache path. Documents the cost of "this file
    // has no blame and never will."
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-blame-miss-")
        .tempdir()
        .unwrap();
    let cache = BlameCache::new(tmp.path().to_path_buf());
    let path = PathBuf::from("nonexistent/file.rs");
    // Prime the negative cache.
    let _ = cache.get(&path);
    let mut g = c.benchmark_group("blame_cache/miss_lookup");
    g.bench_function("warmed_negative", |b| {
        b.iter(|| {
            let _ = cache.get(&path);
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_cold_first_lookup,
    bench_warm_lookup,
    bench_miss_lookup,
);
criterion_main!(benches);
