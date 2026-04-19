//! Shared [`Fixer`] implementations.
//!
//! Each fixer is a small, rule-agnostic helper: rule builders (e.g.
//! `file_exists`, `file_absent`) decide whether the configured `fix:`
//! op makes sense for their kind and, if so, construct one of the
//! fixers here and attach it to the built rule.

use std::path::PathBuf;

use alint_core::{Error, FixContext, FixOutcome, Fixer, Result, Violation};

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
}
