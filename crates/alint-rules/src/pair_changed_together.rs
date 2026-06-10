//! `pair_changed_together` — if any `if_changed:` path is in the
//! `<since>...HEAD` diff, at least one `then_changed:` path must be too.
//! The co-change gate (rust `rustdoc-json-types` `FORMAT_VERSION` must bump
//! when the format struct changes; "`version.txt` and the lockfile change
//! together"). The `changeset_requires_path` sibling.
//!
//! Diff-scoped: `since:` (the base ref) is required — the assertion is
//! about *which files a contribution touched together*. Built on the
//! same `<since>...HEAD` three-dot (merge-base) diff as
//! `scope_filter.changed_since:` / `alint check --changed`.
//!
//! Direction matters: the trigger is `if_changed`, the obligation is
//! `then_changed`. It fires only when `if_changed` changed *without*
//! `then_changed` — a `then_changed`-only change never triggers it (use
//! a second rule with the globs swapped for a bidirectional pact).
//!
//! Graceful no-op outside a git repo, when `git` is unavailable, or
//! when `if_changed` didn't change. A `since:` that fails to resolve
//! hard-fails with a shallow-clone hint (the user asked for a diff; a
//! silently-empty range would mask the misconfiguration).
//!
//! Check-only — alint can't author the missing co-change.

use std::slice;

use alint_core::git::{CommitRangeError, collect_changed_paths_checked};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Glob; the trigger. When the diff changes a path matching this, a
    /// `then_changed` co-change is required.
    if_changed: String,
    /// Glob; the obligation. At least one changed path must match it whenever
    /// `if_changed` fired.
    then_changed: String,
    /// Base ref for the `<since>...HEAD` diff. Use the canonical `{{env.X}}`
    /// interpolation, e.g. `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`.
    since: String,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct PairChangedTogetherRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    if_changed: String,
    if_scope: Scope,
    then_changed: String,
    then_scope: Scope,
    since: String,
}

impl Rule for PairChangedTogetherRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let changed = match collect_changed_paths_checked(ctx.root, &self.since) {
            Ok(Some(set)) => set,
            Ok(None) => return Ok(Vec::new()), // not a git repo: silent
            Err(CommitRangeError::BadRange { stderr }) => return Err(self.bad_range(&stderr)),
        };

        // The trigger: did anything matching `if_changed` change?
        if !changed.iter().any(|p| self.if_scope.matches(p, ctx.index)) {
            return Ok(Vec::new());
        }
        // The obligation: at least one `then_changed` path must have
        // changed in the same range.
        if changed
            .iter()
            .any(|p| self.then_scope.matches(p, ctx.index))
        {
            return Ok(Vec::new());
        }

        let msg = self.message_override.clone().unwrap_or_else(|| {
            format!(
                "the changeset `{}...HEAD` changes a path matching `{}` but no path matching \
                 `{}` changed with it",
                self.since, self.if_changed, self.then_changed,
            )
        });
        Ok(vec![Violation::new(msg)])
    }
}

impl PairChangedTogetherRule {
    fn bad_range(&self, stderr: &str) -> Error {
        crate::commit_range::bad_diff_range(&self.id, &self.since, stderr)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "pair_changed_together has no fix op",
        ));
    }
    if opts.if_changed.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "`if_changed` must not be empty",
        ));
    }
    if opts.then_changed.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "`then_changed` must not be empty",
        ));
    }
    if opts.since.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "`since` must not be empty — `pair_changed_together` is diff-scoped and needs a \
             base ref (typically `since: \"{{env.ALINT_BASE_SHA | default('origin/main')}}\"`)",
        ));
    }
    let if_scope = Scope::from_patterns(slice::from_ref(&opts.if_changed))
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `if_changed`: {e}")))?;
    let then_scope = Scope::from_patterns(slice::from_ref(&opts.then_changed))
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `then_changed`: {e}")))?;

    Ok(Box::new(PairChangedTogetherRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        if_changed: opts.if_changed,
        if_scope,
        then_changed: opts.then_changed,
        then_scope,
        since: opts.since,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full = String::from(
            "id = \"co-change\"\nkind = \"pair_changed_together\"\nlevel = \"error\"\n",
        );
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_accepts_minimal() {
        assert!(
            build(&spec(
                "if_changed = \"src/format.rs\"\nthen_changed = \"FORMAT_VERSION\"\nsince = \"origin/main\"\n"
            ))
            .is_ok()
        );
    }

    #[test]
    fn build_requires_since() {
        let err = build(&spec("if_changed = \"a\"\nthen_changed = \"b\"\n")).unwrap_err();
        assert!(err.to_string().contains("since"), "{err}");
    }

    #[test]
    fn build_rejects_empty_if_changed() {
        let err = build(&spec(
            "if_changed = \"\"\nthen_changed = \"b\"\nsince = \"main\"\n",
        ))
        .unwrap_err();
        assert!(err.to_string().contains("if_changed"), "{err}");
    }

    #[test]
    fn build_rejects_empty_then_changed() {
        let err = build(&spec(
            "if_changed = \"a\"\nthen_changed = \"\"\nsince = \"main\"\n",
        ))
        .unwrap_err();
        assert!(err.to_string().contains("then_changed"), "{err}");
    }

    #[test]
    fn build_rejects_fix() {
        assert!(
            build(&spec(
                "if_changed = \"a\"\nthen_changed = \"b\"\nsince = \"main\"\nfix = { file_create = { content = \"x\" } }\n"
            ))
            .is_err()
        );
    }
}
