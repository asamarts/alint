//! Inline-mode marker splicing.
//!
//! `alint export-agents-md --inline AGENTS.md` writes the
//! generated section between `<!-- alint:start -->` and
//! `<!-- alint:end -->` markers. Outside the markers the file
//! is left untouched — the user owns the prose, alint owns
//! the lint-rule directive block.
//!
//! Edge cases:
//!
//! - **No markers in the target.** Append the section to the
//!   end (with markers) and emit a stderr warning so scripted
//!   runs notice the auto-init. Subsequent runs find the
//!   markers we just wrote and splice cleanly.
//! - **Single open marker but no close.** Hard error — we'd
//!   have to guess where the user-managed prose resumes,
//!   which is the wrong default for a destructive op.
//! - **Multiple `<!-- alint:start -->` markers.** Hard error
//!   — same ambiguity.
//! - **Round-trip identity.** When the file's existing
//!   between-markers content already matches the new body,
//!   `splice_inline` writes nothing — the on-disk bytes are
//!   already correct, no need to bump mtime.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::markdown::{END_MARKER, START_MARKER};

/// Result of [`splice_inline`]. Surfaced primarily for the
/// CLI's stderr summary.
#[derive(Debug, PartialEq, Eq)]
pub enum SpliceOutcome {
    /// Markers were already present and the spliced content
    /// matched the on-disk bytes. No write performed.
    UnchangedRoundTrip,
    /// Markers were present and the section was rewritten in
    /// place.
    Spliced,
    /// File had no markers; the section was appended (with
    /// markers) at the end. Caller emits a one-time stderr
    /// warning so users notice the auto-init.
    Appended,
}

pub fn splice_inline(path: &Path, body: &str) -> Result<()> {
    let outcome = splice_inline_inner(path, body)?;
    if outcome == SpliceOutcome::Appended {
        eprintln!(
            "alint: appended new alint-managed section to {} \
             (no `{START_MARKER}` markers were found). \
             Subsequent --inline runs will splice in place.",
            alint_output::sanitize_terminal(&path.display().to_string()),
        );
    }
    Ok(())
}

