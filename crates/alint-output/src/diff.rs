//! Unified-diff rendering for `alint fix --diff`: a git-style diff of the
//! would-apply edits, computed with `similar`. A text file gets a line-level
//! unified diff; a file whose content isn't UTF-8 gets a one-line binary
//! summary (a line diff of binary is not meaningful). The header follows git
//! conventions so the output pastes into standard diff tooling: `a/<path>` and
//! `b/<path>` for an in-place edit, `/dev/null` on the absent side of a create
//! or delete, and explicit `rename from` / `rename to` lines for a rename.

use std::io::Write;

use alint_core::{StagedFix, StagedKind};
use similar::TextDiff;

/// Write a unified diff for every staged fix. No staged fixes writes nothing
/// (an empty diff; the exit code is still driven by the report). See the module
/// docs for the header conventions.
///
/// # Errors
/// Propagates any write error from `w`.
pub fn write_fix_diff(staged: &[StagedFix], w: &mut dyn Write) -> std::io::Result<()> {
    for fix in staged {
        let path = fix.path.display();
        // Non-UTF-8 on either side: a line diff isn't meaningful, so summarize.
        let (Ok(old), Ok(new)) = (std::str::from_utf8(&fix.old), std::str::from_utf8(&fix.new))
        else {
            write_binary_summary(fix, w)?;
            continue;
        };
        match &fix.kind {
            StagedKind::Modify => {
                write_hunks(&format!("a/{path}"), &format!("b/{path}"), old, new, w)?;
            }
            // New file: the `---` side is `/dev/null` (git's create convention).
            // `similar` emits nothing (header included) when the two sides are
            // equal, so a content-less create (an empty marker: `.keep`,
            // `py.typed`, empty `__init__.py`) would render as blank output. Emit
            // the header explicitly in that case so the created file is visible.
            StagedKind::Create => {
                if new.is_empty() {
                    writeln!(w, "--- /dev/null")?;
                    writeln!(w, "+++ b/{path}")?;
                } else {
                    write_hunks("/dev/null", &format!("b/{path}"), old, new, w)?;
                }
            }
            // Removed file: the `+++` side is `/dev/null`. Same empty-file guard
            // as Create (deleting an already-empty file).
            StagedKind::Delete => {
                if old.is_empty() {
                    writeln!(w, "--- a/{path}")?;
                    writeln!(w, "+++ /dev/null")?;
                } else {
                    write_hunks(&format!("a/{path}"), "/dev/null", old, new, w)?;
                }
            }
            StagedKind::Rename { from } => {
                writeln!(w, "rename from {}", from.display())?;
                writeln!(w, "rename to {path}")?;
                // A pure rename has no content hunk; only emit one when the
                // rename also rewrites the file.
                if fix.old != fix.new {
                    write_hunks(
                        &format!("a/{}", from.display()),
                        &format!("b/{path}"),
                        old,
                        new,
                        w,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Write the `--- <a>` / `+++ <b>` header and unified hunks for one file.
/// `similar` emits nothing when `old == new`, so an unchanged file is silent.
fn write_hunks(a: &str, b: &str, old: &str, new: &str, w: &mut dyn Write) -> std::io::Result<()> {
    let diff = TextDiff::from_lines(old, new);
    let mut unified = diff.unified_diff();
    write!(w, "{}", unified.header(a, b))
}

/// One-line summary for a staged change whose content isn't UTF-8, per kind.
fn write_binary_summary(fix: &StagedFix, w: &mut dyn Write) -> std::io::Result<()> {
    let path = fix.path.display();
    match &fix.kind {
        StagedKind::Create => writeln!(w, "Binary file {path} created ({} bytes)", fix.new.len()),
        StagedKind::Delete => writeln!(w, "Binary file {path} deleted ({} bytes)", fix.old.len()),
        StagedKind::Rename { from } => {
            writeln!(w, "Binary file renamed {} -> {path}", from.display())
        }
        StagedKind::Modify => writeln!(
            w,
            "Binary file {path} differs ({} -> {} bytes)",
            fix.old.len(),
            fix.new.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn render(staged: &[StagedFix]) -> String {
        let mut buf = Vec::new();
        write_fix_diff(staged, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn fix(path: &str, old: &str, new: &str, kind: StagedKind) -> StagedFix {
        StagedFix {
            path: PathBuf::from(path),
            old: old.as_bytes().to_vec(),
            new: new.as_bytes().to_vec(),
            kind,
        }
    }

    #[test]
    fn modify_uses_a_b_headers() {
        let out = render(&[fix("src/x.rs", "a\nb\n", "a\nB\n", StagedKind::Modify)]);
        assert!(out.contains("--- a/src/x.rs"), "{out}");
        assert!(out.contains("+++ b/src/x.rs"), "{out}");
        assert!(out.contains("-b") && out.contains("+B"), "{out}");
    }

    #[test]
    fn create_uses_dev_null_on_the_old_side() {
        let out = render(&[fix("NEW.md", "", "hi\n", StagedKind::Create)]);
        assert!(out.contains("--- /dev/null"), "{out}");
        assert!(out.contains("+++ b/NEW.md"), "{out}");
        assert!(out.contains("+hi"), "{out}");
    }

    #[test]
    fn delete_uses_dev_null_on_the_new_side() {
        let out = render(&[fix("gone.log", "bye\n", "", StagedKind::Delete)]);
        assert!(out.contains("--- a/gone.log"), "{out}");
        assert!(out.contains("+++ /dev/null"), "{out}");
        assert!(out.contains("-bye"), "{out}");
    }

    #[test]
    fn pure_rename_emits_only_the_rename_headers() {
        let body = "keep\n";
        let out = render(&[fix(
            "b.rs",
            body,
            body,
            StagedKind::Rename {
                from: PathBuf::from("A.rs"),
            },
        )]);
        assert!(out.contains("rename from A.rs"), "{out}");
        assert!(out.contains("rename to b.rs"), "{out}");
        // Content unchanged: no +/- content hunk.
        assert!(
            !out.contains("@@"),
            "pure rename has no content hunk; {out}"
        );
    }

    #[test]
    fn rename_with_content_change_shows_both() {
        let out = render(&[fix(
            "b.rs",
            "old\n",
            "new\n",
            StagedKind::Rename {
                from: PathBuf::from("A.rs"),
            },
        )]);
        assert!(out.contains("rename from A.rs"), "{out}");
        assert!(
            out.contains("--- a/A.rs") && out.contains("+++ b/b.rs"),
            "{out}"
        );
        assert!(out.contains("-old") && out.contains("+new"), "{out}");
    }

    #[test]
    fn non_utf8_content_falls_back_to_a_binary_summary() {
        let staged = StagedFix {
            path: PathBuf::from("blob.bin"),
            old: vec![0xff, 0xfe],
            new: vec![0x00, 0x01, 0x02],
            kind: StagedKind::Modify,
        };
        let out = render(&[staged]);
        assert!(
            out.contains("Binary file blob.bin differs (2 -> 3 bytes)"),
            "{out}"
        );
    }

    #[test]
    fn no_staged_fixes_renders_nothing() {
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn empty_file_create_still_emits_a_header() {
        // An empty marker create (.keep / py.typed) must still appear in the
        // diff, even though `similar` renders nothing for two equal empty sides.
        let out = render(&[fix("NEW.keep", "", "", StagedKind::Create)]);
        assert!(
            out.contains("--- /dev/null"),
            "empty create header missing: {out:?}"
        );
        assert!(out.contains("+++ b/NEW.keep"), "{out:?}");
    }

    #[test]
    fn empty_file_delete_still_emits_a_header() {
        let out = render(&[fix("gone.empty", "", "", StagedKind::Delete)]);
        assert!(
            out.contains("--- a/gone.empty"),
            "empty delete header missing: {out:?}"
        );
        assert!(out.contains("+++ /dev/null"), "{out:?}");
    }
}
