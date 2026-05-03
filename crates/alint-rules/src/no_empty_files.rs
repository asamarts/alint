//! `no_empty_files` — flag zero-byte files in scope.
//!
//! Empty files usually indicate placeholders forgotten in a
//! branch or generator output that lost its content. Fixable
//! via `file_remove`, which deletes the empty file.

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, ScopeFilter, Violation,
};

use crate::fixers::FileRemoveFixer;

#[derive(Debug)]
pub struct NoEmptyFilesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    scope_filter: Option<ScopeFilter>,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for NoEmptyFilesRule {
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
            if let Some(filter) = &self.scope_filter
                && !filter.matches(&entry.path, ctx.index)
            {
                continue;
            }
            if entry.size == 0 {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| "file is empty".to_string());
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_empty_files requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with no_empty_files",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoEmptyFilesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        scope_filter: spec.parse_scope_filter()?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml};
    use alint_core::{FileEntry, FileIndex};
    use std::path::Path;

    fn idx(entries: &[(&str, u64)]) -> FileIndex {
        FileIndex::from_entries(
            entries
                .iter()
                .map(|(p, sz)| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: *sz,
                })
                .collect(),
        )
    }

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_accepts_file_remove_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             paths: \"**/*\"\n\
             level: warning\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.fixer().is_some());
    }

    #[test]
    fn build_rejects_incompatible_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             paths: \"**/*\"\n\
             level: warning\n\
             fix:\n  \
               file_create:\n    \
                 content: \"x\"\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_fires_on_zero_byte_files() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let i = idx(&[("a.txt", 0), ("b.txt", 100), ("c.txt", 0)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert_eq!(v.len(), 2, "two empty files should fire");
    }

    #[test]
    fn evaluate_passes_on_non_empty_files() {
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let i = idx(&[("a.txt", 1), ("b.txt", 1024)]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn scope_filter_narrows() {
        // Two empty files; only the one inside a directory with
        // `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: no_empty_files\n\
             paths: \"**/*.txt\"\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let i = idx(&[
            ("pkg/marker.lock", 1),
            ("pkg/empty.txt", 0),
            ("other/empty.txt", 0),
        ]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &i)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/empty.txt")));
    }
}
