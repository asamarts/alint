use std::path::{Path, PathBuf};

use alint_core::{
    ContentSourceSpec, Error, FixContext, FixEdit, FixOutcome, Fixer, Result, Violation,
};

use crate::io::{looks_binary, write_atomic};

/// UTF-8 byte-order mark. Preserved across prepend operations so
/// editors that rely on it don't break.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Creates a file with pre-declared content. Target path is set at
/// rule-build time (either explicit `fix.file_create.path` or the
/// rule's first literal `paths:` entry). Content is either inline
/// or read at apply time from a path-relative-to-root.
#[derive(Debug)]
pub struct FileCreateFixer {
    path: PathBuf,
    source: ContentSourceSpec,
    create_parents: bool,
}

impl FileCreateFixer {
    pub fn new(path: PathBuf, source: ContentSourceSpec, create_parents: bool) -> Self {
        Self {
            path,
            source,
            create_parents,
        }
    }
}

impl Fixer for FileCreateFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "create {} ({} byte{})",
                self.path.display(),
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => format!(
                "create {} (content from {})",
                self.path.display(),
                rel.display()
            ),
        }
    }

    fn apply(&self, _violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        // Confine the config-declared write target to the repo root (honoring
        // the owning rule's `allow_out_of_root`), so a `file_create.path` like
        // `../../x` from an untrusted `extends:`'d ruleset can't write outside
        // the tree on `alint fix`. Refuse (skip loudly) when it escapes.
        let abs = match confine_fix_path(&self.path, ctx.root, ctx.allow_out_of_root) {
            Ok(p) => p,
            Err(reason) => return Ok(FixOutcome::Skipped(reason)),
        };
        if abs.exists() {
            return Ok(FixOutcome::Skipped(format!(
                "{} already exists",
                self.path.display()
            )));
        }
        let content = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(bytes) => bytes,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would create {}",
                self.path.display()
            )));
        }
        if self.create_parents
            && let Some(parent) = abs.parent()
        {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&abs, &content).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "created {}",
            self.path.display()
        )))
    }

    fn fix_edit(&self, _violation: &Violation, _bytes: &[u8], root: &Path) -> Option<FixEdit> {
        // The target is set at build time, not taken from the violation. The
        // editor (LSP) fix path doesn't thread `allow_out_of_root`, so confine
        // strictly (deny escape): an editor code-action must never create a
        // file — or read a template — outside the repo root.
        confine_fix_path(&self.path, root, false).ok()?;
        let content = resolve_source_bytes(&self.source, root, false).ok()?;
        Some(FixEdit::CreateFile {
            path: self.path.clone(),
            content,
        })
    }
}

/// Confine a config-declared fixer path (a `file_create.path` write target or a
/// `content_from` read source) to the repo root, honoring the owning rule's
/// `allow_out_of_root`. Returns the joinable absolute path, or `Err(reason)`
/// when it escapes and isn't permitted — the write/read is then refused. This is
/// the fixer-side counterpart of the read rules' `confine_read` gate; without it
/// an untrusted `extends:`'d ruleset's fixer could write or exfiltrate
/// out-of-tree on `alint fix`.
fn confine_fix_path(rel: &Path, root: &Path, allow: bool) -> std::result::Result<PathBuf, String> {
    match crate::pathsafe::confine_read(rel, root, allow) {
        crate::pathsafe::Confined::In(p) | crate::pathsafe::Confined::AllowedEscape(p) => {
            Ok(root.join(p))
        }
        crate::pathsafe::Confined::Denied => Err(format!(
            "{} escapes the repo root (set a top-level `allow_out_of_root` to permit)",
            rel.display()
        )),
    }
}

