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

    if !opts.extra_ignores.is_empty() {
        let mut overrides = OverrideBuilder::new(root);
        for pattern in &opts.extra_ignores {
            let pattern = if pattern.starts_with('!') {
                pattern.clone()
            } else {
                format!("!{pattern}")
            };
            overrides
                .add(&pattern)
                .map_err(|e| Error::Other(format!("ignore pattern {pattern:?}: {e}")))?;
        }
        let overrides = overrides
            .build()
            .map_err(|e| Error::Other(format!("failed to build overrides: {e}")))?;
        builder.overrides(overrides);
    }

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
