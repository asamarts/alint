use std::path::{Path, PathBuf};

use alint_core::{Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation};

use crate::case::CaseConvention;

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

    fn fix_edit(&self, violation: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        Some(FixEdit::DeleteFile {
            path: path.to_path_buf(),
        })
    }
}

/// Renames the violating file's stem to a target case convention,
/// preserving the extension and keeping the file in the same parent
/// directory. Paired with `filename_case`.
///
/// Skips with a clear reason when: the violation has no path, the
/// target name equals the current name (already conforming), or a
/// different file already occupies the target name (collision).
#[derive(Debug)]
pub struct FileRenameFixer {
    case: CaseConvention,
}

impl FileRenameFixer {
    pub fn new(case: CaseConvention) -> Self {
        Self { case }
    }
}

impl Fixer for FileRenameFixer {
    fn describe(&self) -> String {
        format!("rename stems to {}", self.case.display_name())
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Ok(FixOutcome::Skipped(format!(
                "cannot decode filename stem for {}",
                path.display()
            )));
        };
        let new_stem = self.case.convert(stem);
        if new_stem == stem {
            return Ok(FixOutcome::Skipped(format!(
                "{} already matches target case",
                path.display()
            )));
        }
        if new_stem.is_empty() {
            return Ok(FixOutcome::Skipped(format!(
                "case conversion produced an empty stem for {}",
                path.display()
            )));
        }

        let mut new_basename = new_stem;
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            new_basename.push('.');
            new_basename.push_str(ext);
        }
        let new_path: PathBuf = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.join(&new_basename),
            _ => PathBuf::from(&new_basename),
        };

        let abs_from = ctx.root.join(path);
        let abs_to = ctx.root.join(&new_path);
        if abs_to.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "target {} already exists",
                new_path.display()
            )));
        }
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would rename {} -> {}",
                path.display(),
                new_path.display()
            )));
        }
        std::fs::rename(&abs_from, &abs_to).map_err(|source| Error::Io {
            path: abs_from,
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "renamed {} -> {}",
            path.display(),
            new_path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, _bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        let stem = path.file_stem().and_then(|s| s.to_str())?;
        let new_stem = self.case.convert(stem);
        if new_stem == stem || new_stem.is_empty() {
            return None;
        }
        let mut new_basename = new_stem;
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            new_basename.push('.');
            new_basename.push_str(ext);
        }
        let new_path: PathBuf = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.join(&new_basename),
            _ => PathBuf::from(&new_basename),
        };
        // Collision: don't propose a rename onto an existing file.
        if root.join(&new_path).exists() {
            return None;
        }
        Some(FixEdit::RenameFile {
            from: path.to_path_buf(),
            to: new_path,
        })
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
            fix_size_limit: None,
            allow_out_of_root: false,
        }
    }

    #[test]
    fn file_remove_deletes_violating_path() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("debug.log");
        std::fs::write(&target, "noise").unwrap();
        let outcome = FileRemoveFixer
            .apply(
                &Violation::new("forbidden").with_path(std::path::Path::new("debug.log")),
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
                &Violation::new("forbidden").with_path(std::path::Path::new("victim.bak")),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        match outcome {
            FixOutcome::Applied(s) => {
                assert!(s.starts_with("would remove"));
                assert!(s.contains("victim.bak"), "summary must name the file: {s}");
            }
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(target.exists());
    }

    #[test]
    fn file_rename_converts_stem_preserving_extension() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "fn main() {}\n").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("FooBar.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(tmp.path().join("foo_bar.rs").exists());
        assert!(!tmp.path().join("FooBar.rs").exists());
    }

    #[test]
    fn file_rename_keeps_file_in_same_directory() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/MyModule.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("src/MyModule.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(tmp.path().join("src/my_module.rs").exists());
    }

    #[test]
    fn file_rename_skips_when_already_in_target_case() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("foo_bar.rs"), "").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("foo_bar.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
    }

    #[test]
    fn file_rename_skips_on_target_collision() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "A").unwrap();
        std::fs::write(tmp.path().join("foo_bar.rs"), "B").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("FooBar.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => assert!(reason.contains("already exists")),
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
        // Neither file should have been touched.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("FooBar.rs")).unwrap(),
            "A"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("foo_bar.rs")).unwrap(),
            "B"
        );
    }

    #[test]
    fn file_remove_fix_edit_returns_delete() {
        let v = Violation::new("forbidden").with_path(std::path::Path::new("debug.log"));
        let edit = FileRemoveFixer
            .fix_edit(&v, &[], std::path::Path::new("/repo"))
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::DeleteFile {
                path: std::path::PathBuf::from("debug.log")
            }
        );
    }

    #[test]
    fn file_rename_fix_edit_returns_rename_to_target_case() {
        let tmp = TempDir::new().unwrap();
        let v = Violation::new("case").with_path(std::path::Path::new("FooBar.rs"));
        let edit = FileRenameFixer::new(CaseConvention::Snake)
            .fix_edit(&v, &[], tmp.path())
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::RenameFile {
                from: std::path::PathBuf::from("FooBar.rs"),
                to: std::path::PathBuf::from("foo_bar.rs"),
            }
        );
    }

    #[test]
    fn file_rename_fix_edit_skips_on_collision() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("foo_bar.rs"), "B").unwrap();
        let v = Violation::new("case").with_path(std::path::Path::new("FooBar.rs"));
        assert!(
            FileRenameFixer::new(CaseConvention::Snake)
                .fix_edit(&v, &[], tmp.path())
                .is_none()
        );
    }

    #[test]
    fn file_rename_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("FooBar.rs")),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert!(tmp.path().join("FooBar.rs").exists());
        assert!(!tmp.path().join("foo_bar.rs").exists());
    }
}
