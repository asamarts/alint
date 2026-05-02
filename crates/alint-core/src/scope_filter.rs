//! `ScopeFilter` — per-file rule gate that scopes rule
//! application to files whose ancestor directories contain a
//! named manifest. The "closest-ancestor manifest" pattern, in
//! practical terms.
//!
//! Composes with the existing per-file `paths:` glob and the
//! tree-level `when:` gate as an AND. A file matches a rule
//! iff every gate it has accepts the file.
//!
//! ## Why
//!
//! Bundled ecosystem rulesets (`rust@v1`, `node@v1`, …) need
//! to scope per-file rules to only files inside a package of
//! the matching ecosystem. A `**/*.rs` glob alone is too
//! broad: in a polyglot monorepo, `services/web/scripts/
//! migrate.rs` shouldn't be governed by Rust hygiene rules
//! just because it has the `.rs` extension. With
//! `scope_filter: { has_ancestor: Cargo.toml }`, the rule
//! only fires on files that have a `Cargo.toml` somewhere in
//! their ancestor chain — i.e., files inside an actual Rust
//! package.
//!
//! ## Semantics
//!
//! For a file at `crates/api/src/main.rs`, `has_ancestor:
//! Cargo.toml` walks the ancestor chain `crates/api/src/`,
//! `crates/api/`, `crates/`, root, and returns true on the
//! first match. The walk includes the file's own directory:
//! `crates/api/Cargo.toml` itself matches because
//! `crates/api/` (the file's parent) contains a `Cargo.toml`.
//!
//! See `docs/design/v0.9/scope-filter.md` for full design,
//! pinned decisions, and the bundled-ruleset migration plan.
//!
//! ## Performance
//!
//! Each `has_ancestor` check walks `Path::parent()` upward
//! and consults [`FileIndex::contains_file`] (the v0.9.5
//! path-index) at each step. Both operations are O(1)
//! hashlookups; per-file overhead is `O(depth × M)` where
//! `M` is the number of names in the `has_ancestor` list.
//! Typical: 5 levels × 1 manifest = 150 ns / file. At 1M
//! files × 5 rules with `scope_filter`, total overhead is
//! ~750 ms — and that's before the file-read savings the
//! filter unlocks.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::error::{Error, Result};
use crate::walker::FileIndex;

/// Per-file rule gate. Today's only primitive is
/// `has_ancestor`; the type is an enum-shape struct so future
/// primitives (`closest_ancestor_with_content`, etc.) can land
/// without breaking the public surface.
///
/// Build with [`ScopeFilter::from_spec`] to get the
/// build-time validation (rejects globs, separators, empty
/// lists). Direct construction is allowed for tests via
/// [`ScopeFilter::has_ancestor_unchecked`].
#[derive(Debug, Clone)]
pub struct ScopeFilter {
    has_ancestor: Vec<PathBuf>,
}

/// YAML-level shape of `scope_filter:`. Deserialised by
/// [`RuleSpec`](crate::config::RuleSpec) and validated into
/// the runtime [`ScopeFilter`] via
/// [`ScopeFilter::from_spec`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFilterSpec {
    /// Single literal filename or non-empty list of literal
    /// filenames. Each must be a basename (no path separator,
    /// no glob metacharacters).
    #[serde(deserialize_with = "deserialize_string_or_list")]
    pub has_ancestor: Vec<String>,
}

fn deserialize_string_or_list<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

impl ScopeFilter {
    /// Build from the deserialised spec, validating every
    /// `has_ancestor` entry. Returns `Error::rule_config` on
    /// any of:
    ///
    /// - empty list
    /// - empty string
    /// - string contains a path separator (`/` or `\`)
    /// - string contains a glob metacharacter
    ///   (`* ? [ ] { } !`)
    pub fn from_spec(rule_id: &str, spec: ScopeFilterSpec) -> Result<Self> {
        if spec.has_ancestor.is_empty() {
            return Err(Error::rule_config(
                rule_id,
                "scope_filter.has_ancestor must be a non-empty list",
            ));
        }
        let mut paths = Vec::with_capacity(spec.has_ancestor.len());
        for name in spec.has_ancestor {
            validate_manifest_name(rule_id, &name)?;
            paths.push(PathBuf::from(name));
        }
        Ok(Self { has_ancestor: paths })
    }

    /// Direct construction without validation. Tests only.
    #[doc(hidden)]
    pub fn has_ancestor_unchecked(names: Vec<&str>) -> Self {
        Self {
            has_ancestor: names.into_iter().map(PathBuf::from).collect(),
        }
    }

    /// True iff at least one of the configured ancestor
    /// names exists as a file in some ancestor directory of
    /// `file` — including the file's own directory.
    ///
    /// Walks `Path::parent()` upward from the file, joins the
    /// candidate ancestor name to each directory, and consults
    /// `index.contains_file(...)`. First match wins; the
    /// matching ancestor's path is not exposed (this is a
    /// boolean filter).
    pub fn matches(&self, file: &Path, index: &FileIndex) -> bool {
        let mut cur = file.parent();
        loop {
            let dir = cur.unwrap_or_else(|| Path::new(""));
            for name in &self.has_ancestor {
                let candidate = dir.join(name);
                if index.contains_file(&candidate) {
                    return true;
                }
            }
            match cur {
                Some(p) if p.as_os_str().is_empty() => return false,
                Some(p) => cur = p.parent(),
                None => return false,
            }
        }
    }