/// Read a `ContentSourceSpec` to bytes. Returns the raw payload
/// for inline content; for file-sourced content, reads the file
/// at apply time, resolving its path relative to `ctx_root`. A
/// missing or unreadable source produces a `Skipped`-friendly
/// `Err(String)` so the caller can degrade gracefully rather
/// than abort the whole fix run.
fn resolve_source_bytes(
    source: &ContentSourceSpec,
    ctx_root: &std::path::Path,
    allow_out_of_root: bool,
) -> std::result::Result<Vec<u8>, String> {
    match source {
        ContentSourceSpec::Inline(s) => Ok(s.as_bytes().to_vec()),
        ContentSourceSpec::File(rel) => {
            // Confine the `content_from` read the same way rule reads are
            // confined, so an untrusted ruleset can't exfiltrate an out-of-tree
            // secret (`content_from: ../../secret`) into an in-repo file.
            let abs = confine_fix_path(rel, ctx_root, allow_out_of_root)?;
            std::fs::read(&abs)
                .map_err(|e| format!("content_from `{}` could not be read: {e}", rel.display()))
        }
    }
}

/// Prepends `source` content to the start of each violating
/// file. Paired with `file_header` to inject a required header
/// comment / boilerplate.
///
/// If the file starts with a UTF-8 BOM, the prepended bytes go
/// *after* the BOM so editors that rely on it don't break.
#[derive(Debug)]
pub struct FilePrependFixer {
    source: ContentSourceSpec,
}

impl FilePrependFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self { source }
    }
}

impl Fixer for FilePrependFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "prepend {} byte{} to each violating file",
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => {
                format!(
                    "prepend content from {} to each violating file",
                    rel.display()
                )
            }
        }
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let prepend = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would prepend {} byte(s) to {}",
                prepend.len(),
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not prepending content",
                path.display()
            )));
        }
        // Idempotency guard (L4): if the file already begins with exactly this
        // content (after any BOM), prepending again would stack a duplicate on
        // every `--fix` — which happens when the configured content doesn't
        // satisfy the rule's own pattern, so the violation never clears.
        // `file_starts_with` refuses a fixer outright for this reason; here we
        // can at least no-op safely.
        let body = existing.strip_prefix(UTF8_BOM).unwrap_or(&existing);
        if body.starts_with(prepend.as_slice()) {
            return Ok(FixOutcome::Skipped(format!(
                "{} already begins with the required content",
                path.display()
            )));
        }
        let mut out = Vec::with_capacity(existing.len() + prepend.len());
        if existing.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&existing);
        }
        write_atomic(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!("prepended {}", path.display())))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        let prepend = resolve_source_bytes(&self.source, root, false).ok()?;
        // Idempotency guard (L4): already-present content is not re-prepended.
        let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        if body.starts_with(prepend.as_slice()) {
            return None;
        }
        let mut out = Vec::with_capacity(bytes.len() + prepend.len());
        if bytes.starts_with(UTF8_BOM) {
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&prepend);
            out.extend_from_slice(&bytes[UTF8_BOM.len()..]);
        } else {
            out.extend_from_slice(&prepend);
            out.extend_from_slice(bytes);
        }
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out,
        })
    }
}

/// Appends `source` content to the end of each violating file.
/// Paired with `file_content_matches` / `file_footer` when the
/// required content is satisfied by the appended bytes.
#[derive(Debug)]
pub struct FileAppendFixer {
    source: ContentSourceSpec,
}

impl FileAppendFixer {
    pub fn new(source: ContentSourceSpec) -> Self {
        Self { source }
    }
}

