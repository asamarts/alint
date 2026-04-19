//! Shared [`Fixer`] implementations.
//!
//! Each fixer is a small, rule-agnostic helper: rule builders (e.g.
//! `file_exists`, `file_absent`) decide whether the configured `fix:`
//! op makes sense for their kind and, if so, construct one of the
//! fixers here and attach it to the built rule.

use std::io::Write;
use std::path::PathBuf;

use alint_core::{Error, FixContext, FixOutcome, Fixer, Result, Violation};

/// UTF-8 byte-order mark. Preserved across prepend operations so
/// editors that rely on it don't break.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Creates a file with pre-declared content. Target path is set at
/// rule-build time (either explicit `fix.file_create.path` or the
/// rule's first literal `paths:` entry).
#[derive(Debug)]
pub struct FileCreateFixer {
    path: PathBuf,
    content: String,
    create_parents: bool,
}

impl FileCreateFixer {
    pub fn new(path: PathBuf, content: String, create_parents: bool) -> Self {
        Self {
            path,
            content,
            create_parents,
        }
    }
}

impl Fixer for FileCreateFixer {
    fn describe(&self) -> String {
        format!(
            "create {} ({} byte{})",
            self.path.display(),
            self.content.len(),
            if self.content.len() == 1 { "" } else { "s" }
        )
    }

    fn apply(&self, _violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let abs = ctx.root.join(&self.path);
        if abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} already exists",
                self.path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would create {}",
                self.path.display()
            )));
        }
        if self.create_parents {
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }
        std::fs::write(&abs, &self.content).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "created {}",
            self.path.display()
        )))
    }
}

/// Removes the file named by the violation's `path`. Used by
/// `file_absent` to purge committed files that shouldn't be there.
#[derive(Debug)]
pub struct FileRemoveFixer;

impl Fixer for FileRemoveFixer {
    fn describe(&self) -> String {
        "remove the violating file".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if !abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} does not exist",
                path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would remove {}",
                path.display()
            )));
        }
        std::fs::remove_file(&abs).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("removed {}", path.display())))
    }
}

/// Prepends `content` to the start of each violating file. Paired with
/// `file_header` to inject the required header comment/boilerplate.
///
/// If the file starts with a UTF-8 BOM, `content` is inserted *after*
/// the BOM so editors that rely on it don't break.
#[derive(Debug)]
pub struct FilePrependFixer {
    content: String,
}

impl FilePrependFixer {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}

impl Fixer for FilePrependFixer {
    fn describe(&self) -> String {
        format!(
            "prepend {} byte{} to each violating file",
            self.content.len(),
            if self.content.len() == 1 { "" } else { "s" }
        )
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would prepend {} byte(s) to {}",
                self.content.len(),
                path.display()
            )));
        }
        let existing = std::fs::read(&abs).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        let mut out = Vec::with_capacity(existing.len() + self.content.len());
        if existing.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(self.content.as_bytes());
            out.extend_from_slice(&existing[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(self.content.as_bytes());
            out.extend_from_slice(&existing);
        }
        std::fs::write(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("prepended {}", path.display())))
    }
}

/// Appends `content` to the end of each violating file. Paired with
/// `file_content_matches` when the required pattern is satisfied by
/// the content appearing anywhere in the file.
#[derive(Debug)]
pub struct FileAppendFixer {
    content: String,
}

impl FileAppendFixer {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}

impl Fixer for FileAppendFixer {
    fn describe(&self) -> String {
        format!(
            "append {} byte{} to each violating file",
            self.content.len(),
            if self.content.len() == 1 { "" } else { "s" }
        )
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append {} byte(s) to {}",
                self.content.len(),
                path.display()
            )));
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&abs)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        f.write_all(self.content.as_bytes())
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "appended to {}",
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ctx(tmp: &TempDir, dry_run: bool) -> FixContext<'_> {
        FixContext {
            root: tmp.path(),
            dry_run,
        }
    }

    #[test]
    fn file_create_writes_content_when_missing() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "Apache-2.0\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("missing LICENSE"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let written = std::fs::read_to_string(tmp.path().join("LICENSE")).unwrap();
        assert_eq!(written, "Apache-2.0\n");
    }

    #[test]
    fn file_create_creates_intermediate_directories() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("a/b/c/config.yaml"), "k: v\n".into(), true);
        fixer
            .apply(&Violation::new("missing"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(tmp.path().join("a/b/c/config.yaml").exists());
    }

    #[test]
    fn file_create_skips_when_target_exists() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("README.md"), "existing\n").unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("README.md"), "NEW\n".into(), true);
        let outcome = fixer
            .apply(&Violation::new("x"), &make_ctx(&tmp, false))
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already exists")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "existing\n",
            "pre-existing content must not be overwritten"
        );
    }

    #[test]
    fn file_create_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("x.txt"), "body".into(), true);
        let outcome = fixer
            .apply(&Violation::new("x"), &make_ctx(&tmp, true))
            .unwrap();
        match outcome {
            FixOutcome::Applied(s) => assert!(s.starts_with("would create")),
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[test]
    fn file_remove_deletes_violating_path() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("debug.log");
        std::fs::write(&target, "noise").unwrap();
        let outcome = FileRemoveFixer
            .apply(
                &Violation::new("forbidden").with_path("debug.log"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert!(!target.exists());
    }

    #[test]
    fn file_remove_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FileRemoveFixer
            .apply(&Violation::new("no path"), &make_ctx(&tmp, false))
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("path")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
    }

    #[test]
    fn file_remove_dry_run_keeps_the_file() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("victim.bak");
        std::fs::write(&target, "bytes").unwrap();
        let outcome = FileRemoveFixer
            .apply(
                &Violation::new("forbidden").with_path("victim.bak"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        match outcome {
            FixOutcome::Applied(s) => assert!(s.starts_with("would remove")),
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(target.exists());
    }

    #[test]
    fn file_prepend_inserts_at_start() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        fixer
            .apply(
                &Violation::new("missing header").with_path("a.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n"
        );
    }

    #[test]
    fn file_prepend_preserves_utf8_bom() {
        let tmp = TempDir::new().unwrap();
        // BOM + "hello\n"
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(b"hello\n");
        std::fs::write(tmp.path().join("x.txt"), &bytes).unwrap();
        let fixer = FilePrependFixer::new("HEAD\n".into());
        fixer
            .apply(
                &Violation::new("m").with_path("x.txt"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        let got = std::fs::read(tmp.path().join("x.txt")).unwrap();
        assert_eq!(&got[..3], b"\xEF\xBB\xBF");
        assert_eq!(&got[3..], b"HEAD\nhello\n");
    }

    #[test]
    fn file_prepend_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "original\n").unwrap();
        FilePrependFixer::new("HEAD\n".into())
            .apply(
                &Violation::new("m").with_path("a.rs"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "original\n"
        );
    }

    #[test]
    fn file_prepend_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FilePrependFixer::new("h".into())
            .apply(&Violation::new("m"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(_)));
    }

    #[test]
    fn file_append_writes_at_end() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# Notes\n").unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        fixer
            .apply(
                &Violation::new("missing section").with_path("notes.md"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n"
        );
    }

    #[test]
    fn file_append_dry_run_leaves_file_unchanged() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "orig\n").unwrap();
        FileAppendFixer::new("extra\n".into())
            .apply(
                &Violation::new("m").with_path("x.txt"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
            "orig\n"
        );
    }

    #[test]
    fn file_append_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FileAppendFixer::new("x".into())
            .apply(&Violation::new("m"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(_)));
    }
}
