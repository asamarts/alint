//! `file_max_lines` — files in scope must have AT MOST
//! `max_lines` lines. Mirror of [`crate::file_min_lines`];
//! shares the line-counting semantics so the two compose
//! cleanly when both are applied to the same file.
//!
//! Catches the "everything-module" anti-pattern — a single
//! `lib.rs` / `index.ts` / `helpers.py` that grew until it
//! does the work of a half-dozen smaller files. The threshold
//! is intentionally a soft signal rather than a hard limit;
//! we ship it at `level: warning` in tutorials and rulesets,
//! and leave the cap value to the team.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Maximum allowed line count.
    max_lines: u64,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct FileMaxLinesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_lines: u64,
}

impl Rule for FileMaxLinesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileMaxLinesRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let lines = count_lines(bytes);
        if lines <= self.max_lines {
            return Ok(Vec::new());
        }
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "file has {} line(s); at most {} allowed",
                lines, self.max_lines,
            )
        });
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }
}

/// Count lines with the same `wc -l`-style semantics as
/// `file_min_lines::count_lines`. Kept as a private function
/// here (rather than reused from `file_min_lines`) because
/// inlining it makes the unit tests explicit about what this
/// rule's threshold is being compared against - and the
/// implementation is one line, not worth a cross-module dep.
fn count_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    #[allow(clippy::naive_bytecount)]
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    let trailing_unterminated = u64::from(!bytes.ends_with(b"\n"));
    newlines + trailing_unterminated
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_max_lines requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    Ok(Box::new(FileMaxLinesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        max_lines: opts.max_lines,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};

    #[test]
    fn empty_file_is_zero_lines() {
        assert_eq!(count_lines(b""), 0);
    }

    #[test]
    fn matches_min_lines_semantics() {
        // Identical accounting to file_min_lines so the two
        // rules agree on what "this file has N lines" means.
        assert_eq!(count_lines(b"a\n"), 1);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b"a\nb"), 2);
        assert_eq!(count_lines(b"\n\n"), 2);
        assert_eq!(count_lines(b"a\n\nb\n"), 3);
    }

    #[test]
    fn build_rejects_missing_paths_and_unknown_option() {
        let no_paths = spec_yaml("id: t\nkind: file_max_lines\nmax_lines: 10\nlevel: warning\n");
        let err = build(&no_paths).unwrap_err().to_string();
        assert!(err.contains("paths"), "{err}");

        let unknown = spec_yaml(
            "id: t\nkind: file_max_lines\npaths: \"*.rs\"\nmax_lines: 10\nbogus: true\nlevel: warning\n",
        );
        let err = build(&unknown).unwrap_err().to_string();
        assert!(err.contains("invalid options"), "{err}");
    }

    #[test]
    fn evaluate_file_passes_at_limit_and_fires_above_with_default_and_custom_message() {
        let spec = spec_yaml(
            "id: t\nkind: file_max_lines\npaths: \"*.rs\"\nmax_lines: 2\nlevel: warning\n",
        );
        let rule = build(&spec).unwrap();
        let pf = rule.as_per_file().unwrap();
        let idx = index(&["a.rs"]);
        let c = ctx(Path::new("/fake"), &idx);

        assert!(
            pf.evaluate_file(&c, Path::new("a.rs"), b"1\n2\n")
                .unwrap()
                .is_empty()
        );
        let v = pf
            .evaluate_file(&c, Path::new("a.rs"), b"1\n2\n3\n")
            .unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("3 line(s)"), "{}", v[0].message);
        assert!(v[0].message.contains("at most 2"), "{}", v[0].message);

        let custom_spec = spec_yaml(
            "id: t\nkind: file_max_lines\npaths: \"*.rs\"\nmax_lines: 1\nmessage: \"too long\"\nlevel: error\n",
        );
        let custom_rule = build(&custom_spec).unwrap();
        let v = custom_rule
            .as_per_file()
            .unwrap()
            .evaluate_file(&c, Path::new("a.rs"), b"1\n2\n")
            .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].message, "too long");
    }
}
