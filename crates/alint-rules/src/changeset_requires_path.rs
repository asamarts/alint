//! `changeset_requires_path` — the `<since>...HEAD` diff must ADD a
//! file matching `add_glob:`. The "did you add a changelog entry?"
//! gate (prettier `changelog_unreleased/`, cpython `Misc/NEWS.d/next/`,
//! pnpm `.changeset/*.md`).
//!
//! Diff-scoped: `since:` (the base ref) is required — the assertion
//! is about the *set of files a contribution adds*. An optional
//! `when_changed:` gates the requirement on some other glob having
//! changed (don't demand a changelog for a docs-only PR); with no
//! gate, any non-empty changeset triggers it. Builds on the same
//! `<since>...HEAD` three-dot (merge-base) diff as
//! `scope_filter.changed_since:` / `alint check --changed`.
//!
//! Graceful no-op outside a git repo, when `git` is unavailable, or
//! when nothing relevant changed. A `since:` that fails to resolve
//! hard-fails with a shallow-clone hint (the user asked for a diff;
//! a silently-empty range would mask the misconfiguration).
//!
//! Check-only — alint can't author the missing changelog entry.

use std::slice;

use alint_core::git::{
    CommitRangeError, collect_changed_paths_checked, collect_changed_paths_filtered,
};
use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Glob; the diff must ADD at least one path matching it.
    add_glob: String,
    /// Optional gate: only require the add when some path matching this glob
    /// changed (any status). Omit to require it on any non-empty changeset.
    #[serde(default)]
    when_changed: Option<String>,
    /// Base ref for the `<since>...HEAD` diff. Use the canonical `{{env.X}}`
    /// interpolation, e.g. `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`.
    since: String,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct ChangesetRequiresPathRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    add_glob: String,
    add_scope: Scope,
    when_changed: Option<Scope>,
    since: String,
}

impl Rule for ChangesetRequiresPathRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let all_changed = match collect_changed_paths_checked(ctx.root, &self.since) {
            Ok(Some(set)) => set,
            Ok(None) => return Ok(Vec::new()), // not a git repo: silent
            Err(CommitRangeError::BadRange { stderr }) => return Err(self.bad_range(&stderr)),
        };

        // Does the requirement apply? `when_changed` gates it; with
        // no gate, any non-empty changeset triggers it.
        let applies = match &self.when_changed {
            None => !all_changed.is_empty(),
            Some(scope) => all_changed.iter().any(|p| scope.matches(p, ctx.index)),
        };
        if !applies {
            return Ok(Vec::new());
        }

        let added = match collect_changed_paths_filtered(ctx.root, &self.since, "A") {
            Ok(Some(set)) => set,
            Ok(None) => return Ok(Vec::new()),
            Err(CommitRangeError::BadRange { stderr }) => return Err(self.bad_range(&stderr)),
        };
        if added.iter().any(|p| self.add_scope.matches(p, ctx.index)) {
            return Ok(Vec::new());
        }

        let msg = self.message_override.clone().unwrap_or_else(|| {
            let gate = if self.when_changed.is_some() {
                " (its `when_changed` glob changed)"
            } else {
                ""
            };
            format!(
                "the changeset `{}...HEAD`{gate} adds no file matching `{}`",
                self.since, self.add_glob,
            )
        });
        // No path (a repo-level changeset finding) → a fixed key so the
        // fingerprint doesn't fall through to the volatile message.
        Ok(vec![
            Violation::new(msg).with_baseline_key("changeset-requires-path"),
        ])
    }
}

impl ChangesetRequiresPathRule {
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
            "changeset_requires_path has no fix op",
        ));
    }
    if opts.add_glob.trim().is_empty() {
        return Err(Error::rule_config(&spec.id, "`add_glob` must not be empty"));
    }
    if opts.since.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "`since` must not be empty - `changeset_requires_path` is diff-scoped and needs a \
             base ref (typically `since: \"{{env.ALINT_BASE_SHA | default('origin/main')}}\"`)",
        ));
    }
    let add_scope = Scope::from_patterns(slice::from_ref(&opts.add_glob))
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `add_glob`: {e}")))?;
    let when_changed =
        match &opts.when_changed {
            None => None,
            Some(g) if g.trim().is_empty() => {
                return Err(Error::rule_config(
                    &spec.id,
                    "`when_changed` must not be empty (omit it to require the add on any change)",
                ));
            }
            Some(g) => Some(Scope::from_patterns(slice::from_ref(g)).map_err(|e| {
                Error::rule_config(&spec.id, format!("invalid `when_changed`: {e}"))
            })?),
        };

    Ok(Box::new(ChangesetRequiresPathRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        add_glob: opts.add_glob,
        add_scope,
        when_changed,
        since: opts.since,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full = String::from(
            "id = \"needs-changelog\"\nkind = \"changeset_requires_path\"\nlevel = \"error\"\n",
        );
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_accepts_minimal() {
        assert!(
            build(&spec(
                "add_glob = \".changeset/*.md\"\nsince = \"origin/main\"\n"
            ))
            .is_ok()
        );
    }

    #[test]
    fn build_accepts_when_changed_gate() {
        assert!(
            build(&spec(
                "add_glob = \".changeset/*.md\"\nwhen_changed = \"src/**\"\nsince = \"origin/main\"\n"
            ))
            .is_ok()
        );
    }

    #[test]
    fn build_requires_since() {
        let err = build(&spec("add_glob = \".changeset/*.md\"\n")).unwrap_err();
        // serde reports the missing required field.
        assert!(err.to_string().contains("since"), "{err}");
    }

    #[test]
    fn build_rejects_empty_add_glob() {
        let err = build(&spec("add_glob = \"\"\nsince = \"origin/main\"\n")).unwrap_err();
        assert!(err.to_string().contains("add_glob"), "{err}");
    }

    #[test]
    fn build_rejects_fix() {
        assert!(
            build(&spec(
                "add_glob = \"x\"\nsince = \"main\"\nfix = { file_create = { content = \"x\" } }\n"
            ))
            .is_err()
        );
    }
}
