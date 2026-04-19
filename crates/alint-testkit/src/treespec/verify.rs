//! `disk ⇄ spec`. Compare an on-disk tree to a [`TreeSpec`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::spec::{TreeNode, TreeSpec};
use crate::error::{Error, Result};

/// Strictness of the tree comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerifyMode {
    /// `root` must contain exactly the paths in `spec`. Extra files
    /// on disk are reported as [`Discrepancy::Extra`].
    #[default]
    Strict,
    /// Every path in `spec` must be present and match; extra files
    /// on disk are ignored.
    Contains,
}

/// One specific mismatch between `spec` and the on-disk state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discrepancy {
    /// The path was declared in the spec but missing on disk.
    Missing { path: String },
    /// The path exists on disk but is not in the spec (Strict only).
    Extra { path: String },
    /// The spec says a file but disk has a directory, or vice versa.
    Kind {
        path: String,
        expected: &'static str,
        actual: &'static str,
    },
    /// File contents differ.
    Content {
        path: String,
        expected: String,
        actual: String,
    },
}

impl std::fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { path } => write!(f, "missing: {path}"),
            Self::Extra { path } => write!(f, "unexpected: {path}"),
            Self::Kind {
                path,
                expected,
                actual,
            } => write!(
                f,
                "kind mismatch at {path}: expected {expected}, got {actual}"
            ),
            Self::Content {
                path,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "content differs at {path}:\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
                )
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub discrepancies: Vec<Discrepancy>,
}

impl VerifyReport {
    pub fn is_match(&self) -> bool {
        self.discrepancies.is_empty()
    }
}

impl std::fmt::Display for VerifyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_match() {
            return write!(f, "(no discrepancies)");
        }
        for d in &self.discrepancies {
            writeln!(f, "- {d}")?;
        }
        Ok(())
    }
}

/// Compare the on-disk contents of `root` against `spec`. `root`
/// must exist and be a directory.
pub fn verify(spec: &TreeSpec, root: &Path, mode: VerifyMode) -> Result<VerifyReport> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.to_path_buf()));
    }
    let mut report = VerifyReport::default();
    let mut spec_paths: BTreeSet<String> = BTreeSet::new();
    visit_spec(&spec.root, "", root, &mut report, &mut spec_paths)?;

    if mode == VerifyMode::Strict {
        let actual_paths = walk_disk(root, "")?;
        for actual in actual_paths {
            if !spec_paths.contains(&actual) {
                report
                    .discrepancies
                    .push(Discrepancy::Extra { path: actual });
            }
        }
    }
    Ok(report)
}

fn visit_spec(
    children: &std::collections::BTreeMap<String, TreeNode>,
    prefix: &str,
    root: &Path,
    report: &mut VerifyReport,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    for (name, node) in children {
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        seen.insert(rel.clone());
        let abs = root.join(&rel);
        match (node, abs.is_dir(), abs.is_file(), abs.exists()) {
            (_, _, _, false) => {
                report
                    .discrepancies
                    .push(Discrepancy::Missing { path: rel });
            }
            (TreeNode::File(want), _, true, _) => {
                let got = std::fs::read_to_string(&abs).map_err(|source| Error::Io {
                    path: abs.clone(),
                    source,
                })?;
                if &got != want {
                    report.discrepancies.push(Discrepancy::Content {
                        path: rel,
                        expected: want.clone(),
                        actual: got,
                    });
                }
            }
            (TreeNode::File(_), true, _, _) => {
                report.discrepancies.push(Discrepancy::Kind {
                    path: rel,
                    expected: "file",
                    actual: "dir",
                });
            }
            (TreeNode::Dir(sub), true, _, _) => {
                visit_spec(sub, &rel, root, report, seen)?;
            }
            (TreeNode::Dir(_), _, true, _) => {
                report.discrepancies.push(Discrepancy::Kind {
                    path: rel,
                    expected: "dir",
                    actual: "file",
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Recursively list every entry under `root` as a relative path.
/// Returns directories too (needed for Extra detection of empty dirs).
fn walk_disk(root: &Path, prefix: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let base: PathBuf = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries = std::fs::read_dir(&base).map_err(|source| Error::Io {
        path: base.clone(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: base.clone(),
            source,
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry.file_type().map_err(|source| Error::Io {
            path: entry.path(),
            source,
        })?;
        out.push(rel.clone());
        if file_type.is_dir() {
            let mut nested = walk_disk(root, &rel)?;
            out.append(&mut nested);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::treespec::materialize::materialize;
    use tempfile::TempDir;

    fn spec(src: &str) -> TreeSpec {
        TreeSpec::from_yaml(src).unwrap()
    }

    #[test]
    fn strict_exact_match_has_no_discrepancies() {
        let tmp = TempDir::new().unwrap();
        let s = spec(
            r#"
a.txt: "alpha"
nested:
  b.txt: "bravo"
"#,
        );
        materialize(&s, tmp.path()).unwrap();
        let r = verify(&s, tmp.path(), VerifyMode::Strict).unwrap();
        assert!(r.is_match(), "{r}");
    }

    #[test]
    fn strict_flags_extra_files_on_disk() {
        let tmp = TempDir::new().unwrap();
        let s = spec(r#"a.txt: "alpha""#);
        materialize(&s, tmp.path()).unwrap();
        std::fs::write(tmp.path().join("sneaky.log"), "").unwrap();
        let r = verify(&s, tmp.path(), VerifyMode::Strict).unwrap();
        assert!(!r.is_match());
        assert!(
            r.discrepancies
                .iter()
                .any(|d| matches!(d, Discrepancy::Extra { path } if path == "sneaky.log"))
        );
    }

    #[test]
    fn contains_ignores_extra_files_on_disk() {
        let tmp = TempDir::new().unwrap();
        let s = spec(r#"a.txt: "alpha""#);
        materialize(&s, tmp.path()).unwrap();
        std::fs::write(tmp.path().join("noise.log"), "").unwrap();
        let r = verify(&s, tmp.path(), VerifyMode::Contains).unwrap();
        assert!(r.is_match(), "{r}");
    }

    #[test]
    fn flags_content_mismatch() {
        let tmp = TempDir::new().unwrap();
        let declared = spec(r#"a.txt: "alpha""#);
        materialize(&declared, tmp.path()).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "beta").unwrap();
        let r = verify(&declared, tmp.path(), VerifyMode::Strict).unwrap();
        assert!(
            r.discrepancies
                .iter()
                .any(|d| matches!(d, Discrepancy::Content { path, .. } if path == "a.txt"))
        );
    }

    #[test]
    fn flags_missing_path() {
        let tmp = TempDir::new().unwrap();
        let s = spec(r#"required.txt: "must exist""#);
        let r = verify(&s, tmp.path(), VerifyMode::Strict).unwrap();
        assert!(
            r.discrepancies
                .iter()
                .any(|d| matches!(d, Discrepancy::Missing { path } if path == "required.txt"))
        );
    }

    #[test]
    fn flags_file_vs_dir_kind_mismatch() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("thing")).unwrap();
        let s = spec(r#"thing: "supposed to be a file""#);
        let r = verify(&s, tmp.path(), VerifyMode::Strict).unwrap();
        assert!(
            r.discrepancies
                .iter()
                .any(|d| matches!(d, Discrepancy::Kind { path, expected, actual } if path == "thing" && *expected == "file" && *actual == "dir"))
        );
    }
}
