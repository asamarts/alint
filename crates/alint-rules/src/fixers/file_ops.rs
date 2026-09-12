use std::path::{Path, PathBuf};

use alint_core::{Applicability, Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation};

use crate::case::CaseConvention;

/// Removes the file named by the violation's `path`. Used by
/// `file_absent`, `no_empty_files`, `no_submodules`, `no_symlinks`.
///
/// Carries an [`Applicability`] tier (auto-fix.md 5.5): `file_remove` is
/// **`Unsafe` by default**, because deleting a whole file irreversibly is a poor
/// default for a bare `alint fix` -- it is surfaced as a suggestion and applied
/// only with `--unsafe-fixes`. A user may promote it back to `Safe` per-rule via
/// `fix: { file_remove: { applicability: safe } }` in their own top-level config.
#[derive(Debug)]
pub struct FileRemoveFixer {
    applicability: Applicability,
}

impl FileRemoveFixer {
    /// Construct with the resolved tier (default [`Applicability::Unsafe`]; a
    /// top-level rule may promote to `Safe`). The rule builders pass
    /// `spec.applicability.unwrap_or(Applicability::Unsafe)`.
    pub fn new(applicability: Applicability) -> Self {
        Self { applicability }
    }
}

impl Fixer for FileRemoveFixer {
    fn describe(&self) -> String {
        "remove the violating file".to_string()
    }

    fn applicability(&self) -> Applicability {
        self.applicability
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
        // Yield to a pending content edit: if a content fixer earlier in this
        // pass composed this file, removing it now would let the post-loop flush
        // resurrect it. Skip; the remove applies on a rerun once the edit lands.
        if ctx.has_pending_write(&abs) {
            return Ok(FixOutcome::Skipped(format!(
                "{} has a pending content edit this pass; rerun to remove it",
                path.display()
            )));
        }
        // A dry run reports only; a stage (`--diff`) records the delete so the
        // diff can render it. Both return before touching disk.
        if ctx.dry_run || ctx.stage_ops.is_some() {
            if let Some(sink) = ctx.stage_ops {
                sink.borrow_mut().push(FixEdit::DeleteFile {
                    path: path.to_path_buf(),
                });
            }
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

    /// Compute the rename TARGET path for `path` under this convention, or an
    /// `Err(reason)` explaining why the file must be left untouched. Shared by
    /// `apply` (disk) and `fix_edit` (editor) so their stem-level decisions can
    /// never drift -- both get the same dotfile / compound-extension exemption,
    /// empty-conversion guard, no-conforming-form (non-convergence) guard, and
    /// non-UTF-8-extension guard. Callers add their own filesystem-side checks
    /// (collision, pending write, staging).
    fn resolve_rename_target(&self, path: &Path) -> std::result::Result<PathBuf, String> {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Err(format!(
                "cannot decode filename stem for {}",
                path.display()
            ));
        };
        // A dot ANYWHERE in the stem is structural, not a case concern (mirrors
        // the detector): a dotfile (`.gitignore`) or a compound extension
        // (`index.d.ts`, `Button.test.tsx`) would have its dot DROPPED by
        // `tokenize`, corrupting the file.
        if stem.contains('.') {
            return Err(format!(
                "{} has a structural dot in its stem; not renaming",
                path.display()
            ));
        }
        let new_stem = self.case.convert(stem);
        if new_stem.is_empty() {
            return Err(format!(
                "case conversion produced an empty stem for {}",
                path.display()
            ));
        }
        // The conversion must produce a CONFORMING name, or the rename would not
        // converge (`check` keeps flagging it). A stem with no reachable target
        // form (a non-ASCII letter under snake like `café`, a leading-digit stem
        // under camel) is reported honestly, NOT with the false "already matches".
        if !self.case.check(&new_stem) {
            return Err(format!(
                "{} cannot be renamed to a valid {} name",
                path.display(),
                self.case.display_name()
            ));
        }
        if new_stem == stem {
            return Err(format!("{} already matches target case", path.display()));
        }
        // A non-UTF-8 extension can't survive the string-based basename rebuild;
        // dropping it would change the file's type.
        if path.extension().is_some_and(|e| e.to_str().is_none()) {
            return Err(format!(
                "{} has a non-UTF-8 extension; not renaming",
                path.display()
            ));
        }
        let mut new_basename = new_stem;
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            new_basename.push('.');
            new_basename.push_str(ext);
        }
        Ok(match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.join(&new_basename),
            _ => PathBuf::from(&new_basename),
        })
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
        // Compute the target name (and all stem-level guards) via the shared
        // helper, so `apply` and `fix_edit` can't drift on which files they touch.
        let new_path = match self.resolve_rename_target(path) {
            Ok(target) => target,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };

        let abs_from = ctx.root.join(path);
        let abs_to = ctx.root.join(&new_path);
        if abs_to.exists() {
            // On a case-INSENSITIVE filesystem (macOS/Windows) a pure case flip
            // (`Foo.rs` -> `foo.rs`) reports the target as "existing" because it
            // IS the source file (same inode), which would wrongly abort the
            // rename and never converge. That is not a real collision: allow it
            // through only when the two paths resolve to the SAME file. On a
            // case-sensitive FS the target genuinely does not exist yet, so this
            // branch isn't even entered and behaviour is unchanged.
            let from_canon = std::fs::canonicalize(&abs_from).ok();
            let to_canon = std::fs::canonicalize(&abs_to).ok();
            let same_file = from_canon.is_some() && from_canon == to_canon;
            if !same_file {
                return Ok(FixOutcome::Skipped(format!(
                    "target {} already exists",
                    new_path.display()
                )));
            }
        }
        // In a stage/preview pass (`--diff`), the disk is NOT mutated, so the
        // `exists()` check above can't see a rename an EARLIER fixer already
        // staged onto this same target. Two distinct source names can convert to
        // one target (`fooBar` and `foo_Bar` both -> `foo_bar`); without this the
        // preview would emit two `rename to <same>` hunks -- a self-conflicting
        // patch `git apply` rejects/clobbers. Treat an already-staged target as a
        // collision, mirroring the direct-fix behaviour (the second is skipped).
        if let Some(sink) = ctx.stage_ops
            && sink
                .borrow()
                .iter()
                .any(|edit| matches!(edit, FixEdit::RenameFile { to, .. } if *to == new_path))
        {
            return Ok(FixOutcome::Skipped(format!(
                "target {} is already staged for a rename this pass",
                new_path.display()
            )));
        }
        // Yield to a pending content edit on the source: if a content fixer
        // earlier in this pass composed it, renaming now would strand the
        // composed bytes at the vacated old path when the flush runs (a
        // file-duplication corruption). Skip; the rename applies on a rerun.
        if ctx.has_pending_write(&abs_from) {
            return Ok(FixOutcome::Skipped(format!(
                "{} has a pending content edit this pass; rerun to rename it",
                path.display()
            )));
        }
        // A dry run reports only; a stage (`--diff`) records the rename so the
        // diff can render it. Both return before touching disk.
        if ctx.dry_run || ctx.stage_ops.is_some() {
            if let Some(sink) = ctx.stage_ops {
                sink.borrow_mut().push(FixEdit::RenameFile {
                    from: path.to_path_buf(),
                    to: new_path.clone(),
                });
            }
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
        // Same stem-level guards + target as apply() (the shared helper): dotfile
        // / compound-extension exemption, empty / no-conforming-form conversion,
        // non-UTF-8 extension. An editor code-action must not diverge from what
        // `alint fix` would do.
        let new_path = self.resolve_rename_target(path).ok()?;
        // Collision: don't propose a rename onto a DIFFERENT existing file. A pure
        // case flip on a case-INSENSITIVE FS sees the target as "existing" (it is
        // the same file); allow that through -- mirrors apply()'s A5 same-file
        // check -- so the editor can still offer `Foo.rs` -> `foo.rs`.
        let abs_to = root.join(&new_path);
        if abs_to.exists() {
            let from_canon = std::fs::canonicalize(root.join(path)).ok();
            let to_canon = std::fs::canonicalize(&abs_to).ok();
            let same_file = from_canon.is_some() && from_canon == to_canon;
            if !same_file {
                return None;
            }
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
            compose: None,
            stage_ops: None,
        }
    }