    /// The configured ancestor names, for diagnostics and
    /// audits (e.g.
    /// `coverage_audit_scope_filter.rs`).
    pub fn has_ancestor_names(&self) -> &[PathBuf] {
        &self.has_ancestor
    }
}

/// Build-time guard for cross-file rule builders. Cross-file
/// rules express ancestor scoping through `for_each_dir +
/// when_iter:` instead of `scope_filter:`; the engine consults
/// `Rule::scope_filter()` only on the per-file dispatch path,
/// so a cross-file rule with `scope_filter:` set would silently
/// ignore the field. This helper produces a clear build-time
/// error so the misconfiguration surfaces at config-load time
/// rather than as a confused-rule-doesn't-fire bug.
///
/// Usage in a cross-file rule builder:
///
/// ```ignore
/// pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
///     reject_scope_filter_on_cross_file(spec, "for_each_dir")?;
///     // …
/// }
/// ```
pub fn reject_scope_filter_on_cross_file(
    spec: &crate::config::RuleSpec,
    cross_file_kind_label: &str,
) -> Result<()> {
    if spec.scope_filter.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "scope_filter is supported on per-file rules only; {cross_file_kind_label} is a \
                 cross-file rule. Express ancestor scoping via `for_each_dir + when_iter:` \
                 instead — see docs/design/v0.9/scope-filter.md for the pattern."
            ),
        ));
    }
    Ok(())
}

fn validate_manifest_name(rule_id: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::rule_config(
            rule_id,
            "scope_filter.has_ancestor names must not be empty",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a basename — no path separators \
                 (use a literal filename like `Cargo.toml`)"
            ),
        ));
    }
    if name
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '!'))
    {
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a literal — no glob \
                 metacharacters allowed (use `Cargo.toml`, not `*.toml`)"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::{FileEntry, FileIndex};
    use std::path::Path;
    use std::sync::Arc;

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|p| FileEntry {
                    path: Arc::<Path>::from(Path::new(p)),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        )
    }

    fn filter(names: Vec<&str>) -> ScopeFilter {
        ScopeFilter::has_ancestor_unchecked(names)
    }

    #[test]
    fn root_manifest_matches_root_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "lib.rs"]);
        assert!(f.matches(Path::new("lib.rs"), &i));
    }

    #[test]
    fn root_manifest_matches_nested_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "src/lib.rs"]);
        assert!(f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn nested_manifest_matches_own_dir() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml", "crates/api/src/main.rs"]);
        // Manifest at crates/api/ — main.rs's ancestor.
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    #[test]
    fn manifest_at_files_own_dir_matches_the_manifest_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml"]);
        // `Cargo.toml` is in the file's own dir → match.
        assert!(f.matches(Path::new("crates/api/Cargo.toml"), &i));
    }

    #[test]
    fn root_cargo_toml_matches_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml"]);
        assert!(f.matches(Path::new("Cargo.toml"), &i));
    }

    #[test]
    fn no_manifest_in_any_ancestor_returns_false() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["src/lib.rs"]);
        assert!(!f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn sibling_manifest_does_not_match() {
        let f = filter(vec!["Cargo.toml"]);
        // Sibling has Cargo.toml, but our file is in a different subtree.
        let i = idx(&["crates/api/Cargo.toml", "services/web/src/index.ts"]);
        assert!(!f.matches(Path::new("services/web/src/index.ts"), &i));
    }

    #[test]
    fn two_name_list_matches_if_either_found() {
        let f = filter(vec!["pyproject.toml", "setup.py"]);
        let i = idx(&["app/setup.py", "app/main.py"]);
        assert!(f.matches(Path::new("app/main.py"), &i));
    }

    #[test]
    fn closest_ancestor_among_multiple() {
        // Both root and crates/api have Cargo.toml. Either match.
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "crates/api/Cargo.toml", "crates/api/src/main.rs"]);
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    // ── from_spec validation ──────────────────────────────────

    #[test]
    fn from_spec_rejects_empty_list() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec { has_ancestor: vec![] },
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_empty_string() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: vec!["".into()],
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("not be empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_path_separator() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: vec!["foo/bar".into()],
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("path separators"),
            "msg: {err}"
        );
    }

    #[test]
    fn from_spec_rejects_glob_metacharacters() {
        for bad in &["*.toml", "Cargo?", "[abc]", "{a,b}", "!Cargo"] {
            let err = ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: vec![(*bad).into()],
                },
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("glob"),
                "msg for {bad:?}: {err}"
            );
        }
    }

    #[test]
    fn from_spec_accepts_canonical_manifests() {
        for good in &[
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "setup.py",
            "go.mod",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
        ] {
            ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: vec![(*good).into()],
                },
            )
            .unwrap_or_else(|e| panic!("{good:?} should be valid; got {e}"));
        }
    }

    // ── deserialise OneOrMany ─────────────────────────────────

    #[test]
    fn deserialize_single_string_form() {
        let yaml = "has_ancestor: Cargo.toml\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.has_ancestor, vec!["Cargo.toml"]);
    }

    #[test]
    fn deserialize_list_form() {
        let yaml = "has_ancestor:\n  - pom.xml\n  - build.gradle\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            spec.has_ancestor,
            vec!["pom.xml".to_string(), "build.gradle".to_string()],
        );
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        let yaml = "has_ancestor: Cargo.toml\nunknown: x\n";
        assert!(serde_yaml_ng::from_str::<ScopeFilterSpec>(yaml).is_err());
    }
}
