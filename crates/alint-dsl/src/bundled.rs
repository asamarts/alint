//! Bundled rulesets — YAML ruleset bodies embedded in the alint
//! binary, keyed by a stable `alint://bundled/<name>@<rev>` URI
//! scheme and resolved entirely offline.
//!
//! Each ruleset is a `.alint.yml` fragment designed to be used from
//! a user's own config via:
//!
//! ```yaml
//! extends:
//!   - alint://bundled/oss-baseline@v1
//! ```
//!
//! Bundled rulesets are deliberately limited: no `extends:` of
//! their own (same rule as HTTPS extends), no `facts:` that read
//! from the user repo's state, no `custom:` program execution.
//! They're pure static rule catalogs that users layer on top of
//! their own config.
//!
//! ## Versioning
//!
//! The `<rev>` suffix (e.g. `@v1`) lets rulesets evolve on a
//! separate schedule from the binary. A binary may ship multiple
//! revisions of the same ruleset simultaneously. New revs are
//! added; old revs are not removed until a major binary release.

/// Every bundled ruleset known to this build, as a
/// `(name, rev) -> embedded body` lookup table. Rulesets come
/// from files under `crates/alint-dsl/rulesets/<rev>/<name>.yml`,
/// embedded at compile time via `include_str!`. They live inside
/// the crate (rather than at the repo root) so `cargo publish`
/// bundles them into the crates.io tarball.
const REGISTRY: &[(&str, &str, &str)] = &[
    // Ecosystem / project-shape baselines.
    (
        "oss-baseline",
        "v1",
        include_str!("../rulesets/v1/oss-baseline.yml"),
    ),
    ("rust", "v1", include_str!("../rulesets/v1/rust.yml")),
    (
        "monorepo",
        "v1",
        include_str!("../rulesets/v1/monorepo.yml"),
    ),
    ("node", "v1", include_str!("../rulesets/v1/node.yml")),
    ("python", "v1", include_str!("../rulesets/v1/python.yml")),
    ("go", "v1", include_str!("../rulesets/v1/go.yml")),
    ("java", "v1", include_str!("../rulesets/v1/java.yml")),
    // Namespaced utility rulesets. Slash-separated names are
    // resolved through the usual `alint://bundled/<name>@<rev>`
    // URI — the `@` separator splits name from rev, so slashes
    // inside the name are unambiguous.
    (
        "hygiene/no-tracked-artifacts",
        "v1",
        include_str!("../rulesets/v1/hygiene/no-tracked-artifacts.yml"),
    ),
    (
        "hygiene/lockfiles",
        "v1",
        include_str!("../rulesets/v1/hygiene/lockfiles.yml"),
    ),
    (
        "tooling/editorconfig",
        "v1",
        include_str!("../rulesets/v1/tooling/editorconfig.yml"),
    ),
    (
        "docs/adr",
        "v1",
        include_str!("../rulesets/v1/docs/adr.yml"),
    ),
    (
        "ci/github-actions",
        "v1",
        include_str!("../rulesets/v1/ci/github-actions.yml"),
    ),
    // Workspace-aware overlays — thin extensions of `monorepo@v1`
    // gated by a workspace-flavor fact (Cargo `[workspace]` /
    // pnpm-workspace.yaml / package.json `"workspaces"`). Use
    // `iter.has_file(...)` (v0.5.2) to scope per-member checks
    // to actual package directories.
    (
        "monorepo/cargo-workspace",
        "v1",
        include_str!("../rulesets/v1/monorepo/cargo-workspace.yml"),
    ),
    (
        "monorepo/pnpm-workspace",
        "v1",
        include_str!("../rulesets/v1/monorepo/pnpm-workspace.yml"),
    ),
    (
        "monorepo/yarn-workspace",
        "v1",
        include_str!("../rulesets/v1/monorepo/yarn-workspace.yml"),
    ),
    // License-compliance overlays. Adopting one of these is
    // the user's signal that the project intends to be
    // compliant with the named scheme — no fact gating.
    (
        "compliance/reuse",
        "v1",
        include_str!("../rulesets/v1/compliance/reuse.yml"),
    ),
    (
        "compliance/apache-2",
        "v1",
        include_str!("../rulesets/v1/compliance/apache-2.yml"),
    ),
    // Agentic-era rulesets (v0.6). Composes with the existing
    // hygiene/* and tooling/* sets — `agent-hygiene@v1` covers
    // the patterns that are *distinctly* AI-shaped (versioned
    // duplicate filenames, scratch-doc sprawl, AI-affirmation
    // prose, debug residue, model-attributed TODOs) without
    // duplicating what `hygiene/no-tracked-artifacts@v1`
    // already catches. `agent-context@v1` lints the
    // agent-instruction files (AGENTS.md / CLAUDE.md /
    // .cursorrules / GEMINI.md / copilot-instructions.md) for
    // existence + stub + bloat + stale-path drift.
    (
        "agent-hygiene",
        "v1",
        include_str!("../rulesets/v1/agent-hygiene.yml"),
    ),
    (
        "agent-context",
        "v1",
        include_str!("../rulesets/v1/agent-context.yml"),
    ),
];