    #[test]
    fn file_remove_deletes_violating_path() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("debug.log");
        std::fs::write(&target, "noise").unwrap();
        let outcome = FileRemoveFixer::new(alint_core::Applicability::Unsafe)
            .apply(
                &Violation::new("forbidden").with_path(std::path::Path::new("debug.log")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert!(!target.exists());
    }

    #[test]
    fn file_remove_carries_its_tier() {
        // Close-off (auto-fix.md 5.5): `file_remove` is Unsafe by default (the
        // rule builders pass `Unsafe`), so a bare `alint fix` surfaces it as a
        // suggestion; a top-level rule may promote it to `Safe`. The engine gates
        // `apply` on this tier (`applies_at`/`suggested_at`), so the value must be
        // reported faithfully. `apply` itself is tier-agnostic (it just deletes),
        // which is why the tests above call it directly.
        assert_eq!(
            FileRemoveFixer::new(Applicability::Unsafe).applicability(),
            Applicability::Unsafe
        );
        assert_eq!(
            FileRemoveFixer::new(Applicability::Safe).applicability(),
            Applicability::Safe
        );
        // Sibling whole-file fixers keep the trait default (Safe).
        assert_eq!(
            FileRenameFixer::new(CaseConvention::Snake).applicability(),
            Applicability::Safe
        );
    }

    #[test]
    fn file_remove_skips_when_violation_has_no_path() {
        let tmp = TempDir::new().unwrap();
        let outcome = FileRemoveFixer::new(alint_core::Applicability::Unsafe)
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
        let outcome = FileRemoveFixer::new(alint_core::Applicability::Unsafe)
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
    fn file_rename_never_strips_the_leading_dot_off_a_dotfile() {
        // Round-5 audit (A2): renaming `.gitignore` -> `gitignore` would drop the
        // structural leading dot and change the file's meaning (git stops
        // honoring it; a `.env` with secrets becomes committable). Dotfiles are
        // exempt -- the fixer must skip, not rename.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "build/\n").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new(".gitignore")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(ref r) if r.contains("structural dot")));
        assert!(
            tmp.path().join(".gitignore").exists(),
            "the dotfile is untouched"
        );
        assert!(
            !tmp.path().join("gitignore").exists(),
            "no de-dotted copy created"
        );
    }

    #[test]
    fn file_rename_skips_a_compound_extension() {
        // Round-6 audit (Finding 1): `Button.test.tsx` has stem `Button.test`
        // (file_stem strips only `.tsx`); `tokenize` would drop the inner `.`,
        // renaming to `button-test.tsx` and destroying the `.test` sub-extension.
        // Any dot in the stem is structural -> skip, don't corrupt.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Button.test.tsx"), "").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Kebab)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("Button.test.tsx")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Skipped(ref r) if r.contains("structural dot")));
        assert!(
            tmp.path().join("Button.test.tsx").exists(),
            "the compound-extension file is untouched"
        );
        assert!(
            !tmp.path().join("button-test.tsx").exists(),
            "no de-dotted corruption"
        );
    }

    #[test]
    fn file_rename_reports_honestly_when_no_conforming_name_exists() {
        // Round-5 audit (A3): a stem with no reachable target form (a non-ASCII
        // letter under snake) must NOT report the false "already matches target
        // case" (which left `fix` claiming success on a still-flagged file). It
        // is skipped with an honest "cannot be renamed" message and the file is
        // left in place.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("café.rs"), "").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("café.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => {
                assert!(
                    reason.contains("cannot be renamed"),
                    "honest reason: {reason}"
                );
                assert!(
                    !reason.contains("already matches"),
                    "must not lie: {reason}"
                );
            }
            FixOutcome::Applied(_) => panic!("expected Skipped"),
        }
        assert!(tmp.path().join("café.rs").exists());
    }

    #[test]
    fn file_rename_allows_a_pure_case_flip() {
        // Round-5 audit (A5): a case-only rename (`Foo.rs` -> `foo.rs`) must work.
        // On a case-sensitive FS (this CI) the target doesn't exist so it renames
        // straightforwardly; on a case-INSENSITIVE FS the same-file check added
        // for A5 keeps it from being wrongly rejected as a self-collision. This
        // guards the common-case path on every platform.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Foo.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new("Foo.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(tmp.path().join("foo.rs").exists());
    }

