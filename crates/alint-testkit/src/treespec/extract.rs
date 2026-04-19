//! `disk → spec`. Walk a directory and produce a [`TreeSpec`].
//!
//! v0 limitations (see [`TREE_SPEC.md`]):
//! - Binary / non-UTF-8 files are skipped.
//! - Symlinks are skipped.
//! - Metadata (mode, mtime) is not captured.

use std::collections::BTreeMap;
use std::path::Path;

use super::spec::{TreeNode, TreeSpec};
use crate::error::{Error, Result};

/// Options controlling what [`extract`] captures.
#[derive(Debug, Clone)]
pub struct ExtractOpts {
    /// Files larger than this are skipped (treated as binary). 1 MiB
    /// by default — tree specs are meant for fixture-sized content.
    pub max_inline_bytes: u64,
    /// Skip entries whose name matches any of these. No globbing —
    /// exact-name match on each path component. `.git` is included
    /// by default so extracting a checked-out repo doesn't embed
    /// the object database.
    pub skip_names: Vec<String>,
}

impl Default for ExtractOpts {
    fn default() -> Self {
        Self {
            max_inline_bytes: 1 << 20,
            skip_names: vec![".git".to_string()],
        }
    }
}

/// Walk `root` and emit a [`TreeSpec`] describing its tree.
pub fn extract(root: &Path, opts: &ExtractOpts) -> Result<TreeSpec> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.to_path_buf()));
    }
    let children = walk(root, opts)?;
    Ok(TreeSpec { root: children })
}

fn walk(dir: &Path, opts: &ExtractOpts) -> Result<BTreeMap<String, TreeNode>> {
    let mut out = BTreeMap::new();
    let entries = std::fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        if opts.skip_names.iter().any(|s| s == name) {
            continue;
        }
        let file_type = entry.file_type().map_err(|source| Error::Io {
            path: entry.path(),
            source,
        })?;
        if file_type.is_symlink() {
            // v0: symlinks are not represented.
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            let sub = walk(&path, opts)?;
            out.insert(name.to_string(), TreeNode::Dir(sub));
        } else if file_type.is_file() {
            let meta = std::fs::metadata(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if meta.len() > opts.max_inline_bytes {
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            let Ok(text) = String::from_utf8(bytes) else {
                // Non-UTF-8 is skipped in v0.
                continue;
            };
            out.insert(name.to_string(), TreeNode::File(text));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::treespec::materialize::materialize;
    use tempfile::TempDir;

    #[test]
    fn extract_round_trips_through_materialize() {
        let tmp = TempDir::new().unwrap();
        let original = TreeSpec::from_yaml(
            r##"
a.txt: "alpha"
nested:
  b.txt: "bravo"
  c.md: "# charlie"
empty: {}
"##,
        )
        .unwrap();
        materialize(&original, tmp.path()).unwrap();
        let extracted = extract(tmp.path(), &ExtractOpts::default()).unwrap();
        assert_eq!(extracted, original);
    }

    #[test]
    fn extract_skips_binary_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("text.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("blob.bin"), [0xFFu8, 0xFE, 0x00, 0x01]).unwrap();
        let spec = extract(tmp.path(), &ExtractOpts::default()).unwrap();
        assert!(spec.root.contains_key("text.txt"));
        assert!(!spec.root.contains_key("blob.bin"));
    }

    #[test]
    fn extract_skips_files_above_size_limit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("small.txt"), "x").unwrap();
        std::fs::write(tmp.path().join("big.txt"), "x".repeat(2000)).unwrap();
        let opts = ExtractOpts {
            max_inline_bytes: 1000,
            ..Default::default()
        };
        let spec = extract(tmp.path(), &opts).unwrap();
        assert!(spec.root.contains_key("small.txt"));
        assert!(!spec.root.contains_key("big.txt"));
    }

    #[test]
    fn extract_skips_configured_names() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), "ref: main").unwrap();
        std::fs::write(tmp.path().join("README.md"), "# hi").unwrap();
        let spec = extract(tmp.path(), &ExtractOpts::default()).unwrap();
        assert!(spec.root.contains_key("README.md"));
        assert!(!spec.root.contains_key(".git"));
    }
}