/// Resolve a `<name>@<rev>` spec (the path portion of an
/// `alint://bundled/<name>@<rev>` URL) to its embedded body.
/// Returns `None` if the name / rev combination is unknown.
pub fn resolve(spec: &str) -> Option<&'static str> {
    let (name, rev) = spec.split_once('@')?;
    REGISTRY
        .iter()
        .find(|(n, r, _)| *n == name && *r == rev)
        .map(|(_, _, body)| *body)
}

/// The list of every bundled ruleset this build ships — used by
/// `alint list bundled` and by test assertions.
pub fn catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    REGISTRY.iter().map(|(name, rev, _)| (*name, *rev))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_ruleset() {
        let body = resolve("oss-baseline@v1").expect("oss-baseline@v1 must be registered");
        assert!(body.contains("oss-readme-exists"), "body: {body:.80}");
    }

    #[test]
    fn unknown_name_returns_none() {
        assert!(resolve("definitely-not-shipped@v1").is_none());
    }

    #[test]
    fn unknown_rev_returns_none() {
        assert!(resolve("oss-baseline@v99").is_none());
    }

    #[test]
    fn malformed_spec_returns_none() {
        // No `@` separator.
        assert!(resolve("oss-baseline").is_none());
    }

    #[test]
    fn every_bundled_ruleset_parses_as_valid_config() {
        for (name, rev) in catalog() {
            let spec = format!("{name}@{rev}");
            let body = resolve(&spec).unwrap();
            let parsed: Result<alint_core::Config, _> = serde_yaml_ng::from_str(body);
            assert!(
                parsed.is_ok(),
                "bundled ruleset '{spec}' failed to parse: {}",
                parsed.unwrap_err()
            );
        }
    }

    #[test]
    fn bundled_rulesets_do_not_use_extends() {
        // `load_bundled` rejects these at runtime, but catch it at
        // test time so shipping an invalid ruleset is a test
        // failure, not a user-facing error.
        for (name, rev) in catalog() {
            let spec = format!("{name}@{rev}");
            let body = resolve(&spec).unwrap();
            let parsed: alint_core::Config = serde_yaml_ng::from_str(body).unwrap();
            assert!(
                parsed.extends.is_empty(),
                "bundled ruleset '{spec}' declares `extends:`, which is not allowed"
            );
        }
    }

    /// `xtask docs-export` parses the leading comment block of each
    /// bundled-ruleset YAML and renders it as the doc page's
    /// overview. The contract is:
    ///
    ///     # alint://bundled/<name>@v<rev>
    ///     #
    ///     # <prose description>
    ///     ...
    ///
    /// Line 1 is the canonical URI tag (the renderer strips it),
    /// line 2 is a blank `#`, and line 3 starts the prose. A
    /// missing tag or empty comment block ships a doc page with
    /// the wrong title or no overview at all — silently. This
    /// test catches the gap before merge.
    ///
    /// The same contract is enforced as a dogfood rule in the
    /// repo's `.alint.yml` (`bundled-ruleset-has-uri-header`).
    /// Both checks exist: the rule surfaces the failure in
    /// linter output where contributors expect it; this test is
    /// faster (cargo-test scale) and harder to bypass with bad
    /// path globs.
    #[test]
    fn every_bundled_ruleset_has_uri_header_and_overview() {
        for (name, rev) in catalog() {
            let spec = format!("{name}@{rev}");
            let body = resolve(&spec).unwrap();
            let mut lines = body.lines();
            let l1 = lines.next().unwrap_or("");
            let l2 = lines.next().unwrap_or("");
            let l3 = lines.next().unwrap_or("");

            let expected_uri = format!("# alint://bundled/{name}@{rev}");
            assert_eq!(
                l1, expected_uri,
                "ruleset '{spec}' line 1 must be the canonical URI tag {expected_uri:?}; got {l1:?}"
            );
            assert_eq!(
                l2, "#",
                "ruleset '{spec}' line 2 must be a blank `#` (separating URI tag from prose); got {l2:?}"
            );
            assert!(
                l3.starts_with("# ") && l3.len() > 2,
                "ruleset '{spec}' line 3 must start with `# ` followed by description prose; got {l3:?}"
            );
        }
    }
}
