//! `file_exists` — require that at least one file matching any of the given
//! globs exists in the repository.

use std::path::PathBuf;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation,
};
use serde::Deserialize;

use crate::fixers::FileCreateFixer;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    #[serde(default)]
    root_only: bool,
}

#[derive(Debug)]
pub struct FileExistsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    root_only: bool,
    /// When `true`, only consider walked entries that are also
    /// in git's index. Outside a git repo this becomes a silent
    /// no-op — no entries qualify, so the rule reports the
    /// "missing" violation as if no file existed.
    git_tracked_only: bool,
    fixer: Option<FileCreateFixer>,
}

impl FileExistsRule {
    fn describe_patterns(&self) -> String {
        self.patterns.join(", ")
    }
}

impl Rule for FileExistsRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn wants_git_tracked(&self) -> bool {
        self.git_tracked_only
    }

    fn requires_full_index(&self) -> bool {
        // Existence is an aggregate verdict over the whole tree —
        // "is at least one matching file present?". In `--changed`
        // mode, evaluate against the full index (so an unchanged
        // LICENSE still counts) but let the engine skip the rule
        // entirely when its scope doesn't intersect the diff.
        true
    }

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let found = ctx.index.files().any(|entry| {
            if self.root_only && entry.path.components().count() != 1 {
                return false;
            }
            if !self.scope.matches(&entry.path) {
                return false;
            }
            if self.git_tracked_only && !ctx.is_git_tracked(&entry.path) {
                return false;
            }
            true
        });
        if found {
            Ok(Vec::new())
        } else {
            let message = self.message.clone().unwrap_or_else(|| {
                let scope = if self.root_only {
                    " at the repo root"
                } else {
                    ""
                };
                let tracked = if self.git_tracked_only {
                    " (tracked in git)"
                } else {
                    ""
                };
                format!(
                    "expected a file matching [{}]{scope}{tracked}",
                    self.describe_patterns()
                )
            });
            Ok(vec![Violation::new(message)])
        }
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_exists requires a `paths` field",
        ));
    };
    let patterns = patterns_of(paths);
    let scope = Scope::from_paths_spec(paths)?;
    let opts: Options = spec
        .deserialize_options()
        .unwrap_or(Options { root_only: false });
    let fixer = match &spec.fix {
        Some(FixSpec::FileCreate { file_create: cfg }) => {
            let target = cfg
                .path
                .clone()
                .or_else(|| first_literal_path(&patterns))
                .ok_or_else(|| {
                    Error::rule_config(
                        &spec.id,
                        "fix.file_create needs a `path` — none of the rule's `paths:` \
                         entries is a literal filename",
                    )
                })?;
            Some(FileCreateFixer::new(
                target,
                cfg.content.clone(),
                cfg.create_parents,
            ))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_exists", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileExistsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope,
        patterns,
        root_only: opts.root_only,
        git_tracked_only: spec.git_tracked_only,
        fixer,
    }))
}

/// Best-effort: return the first entry in `patterns` that has no glob
/// metacharacters (so it's a usable file path). Returns `None` if every
/// pattern is a glob — in that case the caller must require an
/// explicit `fix.file_create.path`.
fn first_literal_path(patterns: &[String]) -> Option<PathBuf> {
    patterns
        .iter()
        .find(|p| !p.chars().any(|c| matches!(c, '*' | '?' | '[' | '{')))
        .map(PathBuf::from)
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}
