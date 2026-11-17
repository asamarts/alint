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

use std::path::Path;

use alint_core::{Context, FileEntry, FileIndex, RuleSpec};

/// The environment of a test's own `git` commands: no system or global
/// configuration file and no global excludes or attributes file
/// (`$XDG_CONFIG_HOME/git/ignore`, which `GIT_CONFIG_GLOBAL` does not cover;
/// a path below the null device reads as no file), so a test's repository
/// does not depend on the Git setup of the machine it runs on.
pub const HERMETIC_GIT: [(&str, &str); 3] = {
    const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
    [
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", NULL_DEVICE),
        ("XDG_CONFIG_HOME", NULL_DEVICE),
    ]
};

/// The walk options of a test: without the user's global excludes file, which
/// would leave out the files a test wrote on a machine whose global excludes
/// list them.
pub fn walk_options() -> alint_core::WalkOptions {
    alint_core::WalkOptions::default().with_global_git_excludes(false)
}

/// Parse a `RuleSpec` from a YAML literal. Panics on parse
/// errors with a clear message - tests are expected to feed
/// it well-formed input.
pub fn spec_yaml(yaml: &str) -> RuleSpec {
    alint_core::yaml::from_str(yaml)
        .unwrap_or_else(|e| panic!("invalid spec yaml: {e}\n=== input ===\n{yaml}"))
}

/// Build an in-memory `FileIndex` from a list of relative
/// paths. Every entry is a file (`is_dir: false`) with `size:
/// 0`. Tests that need directories or non-zero sizes should
/// use [`index_with_dirs`] or build the index directly.
pub fn index(paths: &[&str]) -> FileIndex {
    FileIndex::from_entries(
        paths
            .iter()
            .map(|p| FileEntry {
                path: std::path::Path::new(p).into(),
                is_dir: false,
                size: 0,
            })
            .collect(),
    )
}

/// Build an in-memory `FileIndex` where each entry carries an
/// `is_dir` flag. Used by rules that distinguish files from
/// directories (`dir_exists`, `dir_absent`, …).
pub fn index_with_dirs(entries: &[(&str, bool)]) -> FileIndex {
    FileIndex::from_entries(
        entries
            .iter()
            .map(|(p, is_dir)| FileEntry {
                path: std::path::Path::new(p).into(),
                is_dir: *is_dir,
                size: 0,
            })
            .collect(),
    )
}

/// Construct a `Context` with the minimal set of fields a unit
/// test cares about. Pass `root` (typically a `tempdir.path()`
/// or `Path::new("/fake")` when the rule doesn't read the
/// filesystem) and the previously-built `FileIndex`.
pub fn ctx<'a>(root: &'a Path, idx: &'a FileIndex) -> Context<'a> {
    Context::new(root, idx)
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
            path: std::path::Path::new(rel).into(),
            is_dir: false,
            size: content.len() as u64,
        });
    }
    (tmp, FileIndex::from_entries(entries))
}
