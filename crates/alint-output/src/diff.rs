//! Unified-diff rendering for `alint fix --diff`: a git-style diff of the
//! would-apply edits, computed with `similar`. A text file gets a line-level
//! unified diff; a file whose content isn't UTF-8 gets a one-line binary
//! summary (a line diff of binary is not meaningful). The header follows git
//! conventions so the output is consumable by `git apply`: a traditional
//! `a/<path>` / `b/<path>` unified diff for an in-place edit, `/dev/null` on the
//! absent side of a create or delete, and a full `diff --git` envelope with
//! `similarity index` / `rename from` / `rename to` for a rename (a git-only
//! construct that `git apply` ignores -- silently, or by rejecting the whole
//! patch -- unless wrapped in the envelope). Paths containing a control byte
//! (a tab is git's field separator; a newline ends the line), a double quote,
//! or a backslash are C-quoted the way git's `core.quotePath` does, so an
//! unusual filename cannot corrupt the header.

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
        let path = fix.path.display().to_string();
        // Non-UTF-8 on either side: a line diff isn't meaningful, so summarize.
        let (Ok(old), Ok(new)) = (std::str::from_utf8(&fix.old), std::str::from_utf8(&fix.new))
        else {
            write_binary_summary(fix, w)?;
            continue;
        };
        match &fix.kind {
            StagedKind::Modify => {
                write_hunks(&a(&path), &b(&path), old, new, w)?;
            }
            // New file: the `---` side is `/dev/null` (git's create convention).
            // `similar` emits nothing (header included) when the two sides are
            // equal, so a content-less create (an empty marker: `.keep`,
            // `py.typed`, empty `__init__.py`) would render as blank output. Emit
            // the header explicitly in that case so the created file is visible.
            StagedKind::Create => {
                if new.is_empty() {
                    writeln!(w, "--- /dev/null")?;
                    writeln!(w, "+++ {}", b(&path))?;
                } else {
                    write_hunks("/dev/null", &b(&path), old, new, w)?;
                }
            }
            // Removed file: the `+++` side is `/dev/null`. Same empty-file guard
            // as Create (deleting an already-empty file).
            StagedKind::Delete => {
                if old.is_empty() {
                    writeln!(w, "--- {}", a(&path))?;
                    writeln!(w, "+++ /dev/null")?;
                } else {
                    write_hunks(&a(&path), "/dev/null", old, new, w)?;
                }
            }
            // A rename is a git-only construct: bare `rename from`/`rename to`
            // lines are silently ignored (or reject the whole patch) unless
            // wrapped in a `diff --git` envelope, so emit the full git form that
            // `git apply` accepts -- `similarity index 100%` for a pure rename,
            // otherwise the rename headers plus the content hunks.
            StagedKind::Rename { from } => {
                let from = from.display().to_string();
                writeln!(
                    w,
                    "diff --git {} {}",
                    git_quote_path(&format!("a/{from}")),
                    git_quote_path(&format!("b/{path}"))
                )?;
                if fix.old == fix.new {
                    writeln!(w, "similarity index 100%")?;
                }
                writeln!(w, "rename from {}", git_quote_path(&from))?;
                writeln!(w, "rename to {}", git_quote_path(&path))?;
                if fix.old != fix.new {
                    write_hunks(&a(&from), &b(&path), old, new, w)?;
                }
            }
        }
    }
    Ok(())
}

/// `a/<path>`, git-C-quoted if the path needs it (the `---` / `diff --git` side).
fn a(path: &str) -> String {
    git_quote_path(&format!("a/{path}"))
}

/// `b/<path>`, git-C-quoted if the path needs it (the `+++` / `diff --git` side).
fn b(path: &str) -> String {
    git_quote_path(&format!("b/{path}"))
}

/// Quote a path for a git diff header the way git's `quote_c_style` does, so a
/// path containing a control byte (a tab is git's field separator; a newline
/// ends the line), a double quote, or a backslash cannot corrupt the header or
/// be silently mis-parsed by `git apply`. A path needing no quoting is returned
/// unchanged; when quoting IS triggered, high bytes (>= 0x80) are octal-escaped
/// too so the quoted form is unambiguous (matching git). A path whose only
/// unusual bytes are high (plain non-ASCII, no control/quote/backslash) is left
/// raw -- `git apply` accepts raw UTF-8 -- to keep common names readable.
fn git_quote_path(s: &str) -> String {
    let needs_quote = s
        .bytes()
        .any(|byte| byte < 0x20 || byte == b'"' || byte == b'\\' || byte == 0x7f);
    if !needs_quote {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for &byte in s.as_bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(byte as char),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out.push('"');
    out
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
            if fix.old == fix.new {
                writeln!(w, "Binary file renamed {} -> {path}", from.display())
            } else {
                writeln!(
                    w,
                    "Binary file renamed {} -> {path} ({} -> {} bytes)",
                    from.display(),
                    fix.old.len(),
                    fix.new.len()
                )
            }
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
    fn pure_rename_emits_a_git_rename_envelope() {
        let body = "keep\n";
        let out = render(&[fix(
            "b.rs",
            body,
            body,
            StagedKind::Rename {
                from: PathBuf::from("A.rs"),
            },
        )]);
        // The `diff --git` envelope + `similarity index 100%` is what makes a
        // rename consumable by `git apply` (bare rename lines are silently
        // dropped). Round-5 audit regression.
        assert!(out.contains("diff --git a/A.rs b/b.rs"), "{out}");
        assert!(out.contains("similarity index 100%"), "{out}");
        assert!(out.contains("rename from A.rs"), "{out}");
        assert!(out.contains("rename to b.rs"), "{out}");
        // Content unchanged: no +/- content hunk.
        assert!(
            !out.contains("@@"),
            "pure rename has no content hunk; {out}"
        );
    }

    #[test]
    fn rename_with_content_change_shows_envelope_and_hunks() {
        let out = render(&[fix(
            "b.rs",
            "old\n",
            "new\n",
            StagedKind::Rename {
                from: PathBuf::from("A.rs"),
            },
        )]);
        assert!(out.contains("diff --git a/A.rs b/b.rs"), "{out}");
        assert!(out.contains("rename from A.rs"), "{out}");
        // A content-changing rename is NOT 100% similar.
        assert!(!out.contains("similarity index 100%"), "{out}");
        assert!(
            out.contains("--- a/A.rs") && out.contains("+++ b/b.rs"),
            "{out}"
        );
        assert!(out.contains("-old") && out.contains("+new"), "{out}");
    }

    #[test]
    fn header_paths_are_git_c_quoted_when_they_contain_control_bytes() {
        // A tab is git's field separator and a newline ends the line; an
        // unquoted such path corrupts the header. git C-quotes them.
        let out = render(&[fix("ta\tb.rs", "a\n", "b\n", StagedKind::Modify)]);
        assert!(out.contains("--- \"a/ta\\tb.rs\""), "{out}");
        assert!(out.contains("+++ \"b/ta\\tb.rs\""), "{out}");
        // A plain path (no control/quote/backslash) is left unquoted.
        assert_eq!(git_quote_path("a/src/x.rs"), "a/src/x.rs");
        // Quote + backslash are escaped; a high byte octal-escaped once quoting fires.
        assert_eq!(git_quote_path("a/x\"y\\z"), "\"a/x\\\"y\\\\z\"");
        assert_eq!(git_quote_path("a/\tcaf\u{e9}"), "\"a/\\tcaf\\303\\251\"");
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
