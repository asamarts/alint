//! `for_each_match` — for each line matching `select`, the line must
//! satisfy a conjunction of `require:` predicates (`matches` / `forbid`
//! / capture `equal`). The in-file line quantifier (axis ④) and the
//! missing dual of `ordered_block`'s `select:` — where `ordered_block`
//! *orders* selected lines, this asserts predicates over each one.
//! Per-file rule (the `PerFileRule` fast path), not cross-file. Design
//! + reproduce-first motivation: `docs/design/v0.12/for_each_match.md`.
//!
//! ```yaml
//! - id: changelog-entries-well-formed
//!   kind: for_each_match
//!   paths: ["CHANGELOG.md"]
//!   select: '^[*-] .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
//!   require:
//!     matches: ['\)\.$']          # the line must match ALL of these
//!     forbid:  ['\[Fix #\d+\]']   # ...and NONE of these
//!     equal:   [disp, url]        # ...and these named captures must be equal
//!   level: warning
//! ```

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use regex::{Captures, Regex};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    select: String,
    require: RequireSpec,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ForEachMatchRequireSpec")]
#[schemars(extend(
    "anyOf" = [
        {"required": ["matches"]},
        {"required": ["forbid"]},
        {"required": ["equal"]}
    ]
))]
struct RequireSpec {
    /// The selected line must match ALL of these regexes.
    #[serde(default)]
    matches: Vec<String>,
    /// The selected line must match NONE of these regexes.
    #[serde(default)]
    forbid: Vec<String>,
    /// These named `select` captures must all be equal (the only way to assert
    /// intra-line capture equality, since RE2 has no backreference).
    #[serde(default)]
    #[schemars(length(min = 2))]
    equal: Vec<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct ForEachMatchRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    select: Regex,
    /// `(source, compiled)` so messages can name the offending pattern.
    matches: Vec<(String, Regex)>,
    forbid: Vec<(String, Regex)>,
    equal: Vec<String>,
}

impl Rule for ForEachMatchRule {
    alint_core::rule_common_impl!();

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for ForEachMatchRule {
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
            // A line-oriented predicate is degenerate on non-UTF-8.
            return Ok(Vec::new());
        };
        let mut violations = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line_no = i + 1;
            if !self.select.is_match(line) {
                continue; // not a selected element
            }
            // `matches`: every pattern must match the element line.
            for (src, re) in &self.matches {
                if !re.is_match(line) {
                    violations.push(self.violation(
                        path,
                        line_no,
                        &format!("matches `select` but not the required /{src}/"),
                    ));
                }
            }
            // `forbid`: no pattern may match the element line.
            for (src, re) in &self.forbid {
                if re.is_match(line) {
                    violations.push(self.violation(
                        path,
                        line_no,
                        &format!("matches the forbidden /{src}/"),
                    ));
                }
            }
            // `equal`: EVERY `select` match on the line must have
            // agreeing captures — a line can carry more than one
            // (e.g. two PR links), and each is checked.
            if !self.equal.is_empty() {
                for caps in self.select.captures_iter(line) {
                    if let Some(desc) = self.equal_mismatch(&caps) {
                        violations.push(self.violation(path, line_no, &desc));
                    }
                }
            }
        }
        Ok(violations)
    }
}

impl ForEachMatchRule {
    /// `None` when the `equal` captures all agree (or there is no
    /// `equal` check); otherwise a description of the mismatch.
    /// Captures absent on a match compare equal to each other
    /// (all-absent → vacuous pass); a mix of present and absent values
    /// is a mismatch.
    fn equal_mismatch(&self, caps: &Captures<'_>) -> Option<String> {
        if self.equal.is_empty() {
            return None;
        }
        let vals: Vec<Option<&str>> = self
            .equal
            .iter()
            .map(|n| caps.name(n).map(|m| m.as_str()))
            .collect();
        if vals.iter().all(|v| *v == vals[0]) {
            return None;
        }
        let parts: Vec<String> = self
            .equal
            .iter()
            .zip(&vals)
            .map(|(n, v)| match v {
                Some(s) => format!("{n}={s:?}"),
                None => format!("{n}=<unmatched>"),
            })
            .collect();
        Some(format!(
            "captures must be equal but differ: {}",
            parts.join(", ")
        ))
    }

    fn violation(&self, path: &Path, line: usize, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("for_each_match (line {line}): {desc}"));
        Violation::new(msg)
            .with_path(Arc::<Path>::from(path))
            .with_location(line, 1)
    }
}