    #[cfg(unix)]
    #[test]
    fn file_rename_skips_a_non_utf8_extension_rather_than_dropping_it() {
        // Phase-0 audit: a UTF-8 stem with a non-UTF-8 extension must NOT rename
        // to a bare stem (dropping the extension changes the file's type). Skip.
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let tmp = TempDir::new().unwrap();
        let name = OsStr::from_bytes(b"FooBar.\xff"); // UTF-8 stem, non-UTF-8 ext
        std::fs::write(tmp.path().join(name), b"").unwrap();
        let outcome = FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(std::path::Path::new(name)),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        let FixOutcome::Skipped(msg) = &outcome else {
            panic!("expected Skipped for a non-UTF-8 extension, got {outcome:?}")
        };
        assert!(msg.contains("non-UTF-8 extension"), "message: {msg}");
        assert!(tmp.path().join(name).exists(), "the file must be untouched");
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
        let edit = FileRemoveFixer::new(alint_core::Applicability::Unsafe)
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
    fn file_rename_fix_edit_mirrors_apply_guards() {
        // Round-6 audit: the round-5 dotfile / unconvertible-stem guards were
        // added to apply() only, leaving the editor (LSP) path proposing
        // `.gitignore` -> `gitignore` (the security-adjacent data-integrity bug)
        // and a non-converging rename for an unconvertible stem. fix_edit must
        // mirror apply.
        let tmp = TempDir::new().unwrap();
        // Dotfile: no edit proposed (the leading dot is structural).
        let dot = Violation::new("case").with_path(std::path::Path::new(".gitignore"));
        assert!(
            FileRenameFixer::new(CaseConvention::Snake)
                .fix_edit(&dot, &[], tmp.path())
                .is_none(),
            "fix_edit must not propose renaming a dotfile"
        );
        // Unconvertible stem: no edit proposed (would not converge).
        let cafe = Violation::new("case").with_path(std::path::Path::new("café.rs"));
        assert!(
            FileRenameFixer::new(CaseConvention::Snake)
                .fix_edit(&cafe, &[], tmp.path())
                .is_none(),
            "fix_edit must not propose a rename that doesn't converge"
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

    // A `FixContext` in stage mode (`--diff`): a sink is present, `dry_run` is
    // false. Direct-write fixers must record their `FixEdit` here and leave the
    // tree untouched.
    fn stage_ctx<'a>(
        tmp: &'a TempDir,
        sink: &'a std::cell::RefCell<Vec<FixEdit>>,
    ) -> FixContext<'a> {
        FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: Some(sink),
        }
    }

    #[test]
    fn file_remove_in_stage_mode_records_without_deleting() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("debug.log");
        std::fs::write(&target, "noise").unwrap();
        let sink = std::cell::RefCell::new(Vec::new());
        let outcome = FileRemoveFixer::new(alint_core::Applicability::Unsafe)
            .apply(
                &Violation::new("forbidden").with_path(Path::new("debug.log")),
                &stage_ctx(&tmp, &sink),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert!(target.exists(), "stage must not delete the file");
        assert_eq!(
            sink.into_inner(),
            vec![FixEdit::DeleteFile {
                path: PathBuf::from("debug.log")
            }]
        );
    }

    #[test]
    fn file_rename_in_stage_mode_records_without_renaming() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "x").unwrap();
        let sink = std::cell::RefCell::new(Vec::new());
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path(Path::new("FooBar.rs")),
                &stage_ctx(&tmp, &sink),
            )
            .unwrap();
        assert!(
            tmp.path().join("FooBar.rs").exists(),
            "stage must not rename"
        );
        assert!(!tmp.path().join("foo_bar.rs").exists());
        assert_eq!(
            sink.into_inner(),
            vec![FixEdit::RenameFile {
                from: PathBuf::from("FooBar.rs"),
                to: PathBuf::from("foo_bar.rs"),
            }]
        );
    }
}
