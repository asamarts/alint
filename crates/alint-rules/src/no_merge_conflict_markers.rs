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

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct NoMergeConflictMarkersRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for NoMergeConflictMarkersRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
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
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let Some((line_no, marker)) = first_marker(text) else {
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

/// Scan `text` for the first line that matches one of the four
/// git conflict marker prefixes. Returns (1-based line number,
/// the marker token).
fn first_marker(text: &str) -> Option<(usize, &'static str)> {
    for (idx, line) in text.split('\n').enumerate() {
        let trimmed_cr = line.strip_suffix('\r').unwrap_or(line);
        if let Some(marker) = classify_marker(trimmed_cr) {
            return Some((idx + 1, marker));
        }
    }
    None
}

fn classify_marker(line: &str) -> Option<&'static str> {
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
    if line == "=======" {
        return Some("=======");
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "no_merge_conflict_markers requires a `paths` field",
        )
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_merge_conflict_markers has no fix op — conflict resolution requires human judgment",
        ));
    }
    Ok(Box::new(NoMergeConflictMarkersRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
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
}
