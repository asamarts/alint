use std::path::{Path, PathBuf};

use ignore::{WalkBuilder, overrides::OverrideBuilder};

use crate::error::{Error, Result};

/// A single filesystem entry discovered by the walker.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Path relative to the repository root.
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// The indexed result of one filesystem walk. All rules share this index —
/// the walk happens once per `alint check` invocation.
#[derive(Debug, Default)]
pub struct FileIndex {
    pub entries: Vec<FileEntry>,
}

impl FileIndex {
    pub fn files(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }

    pub fn dirs(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| e.is_dir)
    }

    pub fn total_size(&self) -> u64 {
        self.files().map(|f| f.size).sum()
    }

    /// Find a file entry by its exact relative path. Linear scan — acceptable
    /// at the scales we target today; revisit with a `HashSet` / `HashMap`
    /// index if cross-file-rule benches start to show it.
    pub fn find_file(&self, rel: &Path) -> Option<&FileEntry> {
        self.files().find(|e| e.path == rel)
    }
}

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub respect_gitignore: bool,
    pub extra_ignores: Vec<String>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        }
    }
}

pub fn walk(root: &Path, opts: &WalkOptions) -> Result<FileIndex> {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(opts.respect_gitignore)
        .hidden(false)
        .follow_links(true)
        .require_git(false);

    // Always exclude `.git/` — descending into git's internal
    // packfiles + loose objects is wasted work for every alint
    // rule (none of them target `.git/objects/*`), and it races
    // git's auto-gc / pack-rewrite on large repos. We set
    // `hidden(false)` and `require_git(false)` so the `ignore`
    // crate doesn't apply its own implicit `.git/` exclusion;
    // this override puts it back.
    let mut overrides_builder = OverrideBuilder::new(root);
    overrides_builder
        .add("!.git")
        .map_err(|e| Error::Other(format!("ignore pattern .git: {e}")))?;
    for pattern in &opts.extra_ignores {
        let pattern = if pattern.starts_with('!') {
            pattern.clone()
        } else {
            format!("!{pattern}")
        };
        overrides_builder
            .add(&pattern)
            .map_err(|e| Error::Other(format!("ignore pattern {pattern:?}: {e}")))?;
    }
    let overrides = overrides_builder
        .build()
        .map_err(|e| Error::Other(format!("failed to build overrides: {e}")))?;
    builder.overrides(overrides);

    let mut entries = Vec::new();
    for result in builder.build() {
        let entry = result?;
        let abs = entry.path();
        let Ok(rel) = abs.strip_prefix(root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let metadata = entry.metadata().map_err(|e| Error::Io {
            path: abs.to_path_buf(),
            source: std::io::Error::other(e.to_string()),
        })?;
        entries.push(FileEntry {
            path: rel.to_path_buf(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
        });
    }
    Ok(FileIndex { entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn td() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("alint-walker-test-")
            .tempdir()
            .unwrap()
    }

    fn touch(root: &Path, rel: &str, content: &[u8]) {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, content).unwrap();
    }

    fn paths(idx: &FileIndex) -> Vec<String> {
        // Normalise to forward slashes so assertions can compare
        // against literal `"src/foo.rs"` regardless of host OS.
        // Windows' Path::display() emits `src\foo.rs`.
        idx.entries
            .iter()
            .map(|e| e.path.display().to_string().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn fileindex_files_filters_directories_out() {
        let idx = FileIndex {
            entries: vec![
                FileEntry {
                    path: "a".into(),
                    is_dir: true,
                    size: 0,
                },
                FileEntry {
                    path: "a/x.rs".into(),
                    is_dir: false,
                    size: 5,
                },
            ],
        };
        let files: Vec<_> = idx.files().collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, Path::new("a/x.rs"));
    }

    #[test]
    fn fileindex_dirs_filters_files_out() {
        let idx = FileIndex {
            entries: vec![
                FileEntry {
                    path: "a".into(),
                    is_dir: true,
                    size: 0,
                },
                FileEntry {
                    path: "a/x.rs".into(),
                    is_dir: false,
                    size: 5,
                },
            ],
        };
        let dirs: Vec<_> = idx.dirs().collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].path, Path::new("a"));
    }

    #[test]
    fn fileindex_total_size_sums_files_only() {
        let idx = FileIndex {
            entries: vec![
                FileEntry {
                    path: "a".into(),
                    is_dir: true,
                    size: 999, // dirs report 0 in `walk`, but defensively excluded here
                },
                FileEntry {
                    path: "a/x.rs".into(),
                    is_dir: false,
                    size: 100,
                },
                FileEntry {
                    path: "a/y.rs".into(),
                    is_dir: false,
                    size: 50,
                },
            ],
        };
        // total_size sums via `files()` so the directory's
        // bogus size is ignored.
        assert_eq!(idx.total_size(), 150);
    }

    #[test]
    fn fileindex_find_file_returns_match_or_none() {
        let idx = FileIndex {
            entries: vec![
                FileEntry {
                    path: "a/x.rs".into(),
                    is_dir: false,
                    size: 0,
                },
                FileEntry {
                    path: "b".into(),
                    is_dir: true,
                    size: 0,
                },
            ],
        };
        assert!(idx.find_file(Path::new("a/x.rs")).is_some());
        assert!(idx.find_file(Path::new("missing.rs")).is_none());
        // find_file filters dirs — querying a known directory
        // returns None.
        assert!(idx.find_file(Path::new("b")).is_none());
    }

    #[test]
    fn walk_excludes_dot_git_directory() {
        let tmp = td();
        touch(tmp.path(), "README.md", b"# demo\n");
        // Fake `.git/` content — should never appear in the index.
        touch(tmp.path(), ".git/config", b"[core]\n");
        touch(tmp.path(), ".git/HEAD", b"ref: refs/heads/main\n");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();

        let p = paths(&idx);
        assert!(p.contains(&"README.md".into()), "missing README.md: {p:?}");
        assert!(
            !p.iter().any(|s| s.starts_with(".git")),
            ".git was not excluded: {p:?}",
        );
    }

    #[test]
    fn walk_respects_gitignore_when_enabled() {
        let tmp = td();
        touch(tmp.path(), ".gitignore", b"target/\nignored.txt\n");
        touch(tmp.path(), "src/main.rs", b"fn main() {}\n");
        touch(tmp.path(), "target/debug/build.log", b"junk");
        touch(tmp.path(), "ignored.txt", b"junk");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: true,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();

        let p = paths(&idx);
        assert!(p.contains(&"src/main.rs".into()));
        assert!(
            !p.iter().any(|s| s.starts_with("target")),
            "target/ should be ignored: {p:?}",
        );
        assert!(
            !p.contains(&"ignored.txt".into()),
            "ignored.txt should be filtered: {p:?}",
        );
    }

    #[test]
    fn walk_includes_gitignored_paths_when_respect_gitignore_false() {
        let tmp = td();
        touch(tmp.path(), ".gitignore", b"ignored.txt\n");
        touch(tmp.path(), "ignored.txt", b"x");
        touch(tmp.path(), "kept.txt", b"y");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: Vec::new(),
            },
        )
        .unwrap();
        let p = paths(&idx);
        assert!(
            p.contains(&"ignored.txt".into()),
            "respect_gitignore=false should include it: {p:?}",
        );
        assert!(p.contains(&"kept.txt".into()));
    }

    #[test]
    fn walk_applies_extra_ignores_as_excludes() {
        let tmp = td();
        touch(tmp.path(), "src/keep.rs", b"x");
        touch(tmp.path(), "vendor/skip.rs", b"y");

        let idx = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: vec!["vendor/**".to_string()],
            },
        )
        .unwrap();
        let p = paths(&idx);
        assert!(p.contains(&"src/keep.rs".into()));
        // `vendor/**` excludes the contents but the dir entry
        // itself may still appear; the rule layer's `path_scope`
        // covers the dir-vs-file distinction. What matters here
        // is that no FILE under vendor/ was indexed.
        let file_paths: Vec<&FileEntry> = idx.files().collect();
        assert!(
            !file_paths.iter().any(|e| e.path.starts_with("vendor")),
            "no file under vendor/ should be indexed: {p:?}",
        );
    }

    #[test]
    fn walk_invalid_extra_ignore_pattern_surfaces_error() {
        let tmp = td();
        touch(tmp.path(), "a.txt", b"x");
        let err = walk(
            tmp.path(),
            &WalkOptions {
                respect_gitignore: false,
                extra_ignores: vec!["[unterminated".to_string()],
            },
        );
        assert!(err.is_err(), "bad pattern should fail: {err:?}");
    }

    #[test]
    fn walk_emits_files_with_correct_size() {
        let tmp = td();
        touch(tmp.path(), "a.txt", &[0u8; 1024]);
        let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();
        let entry = idx
            .files()
            .find(|e| e.path == Path::new("a.txt"))
            .expect("a.txt entry");
        assert_eq!(entry.size, 1024);
        assert!(!entry.is_dir);
    }

    #[test]
    fn default_walk_options_respects_gitignore_and_no_extra_ignores() {
        let opts = WalkOptions::default();
        assert!(opts.respect_gitignore);
        assert!(opts.extra_ignores.is_empty());
    }
}
