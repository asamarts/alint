//! Shared [`Fixer`] implementations.
//!
//! Each fixer is a small, rule-agnostic helper: rule builders (e.g.
//! `file_exists`, `file_absent`) decide whether the configured `fix:`
//! op makes sense for their kind and, if so, construct one of the
//! fixers here and attach it to the built rule.

use std::io::Write;
use std::path::PathBuf;

use alint_core::{Error, FixContext, FixOutcome, Fixer, Result, Violation};

use crate::case::CaseConvention;

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
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
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
        if let Some(skip) = alint_core::check_fix_size(&abs, path, ctx)? {
            return Ok(skip);
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
                "would rename {} → {}",
                path.display(),
                new_path.display()
            )));
        }
        std::fs::rename(&abs_from, &abs_to).map_err(|source| Error::Io {
            path: abs_from,
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "renamed {} → {}",
            path.display(),
            new_path.display()
        )))
    }
}

/// Strips trailing space/tab on every line of each violating
/// file. Preserves original line endings (LF stays LF, CRLF
/// stays CRLF).
#[derive(Debug)]
pub struct FileTrimTrailingWhitespaceFixer;

impl Fixer for FileTrimTrailingWhitespaceFixer {
    fn describe(&self) -> String {
        "strip trailing whitespace on every line".to_string()
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
                "would trim trailing whitespace in {}",
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let Ok(text) = std::str::from_utf8(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot trim",
                path.display()
            )));
        };
        let trimmed = strip_trailing_whitespace(text);
        if trimmed.as_bytes() == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already clean",
                path.display()
            )));
        }
        std::fs::write(&abs, trimmed.as_bytes()).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "trimmed trailing whitespace in {}",
            path.display()
        )))
    }
}

fn strip_trailing_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        // Preserve CR before the (upcoming) LF so CRLF endings survive.
        let (body, cr) = match line.strip_suffix('\r') {
            Some(stripped) => (stripped, "\r"),
            None => (line, ""),
        };
        out.push_str(body.trim_end_matches([' ', '\t']));
        out.push_str(cr);
    }
    out
}

/// Appends a single `\n` byte when a file has content but
/// doesn't end with one.
#[derive(Debug)]
pub struct FileAppendFinalNewlineFixer;

impl Fixer for FileAppendFinalNewlineFixer {
    fn describe(&self) -> String {
        "append final newline when missing".to_string()
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
                "would append final newline to {}",
                path.display()
            )));
        }
        if let Some(skip) = alint_core::check_fix_size(&abs, path, ctx)? {
            return Ok(skip);
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&abs)
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        f.write_all(b"\n").map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "appended final newline to {}",
            path.display()
        )))
    }
}

/// Which line ending [`FileNormalizeLineEndingsFixer`] rewrites to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndingTarget {
    Lf,
    Crlf,
}

impl LineEndingTarget {
    pub fn name(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::Crlf => b"\r\n",
        }
    }
}

/// Rewrites every line ending in a file to the target (`lf` or `crlf`).
#[derive(Debug)]
pub struct FileNormalizeLineEndingsFixer {
    target: LineEndingTarget,
}

impl FileNormalizeLineEndingsFixer {
    pub fn new(target: LineEndingTarget) -> Self {
        Self { target }
    }
}

impl Fixer for FileNormalizeLineEndingsFixer {
    fn describe(&self) -> String {
        format!("normalize line endings to {}", self.target.name())
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
                "would normalize line endings in {} to {}",
                path.display(),
                self.target.name()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let normalized = normalize_line_endings(&existing, self.target);
        if normalized == existing {
            return Ok(FixOutcome::Skipped(format!(
                "{} already {}",
                path.display(),
                self.target.name()
            )));
        }
        std::fs::write(&abs, &normalized).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "normalized {} to {}",
            path.display(),
            self.target.name()
        )))
    }
}

fn normalize_line_endings(bytes: &[u8], target: LineEndingTarget) -> Vec<u8> {
    let target_bytes = target.bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Drop a preceding CR so `\r\n` collapses to `\n` before
            // we emit the target.
            if out.last().copied() == Some(b'\r') {
                out.pop();
            }
            out.extend_from_slice(target_bytes);
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
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

    #[test]
    fn file_rename_converts_stem_preserving_extension() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "fn main() {}\n").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("FooBar.rs"),
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
                &Violation::new("case").with_path("src/MyModule.rs"),
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
                &Violation::new("case").with_path("foo_bar.rs"),
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
                &Violation::new("case").with_path("FooBar.rs"),
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
    fn file_rename_dry_run_does_not_touch_disk() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("FooBar.rs"), "").unwrap();
        FileRenameFixer::new(CaseConvention::Snake)
            .apply(
                &Violation::new("case").with_path("FooBar.rs"),
                &make_ctx(&tmp, true),
            )
            .unwrap();
        assert!(tmp.path().join("FooBar.rs").exists());
        assert!(!tmp.path().join("foo_bar.rs").exists());
    }

    // ── text-hygiene fixers ────────────────────────────────────

    #[test]
    fn strip_trailing_whitespace_preserves_lf_and_crlf() {
        assert_eq!(strip_trailing_whitespace("a  \nb\t\n"), "a\nb\n");
        assert_eq!(strip_trailing_whitespace("a  \r\nb\t\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn file_trim_trailing_whitespace_rewrites_in_place() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.rs"), "let _ = 1;   \n").unwrap();
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(
                &Violation::new("ws").with_path("x.rs"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.rs")).unwrap(),
            "let _ = 1;\n"
        );
    }

    #[test]
    fn file_trim_trailing_whitespace_honors_size_limit() {
        let tmp = TempDir::new().unwrap();
        let big = "x   \n".repeat(2_000);
        std::fs::write(tmp.path().join("big.txt"), &big).unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: Some(100),
        };
        let outcome = FileTrimTrailingWhitespaceFixer
            .apply(&Violation::new("ws").with_path("big.txt"), &ctx)
            .unwrap();
        match outcome {
            FixOutcome::Skipped(reason) => {
                assert!(reason.contains("fix_size_limit"), "{reason}");
            }
            FixOutcome::Applied(_) => panic!("expected Skipped on oversized file"),
        }
        // Disk unchanged.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("big.txt")).unwrap(),
            big
        );
    }

    #[test]
    fn file_append_final_newline_adds_missing_newline() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.txt"), "hello").unwrap();
        FileAppendFinalNewlineFixer
            .apply(
                &Violation::new("eof").with_path("x.txt"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn normalize_line_endings_lf_target() {
        let mixed = b"a\r\nb\nc\r\nd".to_vec();
        let out = normalize_line_endings(&mixed, LineEndingTarget::Lf);
        assert_eq!(out, b"a\nb\nc\nd");
    }

    #[test]
    fn normalize_line_endings_crlf_target() {
        let mixed = b"a\r\nb\nc\r\nd".to_vec();
        let out = normalize_line_endings(&mixed, LineEndingTarget::Crlf);
        assert_eq!(out, b"a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn file_normalize_line_endings_rewrites_to_lf() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.md"), "one\r\ntwo\r\n").unwrap();
        FileNormalizeLineEndingsFixer::new(LineEndingTarget::Lf)
            .apply(
                &Violation::new("le").with_path("a.md"),
                &make_ctx(&tmp, false),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.md")).unwrap(),
            "one\ntwo\n"
        );
    }
}
