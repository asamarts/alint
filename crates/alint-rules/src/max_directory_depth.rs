//! `max_directory_depth` — cap the depth of any path in scope.
//!
//! Depth is the number of `/`-separated components in the path.
//! `README.md` is depth 1; `src/lib.rs` is depth 2; `a/b/c/d.rs`
//! is depth 4. Flags one violation per path that exceeds the cap.
//!
//! Check-only: moving files around to flatten the tree isn't a
//! decision alint can make automatically.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    max_depth: usize,
}

#[derive(Debug)]
pub struct MaxDirectoryDepthRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    max_depth: usize,
}

impl Rule for MaxDirectoryDepthRule {
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
            let depth = entry.path.components().count();
            if depth > self.max_depth {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "{} is at depth {depth}; max is {}",
                        entry.path.display(),
                        self.max_depth
                    )
                });
                violations.push(Violation::new(msg).with_path(&entry.path));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "max_directory_depth requires a `paths` field (often `\"**\"`)",
        )
    })?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.max_depth == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "max_directory_depth `max_depth` must be > 0",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "max_directory_depth has no fix op — moving files is a human decision",
        ));
    }
    Ok(Box::new(MaxDirectoryDepthRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        max_depth: opts.max_depth,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    fn depth_of(s: &str) -> usize {
        PathBuf::from(s).components().count()
    }

    #[test]
    fn depth_counts_components() {
        assert_eq!(depth_of("README.md"), 1);
        assert_eq!(depth_of("src/lib.rs"), 2);
        assert_eq!(depth_of("a/b/c/d.rs"), 4);
    }
}
