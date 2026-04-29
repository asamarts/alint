//! Shared helpers for rule-kind unit tests.
//!
//! Phase-1 of the v0.8 test foundation adds unit tests to ~34
//! rule modules that today have none. Most of those tests need
//! the same three pieces of scaffolding — a `RuleSpec`-from-YAML
//! deserialiser, a `FileIndex` builder from a list of relative
//! paths, and a hand-crafted `Context` — so this module hosts
//! them once.
//!
//! Lives under `#[cfg(test)]` so the helpers don't ship in
//! release builds. Per-rule tests bring them in via
//! `use crate::test_support::*;`.

use std::path::{Path, PathBuf};

use alint_core::{Context, FileEntry, FileIndex, RuleSpec};

/// Parse a `RuleSpec` from a YAML literal. Panics on parse
/// errors with a clear message — tests are expected to feed
/// it well-formed input.
pub fn spec_yaml(yaml: &str) -> RuleSpec {
    serde_yaml_ng::from_str(yaml)
        .unwrap_or_else(|e| panic!("invalid spec yaml: {e}\n=== input ===\n{yaml}"))
}

/// Build an in-memory `FileIndex` from a list of relative
/// paths. Every entry is a file (`is_dir: false`) with `size:
/// 0`. Tests that need directories or non-zero sizes should
/// use [`index_with_dirs`] or build the index directly.
pub fn index(paths: &[&str]) -> FileIndex {
    FileIndex {
        entries: paths
            .iter()
            .map(|p| FileEntry {
                path: PathBuf::from(p),
                is_dir: false,
                size: 0,
            })
            .collect(),
    }
}

/// Build an in-memory `FileIndex` where each entry carries an
/// `is_dir` flag. Used by rules that distinguish files from
/// directories (`dir_exists`, `dir_absent`, …).
pub fn index_with_dirs(entries: &[(&str, bool)]) -> FileIndex {
    FileIndex {
        entries: entries
            .iter()
            .map(|(p, is_dir)| FileEntry {
                path: PathBuf::from(p),
                is_dir: *is_dir,
                size: 0,
            })
            .collect(),
    }
}

/// Construct a `Context` with the minimal set of fields a unit
/// test cares about. Pass `root` (typically a `tempdir.path()`
/// or `Path::new("/fake")` when the rule doesn't read the
/// filesystem) and the previously-built `FileIndex`.
pub fn ctx<'a>(root: &'a Path, idx: &'a FileIndex) -> Context<'a> {
    Context {
        root,
        index: idx,
        registry: None,
        facts: None,
        vars: None,
        git_tracked: None,
        git_blame: None,
    }
}

/// A tempdir + matching `FileIndex` populated with files whose
/// content is written to disk. Returns `(tempdir, index)`; keep
/// the tempdir alive so its path stays valid for the duration
/// of the test.
///
/// Used by content-family rule tests that need the rule's
/// `evaluate()` to actually read bytes off disk (rather than
/// just inspect the index).
pub fn tempdir_with_files(files: &[(&str, &[u8])]) -> (tempfile::TempDir, FileIndex) {
    let tmp = tempfile::Builder::new()
        .prefix("alint-rule-test-")
        .tempdir()
        .expect("tempdir create");
    let mut entries = Vec::with_capacity(files.len());
    for (rel, content) in files {
        let abs = tmp.path().join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&abs, content).expect("write fixture file");
        entries.push(FileEntry {
            path: PathBuf::from(rel),
            is_dir: false,
            size: content.len() as u64,
        });
    }
    (tmp, FileIndex { entries })
}
