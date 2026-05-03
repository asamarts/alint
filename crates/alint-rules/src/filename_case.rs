//! `filename_case` — every file in scope must have a basename whose stem
//! matches a case convention.
//!
//! The check runs on the *stem* (filename with the final extension removed),
//! matching ls-lint precedent. For files with compound extensions like
//! `foo.spec.ts`, the stem is `foo.spec`, which will fail most case checks —
//! use `filename_regex` for finer control in those situations.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::case::CaseConvention;
use crate::fixers::FileRenameFixer;

#[derive(Debug, Deserialize)]
struct Options {
    case: CaseConvention,
}

#[derive(Debug)]
pub struct FilenameCaseRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    case: CaseConvention,
    fixer: Option<FileRenameFixer>,
}

impl Rule for FilenameCaseRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let Some(stem) = entry.path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !self.case.check(stem) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "filename stem {:?} is not {}",
                        stem,
                        self.case.display_name()
                    )
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "filename_case requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRename { .. }) => Some(FileRenameFixer::new(opts.case)),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with filename_case",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(FilenameCaseRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        case: opts.case,
        fixer,
    }))
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
             kind: filename_case\n\
             case: snake_case\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_missing_case_option() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"src/**/*.rs\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err(), "missing `case:` should error");
    }

    #[test]
    fn build_rejects_incompatible_fix_op() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"src/**/*.rs\"\n\
             case: snake_case\n\
             level: error\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("file_remove"), "unexpected: {err}");
    }

    #[test]
    fn evaluate_passes_on_canonical_snake_case() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"src/**/*.rs\"\n\
             case: snake_case\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["src/main.rs", "src/lib.rs", "src/sub/mod.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_on_pascal_case_when_snake_required() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"src/**/*.rs\"\n\
             case: snake_case\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["src/MainModule.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn evaluate_skips_files_outside_scope() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"src/**/*.rs\"\n\
             case: snake_case\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        // PascalCase, but outside scope.
        let idx = index(&["docs/MainDoc.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "out-of-scope shouldn't fire: {v:?}");
    }

    #[test]
    fn pascal_case_matches_canonical_components() {
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"components/**/*.tsx\"\n\
             case: PascalCase\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&[
            "components/Button.tsx",
            "components/UserCard.tsx",
            "components/bad_name.tsx",
        ]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only `bad_name` should fire");
    }

    #[test]
    fn scope_filter_narrows() {
        // Two PascalCase files violating snake_case; only the
        // one inside a directory with `marker.lock` as ancestor
        // should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: filename_case\n\
             paths: \"**/*.rs\"\n\
             case: snake_case\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["pkg/marker.lock", "pkg/BadName.rs", "other/AlsoBad.rs"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/BadName.rs")));
    }
}
