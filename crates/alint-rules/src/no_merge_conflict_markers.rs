//! `no_merge_conflict_markers` — flag files that still carry
//! unresolved git-merge conflict markers.
//!
//! Canonical markers (all appear at the start of a line):
//!   - `<<<<<<< <ref>` — start of "ours"
//!   - `|||||||  <base>` — common ancestor (merge.conflictstyle=diff3)
//!   - `=======`        — separator
//!   - `>>>>>>> <ref>`  — start of "theirs"
//!
//! Check-only: resolving a conflict requires human judgment, so
//! no auto-fix exists.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};

#[derive(Debug)]
pub struct NoMergeConflictMarkersRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for NoMergeConflictMarkersRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for NoMergeConflictMarkersRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // A binary-looking file that is not valid UTF-8 (image, archive) is
        // skipped, as before the lossy decode: random bytes can spell a marker
        // line. Valid-UTF-8 text with a NUL is still scanned.
        if crate::io::char_scan_mode(bytes) == crate::io::CharScan::Skip {
            return Ok(Vec::new());
        }
        // Decode lossily: one stray non-UTF-8 byte must not hide the markers
        // (they are ASCII, so an invalid byte -> U+FFFD never creates or masks
        // one, and line numbers are unchanged).
        let text = String::from_utf8_lossy(bytes);
        let Some((line_no, marker)) = first_marker(&text) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| {
            format!("unresolved merge conflict marker on line {line_no}: {marker:?}")
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(line_no, 1),
        ])
    }
}

/// True when the file carries an unambiguous conflict *anchor* - a
/// line starting with `<<<<<<< `, `>>>>>>> `, or `||||||| ` (each
/// followed by a ref, so it never collides with prose). A bare
/// `=======` is only treated as a separator when such an anchor is
/// present: on its own, a seven-character `=======` is identical to
/// a reST/Markdown setext heading underline (`Changes` and `Git tag`
/// are both exactly seven characters), and a real conflict always
/// carries a `<<<<<<<` start anyway.
fn has_conflict_anchor(text: &str) -> bool {
    text.split('\n').any(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let head = line.as_bytes();
        head.len() >= 8
            && (head.starts_with(b"<<<<<<< ")
                || head.starts_with(b">>>>>>> ")
                || head.starts_with(b"||||||| "))
    })
}

/// Scan `text` for the first line that matches one of the four
/// git conflict marker prefixes. Returns (1-based line number,
/// the marker token).
fn first_marker(text: &str) -> Option<(usize, &'static str)> {
    let separator_armed = has_conflict_anchor(text);
    for (idx, line) in text.split('\n').enumerate() {
        let trimmed_cr = line.strip_suffix('\r').unwrap_or(line);
        if let Some(marker) = classify_marker(trimmed_cr, separator_armed) {
            return Some((idx + 1, marker));
        }
    }
    None
}

fn classify_marker(line: &str, separator_armed: bool) -> Option<&'static str> {
    // `<<<<<<< `, `>>>>>>> `, `||||||| ` are 7 + space + ref.
    // `=======` is 7 chars, EXACTLY the whole line.
    if line.len() >= 8 {
        let head = line.as_bytes();
        if head.starts_with(b"<<<<<<< ") {
            return Some("<<<<<<<");
        }
        if head.starts_with(b">>>>>>> ") {
            return Some(">>>>>>>");
        }
        if head.starts_with(b"||||||| ") {
            return Some("|||||||");
        }
    }
    // Only a real separator inside an actual conflict; standalone it
    // is a setext heading underline. See `has_conflict_anchor`.
    if separator_armed && line == "=======" {
        return Some("=======");
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "no_merge_conflict_markers requires a `paths` field",
        )
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_merge_conflict_markers has no fix op - conflict resolution requires human judgment",
        ));
    }
    Ok(Box::new(NoMergeConflictMarkersRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_ours_marker() {
        assert_eq!(
            first_marker("clean\n<<<<<<< HEAD\nconflict\n"),
            Some((2, "<<<<<<<"))
        );
    }

    #[test]
    fn flags_separator_marker() {
        assert_eq!(
            first_marker("<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n"),
            Some((1, "<<<<<<<"))
        );
    }

    #[test]
    fn flags_diff3_base_marker() {
        assert_eq!(first_marker("||||||| base\nshared\n"), Some((1, "|||||||")));
    }

    #[test]
    fn ignores_marker_not_at_line_start() {
        assert_eq!(
            first_marker("leading text <<<<<<< HEAD\n"),
            None,
            "markers must be at column 1"
        );
    }

    #[test]
    fn ignores_short_runs() {
        // Six `<` is not a marker.
        assert_eq!(first_marker("<<<<<< HEAD\n"), None);
    }

    #[test]
    fn clean_file_is_silent() {
        assert_eq!(first_marker("no markers here\njust code\n"), None);
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        assert_eq!(
            first_marker("clean\r\n<<<<<<< HEAD\r\nconflict\r\n"),
            Some((2, "<<<<<<<"))
        );
    }

    #[test]
    fn bare_separator_alone_is_setext_not_conflict() {
        // A 7-char reST/Markdown setext heading underline — "Changes"
        // and "Git tag" are each exactly seven chars — is NOT a
        // conflict separator absent any anchor marker. This is the
        // flask/django `docs/**` false-positive class.
        assert_eq!(first_marker("Changes\n=======\n\nThe changes.\n"), None);
        assert_eq!(first_marker("Git tag\n=======\n"), None);
    }

    #[test]
    fn real_conflict_with_a_setext_heading_still_fires() {
        // The file carries a genuine conflict AND a setext heading;
        // the anchor arms detection so it is still flagged (at the
        // `<<<` start, which precedes the heading here).
        assert_eq!(
            first_marker("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n\nChanges\n=======\n"),
            Some((1, "<<<<<<<"))
        );
    }

    #[test]
    fn separator_without_ours_is_caught_when_anchor_present() {
        // A conflict missing its `<<<` start but keeping `=======` +
        // `>>>>>>>` is still a conflict — the `>>>>>>>` anchor arms
        // the bare separator (this is why we keep the `=======` arm
        // rather than dropping it).
        assert_eq!(
            first_marker("=======\ntheirs\n>>>>>>> branch\n"),
            Some((1, "======="))
        );
    }
}

#[cfg(test)]
mod non_utf8_tests {
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn non_utf8_text_is_still_scanned_for_markers() {
        // Fail-closed regression: one Latin-1 byte used to skip the whole file.
        let rule = super::build(&spec_yaml(
            "id: t\nkind: no_merge_conflict_markers\npaths: \"**/*\"\nlevel: error\n",
        ))
        .unwrap();
        let body: &[u8] = b"caf\xe9\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n";
        let (tmp, idx) = tempdir_with_files(&[("a.txt", body)]);
        let vs = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].line, Some(2));
    }

    #[test]
    fn invalid_utf8_binaries_are_skipped() {
        // Regression: the lossy decode scanned binaries, so marker-shaped bytes
        // in an archive were reported.
        let rule = super::build(&spec_yaml(
            "id: t\nkind: no_merge_conflict_markers\npaths: \"**/*\"\nlevel: error\n",
        ))
        .unwrap();
        let blob: &[u8] = b"\x00\xff\xfe\n<<<<<<< HEAD\nx\n>>>>>>> b\n\xc3";
        let (tmp, idx) = tempdir_with_files(&[("a.bin", blob)]);
        assert!(rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }
}