/// Compile a `require.<field>` pattern list, keeping each source for
/// messages.
fn compile_patterns(id: &str, field: &str, pats: Vec<String>) -> Result<Vec<(String, Regex)>> {
    pats.into_iter()
        .map(|p| {
            Regex::new(&p).map(|re| (p.clone(), re)).map_err(|e| {
                Error::rule_config(id, format!("invalid `require.{field}` regex `{p}`: {e}"))
            })
        })
        .collect()
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("select"),
            selector_type: alint_core::SelectorType::Regex,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.matches"),
            selector_type: alint_core::SelectorType::Regex,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("require.forbid"),
            selector_type: alint_core::SelectorType::Regex,
            rerootable: false,
        },
    ]),
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
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "for_each_match requires a `paths` field (the files to scan line by line)",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let cfg = |msg: String| Error::rule_config(&spec.id, msg);

    if opts.select.trim().is_empty() {
        return Err(cfg("`select` regex must not be empty".into()));
    }
    let select =
        Regex::new(&opts.select).map_err(|e| cfg(format!("invalid `select` regex: {e}")))?;

    let matches = compile_patterns(&spec.id, "matches", opts.require.matches)?;
    let forbid = compile_patterns(&spec.id, "forbid", opts.require.forbid)?;
    let equal = opts.require.equal;

    if matches.is_empty() && forbid.is_empty() && equal.is_empty() {
        return Err(cfg(
            "`require` must set at least one of `matches`, `forbid`, `equal`".into(),
        ));
    }
    if !equal.is_empty() {
        if equal.len() < 2 {
            return Err(cfg(
                "`require.equal` needs at least two capture names to compare".into(),
            ));
        }
        let names: HashSet<&str> = select.capture_names().flatten().collect();
        for n in &equal {
            if !names.contains(n.as_str()) {
                return Err(cfg(format!(
                    "`require.equal` requires named captures; `{n}` is not a named \
                 group in `select` (use `(?P<{n}>...)`)"
                )));
            }
        }
    }

    Ok(Box::new(ForEachMatchRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        select,
        matches,
        forbid,
        equal,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(select: &str, matches: &[&str], forbid: &[&str], equal: &[&str]) -> ForEachMatchRule {
        ForEachMatchRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            select: Regex::new(select).unwrap(),
            matches: matches
                .iter()
                .map(|p| ((*p).to_string(), Regex::new(p).unwrap()))
                .collect(),
            forbid: forbid
                .iter()
                .map(|p| ((*p).to_string(), Regex::new(p).unwrap()))
                .collect(),
            equal: equal.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn eval(r: &ForEachMatchRule, text: &str) -> Vec<Violation> {
        let index = alint_core::FileIndex::from_entries(Vec::new());
        let ctx = Context::new(Path::new("/"), &index);
        r.evaluate_file(&ctx, Path::new("CHANGELOG.md"), text.as_bytes())
            .unwrap()
    }

    #[test]
    fn matches_requires_every_selected_line_to_conform() {
        // Every `* ` entry must end with a linked PR ref + period.
        let r = rule(r"^\* ", &[r"\(\[#\d+\]\([^)]+\)\)\.$"], &[], &[]);
        let good = "* Add a feature ([#12](https://x/pull/12)).\n";
        assert!(eval(&r, good).is_empty());
        // One good, one malformed -> exactly one violation (the gap
        // file_content_matches existence semantics cannot express).
        let mixed = "* Add a feature ([#12](https://x/pull/12)).\n* broken entry\n";
        let v = eval(&r, mixed);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].line, Some(2));
        assert!(v[0].message.contains("required"));
    }

    #[test]
    fn forbid_flags_a_banned_form() {
        let r = rule(r"^\* ", &[], &[r"\[Fix #\d+\]"], &[]);
        let bad = "* Closes [Fix #7] the thing\n* Normal entry\n";
        let v = eval(&r, bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].line, Some(1));
        assert!(v[0].message.contains("forbidden"));
    }

    #[test]
    fn equal_flags_intra_line_capture_disagreement() {
        // The mypy bad-pr-link shape: display number must equal the
        // URL number (RE2 cannot express this with a backreference).
        let r = rule(
            r"\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)",
            &[],
            &[],
            &["disp", "url"],
        );
        let ok = "- Fixed ([#1234](https://x/pull/1234))\n";
        assert!(eval(&r, ok).is_empty());
        let bad = "- Typo ([#55](https://x/pull/99))\n";
        let v = eval(&r, bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("differ"));
        assert!(v[0].message.contains("55") && v[0].message.contains("99"));
    }

    #[test]
    fn equal_checks_every_match_on_a_line() {
        // A line can carry more than one `select` match; each is
        // checked, not just the first (the v0.12 C1 correctness fix).
        let r = rule(
            r"\[#(?P<disp>\d+)\]\(pull/(?P<url>\d+)\)",
            &[],
            &[],
            &["disp", "url"],
        );
        // First link agrees, second disagrees -> exactly one violation
        // (on the second occurrence), not a silent pass.
        let line = "see [#1](pull/1) and [#2](pull/9)\n";
        let v = eval(&r, line);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("differ"));
        assert!(v[0].message.contains('2') && v[0].message.contains('9'));
        // Both agree -> silent.
        assert!(eval(&r, "[#3](pull/3) and [#4](pull/4)\n").is_empty());
    }

    #[test]
    fn equal_all_absent_passes_mixed_fires() {
        // Optional capture groups: a match where NEITHER participates
        // is a vacuous pass; a mix of present + absent is a mismatch.
        let r = rule(r"^item (?P<a>x)?(?P<b>y)?", &[], &[], &["a", "b"]);
        assert!(
            eval(&r, "item zzz\n").is_empty(),
            "all-absent -> vacuous pass"
        );
        // `a` present (x), `b` absent -> a mix -> mismatch.
        let v = eval(&r, "item x\n");
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("unmatched"), "{}", v[0].message);
    }

    #[test]
    fn multiline_select_no_ops_under_per_line_iteration() {
        // A `(?s)`-spanning select cannot match a single line, so the
        // rule silently selects nothing (the documented line-oriented
        // design — a user porting a whole-file regex gets a no-op).
        let r = rule(r"(?s)BEGIN.*END", &[r"WONTMATCH"], &[], &[]);
        assert!(eval(&r, "BEGIN\nmiddle\nEND\n").is_empty());
    }

    #[test]
    fn empty_and_no_selected_lines_are_silent() {
        let r = rule(r"^ENTRY ", &[r"WONTMATCH"], &[], &[]);
        assert!(eval(&r, "").is_empty(), "empty file");
        assert!(
            eval(&r, "alpha\nbravo\n").is_empty(),
            "no line matches select"
        );
    }

    #[test]
    fn non_selected_lines_are_ignored() {
        let r = rule(r"^\* ", &[r"\.$"], &[], &[]);
        // Only the `* ` line is an element; the prose line is ignored
        // even though it doesn't end in a period.
        assert!(eval(&r, "Some heading\n* An entry.\n").is_empty());
    }

    #[test]
    fn build_requires_at_least_one_predicate() {
        use crate::test_support::spec_yaml;
        let empty = "id: t\nkind: for_each_match\npaths: [\"CHANGELOG.md\"]\n\
                     select: '^\\* '\nrequire: {}\nlevel: error\n";
        assert!(
            build(&spec_yaml(empty)).is_err(),
            "empty require is rejected"
        );
        let ok = "id: t\nkind: for_each_match\npaths: [\"CHANGELOG.md\"]\n\
                  select: '^\\* '\nrequire:\n  matches: ['\\.$']\nlevel: error\n";
        assert!(build(&spec_yaml(ok)).is_ok(), "one predicate builds");
    }

    #[test]
    fn build_rejects_equal_naming_an_undefined_capture() {
        use crate::test_support::spec_yaml;
        // `url` is not a named group in `select` -> rejected.
        let bad = "id: t\nkind: for_each_match\npaths: [\"CHANGELOG.md\"]\n\
                   select: '\\[#(?P<disp>\\d+)\\]'\nrequire:\n  equal: [disp, url]\nlevel: error\n";
        assert!(build(&spec_yaml(bad)).is_err());
    }

    #[test]
    fn build_rejects_invalid_select_regex() {
        use crate::test_support::spec_yaml;
        let bad = "id: t\nkind: for_each_match\npaths: [\"CHANGELOG.md\"]\n\
                   select: '(unclosed'\nrequire:\n  matches: ['x']\nlevel: error\n";
        let err = build(&spec_yaml(bad)).unwrap_err();
        assert!(err.to_string().contains("select"), "{err}");
    }

    #[test]
    fn build_rejects_equal_with_fewer_than_two_names() {
        use crate::test_support::spec_yaml;
        // `equal` compares captures pairwise; a single name has
        // nothing to compare against -> a config error, caught before
        // the named-capture validation.
        let bad = "id: t\nkind: for_each_match\npaths: [\"CHANGELOG.md\"]\n\
                   select: '(?P<a>\\d+)'\nrequire:\n  equal: [a]\nlevel: error\n";
        let err = build(&spec_yaml(bad)).unwrap_err();
        assert!(err.to_string().contains("at least two"), "{err}");
    }
}
