//! `file_exists` — require that at least one file matching any of the given
//! globs exists in the repository.

use std::path::{Path, PathBuf};

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
    /// `Some(paths)` when every entry in `patterns` is a literal
    /// path (no glob metacharacters, no `!` excludes) and the
    /// rule does not opt into `git_tracked_only`. The fast path
    /// uses these to do O(1) `FileIndex::contains_file` lookups
    /// instead of iterating every entry through
    /// `Scope::matches`. At 1M files in a 5,000-package
    /// monorepo, `for_each_dir` rules spawn one nested
    /// `file_exists` per directory; without this short-circuit
    /// each one is an O(N) scan and the fan-out becomes
    /// O(D × N). With it, they collapse to O(D) lookups.
    literal_paths: Option<Vec<PathBuf>>,
    root_only: bool,
    /// When `true`, only consider walked entries that are also
    /// in git's index. Outside a git repo this becomes a silent
    /// no-op — no entries qualify, so the rule reports the
    /// "missing" violation as if no file existed.
    git_tracked_only: bool,
    fixer: Option<FileCreateFixer>,
}

/// True when `pattern` is a plain literal path string — no glob
/// metacharacters, no `!` exclude prefix. Such patterns can be
/// answered by an O(1) hash-set lookup against
/// [`alint_core::FileIndex::contains_file`] instead of a O(N)
/// scope-match scan.
fn is_literal_path(pattern: &str) -> bool {
    !pattern.starts_with('!')
        && !pattern
            .chars()
            .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
}

/// True iff `paths` is a flat list (single string or `Many`)
/// with no excludes — `IncludeExclude` form is excluded since
/// the fast path can't honour excludes by hash lookup alone.
fn paths_spec_has_no_excludes(spec: &PathsSpec) -> bool {
    match spec {
        PathsSpec::Single(_) | PathsSpec::Many(_) => true,
        PathsSpec::IncludeExclude { exclude, .. } => exclude.is_empty(),
    }
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
        let found = if let Some(literals) = self.literal_paths.as_ref() {
            // Fast path: each pattern is a literal relative
            // path. Hash-lookup against the index's lazily-
            // built path set is O(1) per pattern; for
            // `for_each_dir`-spawned rules at 1M scale this is
            // the difference between O(D × N) and O(D).
            literals.iter().any(|p| {
                if self.root_only && literal_is_nested(p) {
                    return false;
                }
                ctx.index.contains_file(p)
            })
        } else {
            // Slow path: glob patterns and/or `git_tracked_only`
            // require iterating every entry. Same shape as the
            // pre-v0.10 implementation — preserved verbatim so
            // glob-using rules keep their existing semantics.
            ctx.index.files().any(|entry| {
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
            })
        };
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
    alint_core::reject_scope_filter_on_cross_file(spec, "file_exists")?;
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
    // The fast path needs every pattern to be a plain relative
    // path (no glob metacharacters, no `!` exclude) AND the
    // rule must not opt into `git_tracked_only` (which requires
    // a per-entry callback). When all preconditions hold,
    // `literal_paths` carries the parsed `PathBuf`s ready for
    // `FileIndex::contains_file` lookup at evaluate time.
    let literal_paths = if !spec.git_tracked_only
        && paths_spec_has_no_excludes(paths)
        && patterns.iter().all(|p| is_literal_path(p))
    {
        Some(patterns.iter().map(PathBuf::from).collect())
    } else {
        None
    };
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
            let source = alint_core::resolve_content_source(
                &spec.id,
                "file_create",
                &cfg.content,
                &cfg.content_from,
            )?;
            Some(FileCreateFixer::new(target, source, cfg.create_parents))
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
        literal_paths,
        root_only: opts.root_only,
        git_tracked_only: spec.git_tracked_only,
        fixer,
    }))
}

/// True when a literal `paths:` pattern names something nested
/// (more than one path component). Mirrors the slow-path
/// `entry.path.components().count() != 1` check used to honour
/// `root_only` against entries during a scope-match scan.
fn literal_is_nested(p: &Path) -> bool {
    p.components().count() != 1
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_accepts_root_only_option() {
        // `root_only: true` is the supported option; building
        // it should succeed and produce a configured rule.
        // (Unknown options are tolerated by file_exists' build
        // path via `.unwrap_or(default)`; the JSON Schema and
        // DSL loader catch typos at config-load time before
        // we get here, which is the right layer for that
        // check.)
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"LICENSE\"\n\
             level: error\n\
             root_only: true\n",
        );
        assert!(build(&spec).is_ok());
    }

    #[test]
    fn build_rejects_incompatible_fix_op() {
        // file_exists supports `file_create` only; `file_remove`
        // (or any other op) must surface a clear config error so
        // a typo doesn't silently disable the fix path.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"LICENSE\"\n\
             level: error\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("file_remove"), "unexpected: {err}");
    }

    #[test]
    fn build_file_create_needs_explicit_path_for_glob_only_paths() {
        // When every entry in `paths:` is a glob, the fixer
        // can't pick a literal target; the user must supply
        // `fix.file_create.path` explicitly.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"docs/**/*.md\"\n\
             level: error\n\
             fix:\n  \
               file_create:\n    \
                 content: \"# title\\n\"\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("path"), "unexpected: {err}");
    }

    #[test]
    fn evaluate_passes_when_matching_file_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"README.md\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["README.md", "Cargo.toml"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected violations: {v:?}");
    }

    #[test]
    fn evaluate_fires_when_no_matching_file_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"LICENSE\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["README.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "expected one violation; got: {v:?}");
    }

    #[test]
    fn evaluate_root_only_excludes_nested_matches() {
        // `root_only: true` only counts entries whose path has
        // no parent component — `LICENSE` qualifies,
        // `pkg/LICENSE` does not.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_exists\n\
             paths: \"LICENSE\"\n\
             level: error\n\
             root_only: true\n",
        );
        let rule = build(&spec).unwrap();
        let idx_only_nested = index(&["pkg/LICENSE"]);
        let v = rule
            .evaluate(&ctx(Path::new("/fake"), &idx_only_nested))
            .unwrap();
        assert_eq!(v.len(), 1, "nested match shouldn't satisfy root_only");
    }

    #[test]
    fn first_literal_path_picks_first_non_glob() {
        let patterns = vec!["docs/**/*.md".into(), "LICENSE".into(), "README.md".into()];
        assert_eq!(
            first_literal_path(&patterns).as_deref(),
            Some(Path::new("LICENSE")),
        );
    }

    #[test]
    fn first_literal_path_returns_none_when_all_glob() {
        let patterns = vec!["docs/**/*.md".into(), "src/[a-z]*.rs".into()];
        assert!(first_literal_path(&patterns).is_none());
    }

    #[test]
    fn patterns_of_handles_every_paths_spec_shape() {
        assert_eq!(patterns_of(&PathsSpec::Single("a".into())), vec!["a"]);
        assert_eq!(
            patterns_of(&PathsSpec::Many(vec!["a".into(), "b".into()])),
            vec!["a", "b"],
        );
        assert_eq!(
            patterns_of(&PathsSpec::IncludeExclude {
                include: vec!["a".into()],
                exclude: vec!["b".into()],
            }),
            vec!["a"],
        );
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // file_exists is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: file_exists
paths: "LICENSE"
level: error
scope_filter:
  has_ancestor: Cargo.toml
"#;
        let spec = spec_yaml(yaml);
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("scope_filter is supported on per-file rules only"),
            "expected per-file-only message, got: {err}",
        );
        assert!(
            err.contains("file_exists"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }
}