/// Pure file-I/O step extracted for testability. The CLI
/// wrapper [`splice_inline`] adds the user-facing stderr
/// notification.
pub fn splice_inline_inner(path: &Path, body: &str) -> Result<SpliceOutcome> {
    // Body always starts with the start marker and ends with
    // the end marker (per `markdown::render`). We split it
    // into the inner region we want to inject.
    let inner = body.trim_end_matches('\n');
    if !inner.starts_with(START_MARKER) || !inner.ends_with(END_MARKER) {
        bail!(
            "cannot splice --inline content that is not delimited by `{START_MARKER}` and `{END_MARKER}`"
        );
    }

    crate::safe_write::refuse_symlink_or_special(path, "AGENTS.md")?;
    let existing =
        match alint_core::bounded_file::read_regular_capped(path, alint_core::MAX_ANALYZE_BYTES) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|e| {
                anyhow::anyhow!("reading {}: file is not valid UTF-8: {e}", path.display())
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("reading {}", path.display()));
            }
        };

    // Find marker pair. Multiple-start / orphan-start cases
    // are user-error and surface as hard errors rather than
    // silent overwrites.
    let start_count = existing.matches(START_MARKER).count();
    let end_count = existing.matches(END_MARKER).count();
    if start_count > 1 {
        bail!(
            "{}: contains {} `{START_MARKER}` markers; refusing to splice ambiguously. \
             Resolve to a single pair (or remove all of them and re-run).",
            path.display(),
            start_count,
        );
    }
    if end_count > 1 {
        bail!(
            "{}: contains {} `{END_MARKER}` markers; refusing to splice ambiguously. \
             Resolve to a single pair (or remove all of them and re-run).",
            path.display(),
            end_count,
        );
    }
    if start_count == 1 && end_count == 0 {
        bail!(
            "{}: has `{START_MARKER}` but no matching `{END_MARKER}`. \
             Resolve manually before re-running.",
            path.display(),
        );
    }
    if start_count == 0 && end_count >= 1 {
        bail!(
            "{}: has `{END_MARKER}` but no matching `{START_MARKER}`. \
             Resolve manually before re-running.",
            path.display(),
        );
    }

    if start_count == 0 {
        // Append: the file is preserved, our new section is
        // tacked on (with the markers) at the end. Re-runs
        // splice in place from then on.
        let mut new_contents = existing;
        if !new_contents.is_empty() && !new_contents.ends_with('\n') {
            new_contents.push('\n');
        }
        if !new_contents.is_empty() {
            new_contents.push('\n');
        }
        new_contents.push_str(inner);
        new_contents.push('\n');
        crate::safe_write::write_replacing(path, new_contents.as_bytes(), "AGENTS.md")?;
        return Ok(SpliceOutcome::Appended);
    }

    // Splice in place. Slice on byte indices.
    let start_idx = existing.find(START_MARKER).expect("checked count above");
    let end_idx = existing.find(END_MARKER).expect("checked count above") + END_MARKER.len();
    if end_idx <= start_idx {
        bail!(
            "{}: `{END_MARKER}` appears before `{START_MARKER}`; refusing to splice.",
            path.display(),
        );
    }

    let before = &existing[..start_idx];
    let after = &existing[end_idx..];
    // The body's leading/trailing newlines belong to the
    // injected region; we don't add extra here.
    let new_contents = format!("{before}{inner}{after}");

    if new_contents == existing {
        return Ok(SpliceOutcome::UnchangedRoundTrip);
    }
    crate::safe_write::write_replacing(path, new_contents.as_bytes(), "AGENTS.md")?;
    Ok(SpliceOutcome::Spliced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn td() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("alint-splice-")
            .tempdir()
            .unwrap()
    }

    fn body() -> String {
        format!("{START_MARKER}\n\n## section\n\nbullet\n\n{END_MARKER}\n")
    }

    #[test]
    fn appends_when_target_has_no_markers() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, "# Existing\n\nSome prose.\n").unwrap();
        let outcome = splice_inline_inner(&path, &body()).unwrap();
        assert_eq!(outcome, SpliceOutcome::Appended);
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.starts_with("# Existing"));
        assert!(after.contains(START_MARKER));
        assert!(after.contains(END_MARKER));
    }

    #[test]
    fn appends_to_empty_or_missing_file() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        let outcome = splice_inline_inner(&path, &body()).unwrap();
        assert_eq!(outcome, SpliceOutcome::Appended);
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains(START_MARKER));
        assert!(after.contains(END_MARKER));
    }

    #[test]
    fn splices_in_place_when_markers_present() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(
            &path,
            format!(
                "# header\n\nprose before\n\n{START_MARKER}\n\nold body\n\n{END_MARKER}\n\nprose after\n"
            ),
        )
        .unwrap();
        let outcome = splice_inline_inner(&path, &body()).unwrap();
        assert_eq!(outcome, SpliceOutcome::Spliced);
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("prose before"));
        assert!(after.contains("prose after"));
        assert!(after.contains("## section"));
        assert!(!after.contains("old body"));
    }

    #[test]
    fn round_trip_identity_no_write() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        // Materialise the exact body with no surrounding prose.
        fs::write(&path, body()).unwrap();
        let outcome = splice_inline_inner(&path, &body()).unwrap();
        assert_eq!(outcome, SpliceOutcome::UnchangedRoundTrip);
    }

    #[test]
    fn rejects_multiple_start_markers() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(
            &path,
            format!(
                "{START_MARKER}\nfirst\n{END_MARKER}\n\n{START_MARKER}\nsecond\n{END_MARKER}\n"
            ),
        )
        .unwrap();
        let err = splice_inline_inner(&path, &body()).unwrap_err();
        assert!(err.to_string().contains("ambiguously"), "unexpected: {err}");
    }

    #[test]
    fn rejects_orphan_start() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, format!("{START_MARKER}\nopen but no close\n")).unwrap();
        let err = splice_inline_inner(&path, &body()).unwrap_err();
        assert!(err.to_string().contains("no matching"), "unexpected: {err}");
    }

    #[test]
    fn rejects_orphan_end() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, format!("only end {END_MARKER}\n")).unwrap();
        let err = splice_inline_inner(&path, &body()).unwrap_err();
        assert!(err.to_string().contains("no matching"), "unexpected: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_to_splice_through_a_symlink() {
        let tmp = td();
        let target = tmp.path().join("outside.md");
        fs::write(&target, "untouched\n").unwrap();
        let link = tmp.path().join("AGENTS.md");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = splice_inline_inner(&link, &body()).unwrap_err();
        assert!(err.to_string().contains("symlink"), "unexpected: {err}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "untouched\n");
    }

    #[test]
    fn rejects_multiple_end_markers() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        fs::write(
            &path,
            format!("{START_MARKER}\nfirst\n{END_MARKER}\nextra\n{END_MARKER}\n"),
        )
        .unwrap();
        let err = splice_inline_inner(&path, &body()).unwrap_err();
        assert!(
            err.to_string().contains(END_MARKER) && err.to_string().contains("ambiguously"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn rejects_markerless_body_at_runtime() {
        let tmp = td();
        let path = tmp.path().join("AGENTS.md");
        let err = splice_inline_inner(&path, "{\"format\":\"agents-md\"}\n").unwrap_err();
        assert!(
            err.to_string().contains("not delimited"),
            "unexpected: {err}"
        );
    }
}