impl Fixer for FileAppendFixer {
    fn describe(&self) -> String {
        match &self.source {
            ContentSourceSpec::Inline(s) => format!(
                "append {} byte{} to each violating file",
                s.len(),
                if s.len() == 1 { "" } else { "s" }
            ),
            ContentSourceSpec::File(rel) => {
                format!(
                    "append content from {} to each violating file",
                    rel.display()
                )
            }
        }
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let payload = match resolve_source_bytes(&self.source, ctx.root, ctx.allow_out_of_root) {
            Ok(b) => b,
            Err(skip_msg) => return Ok(FixOutcome::Skipped(skip_msg)),
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would append {} byte(s) to {}",
                payload.len(),
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        if looks_binary(&existing) {
            return Ok(FixOutcome::Skipped(format!(
                "{} looks binary; not appending content",
                path.display()
            )));
        }
        // Idempotency guard (L4): see FilePrependFixer — don't stack the footer
        // on every `--fix` when the content doesn't satisfy the rule's pattern.
        if existing.ends_with(payload.as_slice()) {
            return Ok(FixOutcome::Skipped(format!(
                "{} already ends with the required content",
                path.display()
            )));
        }
        let mut out = existing;
        out.extend_from_slice(&payload);
        write_atomic(&abs, &out).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended to {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], root: &Path) -> Option<FixEdit> {
        let path = violation.path.as_deref()?;
        let payload = resolve_source_bytes(&self.source, root, false).ok()?;
        // Idempotency guard (L4): already-present content is not re-appended.
        if bytes.ends_with(payload.as_slice()) {
            return None;
        }
        let mut out = bytes.to_vec();
        out.extend_from_slice(&payload);
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out,
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
    fn file_create_reads_content_from_relative_path() {
        // `content_from` relative to ctx.root: stage a template
        // file in the tempdir, point the fixer at it via a
        // relative path, and verify the apply step reads from
        // disk at apply time.
        let tmp = TempDir::new().unwrap();
        let template_dir = tmp.path().join(".alint/templates");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(
            template_dir.join("LICENSE-MIT.txt"),
            "MIT License\n\nCopyright (c) 2026 demo\n",
        )
        .unwrap();
        let fixer = FileCreateFixer::new(
            PathBuf::from("LICENSE"),
            ContentSourceSpec::File(PathBuf::from(".alint/templates/LICENSE-MIT.txt")),
            true,
        );
        let outcome = fixer
            .apply(&Violation::new("missing LICENSE"), &make_ctx(&tmp, false))
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let written = std::fs::read_to_string(tmp.path().join("LICENSE")).unwrap();
        assert!(written.starts_with("MIT License"));
        assert!(written.contains("Copyright (c) 2026"));
    }

    #[test]
    fn file_create_skips_when_content_from_missing() {
        // Missing source file produces a `Skipped` outcome
        // rather than aborting the whole fix run — same posture
        // as the rest of the fixer module.
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(
            PathBuf::from("LICENSE"),
            ContentSourceSpec::File(PathBuf::from("does/not/exist.txt")),
            true,
        );
        let outcome = fixer
            .apply(&Violation::new("missing"), &make_ctx(&tmp, false))
            .unwrap();
        let FixOutcome::Skipped(msg) = &outcome else {
            panic!("expected Skipped, got {outcome:?}")
        };
        assert!(msg.contains("could not be read"));
        // The target file should NOT have been created since
        // we skipped before the write.
        assert!(!tmp.path().join("LICENSE").exists());
    }

    #[test]
    fn file_prepend_with_content_from_reads_at_apply() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("hdr.txt"),
            "// SPDX-License-Identifier: MIT\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new(ContentSourceSpec::File(PathBuf::from("hdr.txt")));
        let outcome = fixer
            .apply(
                &Violation::new("missing header").with_path(PathBuf::from("a.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        let updated = std::fs::read_to_string(tmp.path().join("a.rs")).unwrap();
        assert!(updated.starts_with("// SPDX-License-Identifier: MIT\n"));
        assert!(updated.contains("fn main() {}"));
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
            FixOutcome::Applied(s) => {
                assert!(s.starts_with("would create"));
                assert!(s.contains("x.txt"), "summary must name the file: {s}");
            }
            FixOutcome::Skipped(_) => panic!("expected Applied"),
        }
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[test]
    fn file_prepend_inserts_at_start() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        fixer
            .apply(
                &Violation::new("missing header").with_path(std::path::Path::new("a.rs")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n"
        );
    }

    #[test]
    fn file_prepend_is_idempotent_across_runs() {
        // L4: a second `--fix` must NOT stack a duplicate header (the failure
        // mode when the content doesn't satisfy the rule's pattern).
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
        let fixer = FilePrependFixer::new("// Copyright 2026\n".into());
        let v = Violation::new("missing header").with_path(std::path::Path::new("a.rs"));
        let first = fixer.apply(&v, &make_ctx(&tmp, false)).unwrap();
        assert!(matches!(first, FixOutcome::Applied(_)));
        let second = fixer.apply(&v, &make_ctx(&tmp, false)).unwrap();
        assert!(matches!(second, FixOutcome::Skipped(_)), "second run skips");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
            "// Copyright 2026\nfn main() {}\n",
            "header not stacked"
        );
        // The editor path is idempotent too.
        let bytes = std::fs::read(tmp.path().join("a.rs")).unwrap();
        assert!(fixer.fix_edit(&v, &bytes, tmp.path()).is_none());
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
                &Violation::new("m").with_path(std::path::Path::new("x.txt")),
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
                &Violation::new("m").with_path(std::path::Path::new("a.rs")),
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
                &Violation::new("missing section").with_path(std::path::Path::new("notes.md")),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n"
        );
    }

    #[test]
    fn file_append_is_idempotent_across_runs() {
        // L4: a second `--fix` must NOT stack a duplicate footer.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("notes.md"), "# Notes\n").unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        let v = Violation::new("missing section").with_path(std::path::Path::new("notes.md"));
        assert!(matches!(
            fixer.apply(&v, &make_ctx(&tmp, false)).unwrap(),
            FixOutcome::Applied(_)
        ));
        assert!(
            matches!(
                fixer.apply(&v, &make_ctx(&tmp, false)).unwrap(),
                FixOutcome::Skipped(_)
            ),
            "second run skips"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("notes.md")).unwrap(),
            "# Notes\n\n## Section\n",
            "footer not stacked"
        );
    }

    #[test]
    fn file_append_dry_run_leaves_file_unchanged() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "orig\n").unwrap();
        FileAppendFixer::new("extra\n".into())
            .apply(
                &Violation::new("m").with_path(std::path::Path::new("x.txt")),
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

    #[test]
    fn file_create_fix_edit_returns_create_with_inline_content() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileCreateFixer::new(PathBuf::from("LICENSE"), "Apache-2.0\n".into(), true);
        let edit = fixer
            .fix_edit(&Violation::new("missing"), &[], tmp.path())
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::CreateFile {
                path: PathBuf::from("LICENSE"),
                content: b"Apache-2.0\n".to_vec(),
            }
        );
    }

    #[test]
    fn file_append_fix_edit_appends_payload() {
        let tmp = TempDir::new().unwrap();
        let fixer = FileAppendFixer::new("\n## Section\n".into());
        let edit = fixer
            .fix_edit(
                &Violation::new("m").with_path(std::path::Path::new("notes.md")),
                b"# Notes\n",
                tmp.path(),
            )
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: PathBuf::from("notes.md"),
                content: b"# Notes\n\n## Section\n".to_vec(),
            }
        );
    }

    #[test]
    fn file_prepend_fix_edit_inserts_before_existing_bytes() {
        let tmp = TempDir::new().unwrap();
        let fixer = FilePrependFixer::new("// header\n".into());
        let edit = fixer
            .fix_edit(
                &Violation::new("m").with_path(std::path::Path::new("a.rs")),
                b"fn main() {}\n",
                tmp.path(),
            )
            .unwrap();
        assert_eq!(
            edit,
            FixEdit::SetContent {
                path: PathBuf::from("a.rs"),
                content: b"// header\nfn main() {}\n".to_vec(),
            }
        );
    }
}
