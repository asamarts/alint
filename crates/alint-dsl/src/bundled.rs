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
}
