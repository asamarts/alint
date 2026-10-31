//! Registry injection through the language server: a session built from an
//! injected registry runs its kinds, and hover docs resolve aliases through it.

use std::borrow::Cow;

use alint_core::{
    ContentAccess, Context, DispatchClass, GitTrackedSupport, KindMeta, Level, PathsSupport, Rule,
    RuleSpec, ScopeFilterSupport, SinceSupport, TraceMeta, Violation,
};

use super::Finding;

static MARKER_META: KindMeta = KindMeta {
    dispatch: DispatchClass::SingleShot,
    paths: PathsSupport::Forbidden,
    scope_filter: ScopeFilterSupport::Rejected {
        reason: Cow::Borrowed("not file-scoped"),
    },
    git_tracked_only: GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: Cow::Borrowed(&[]),
    since: SinceSupport::None,
    capabilities: Cow::Borrowed(&[]),
    content: ContentAccess::PathOnly,
    formats: Cow::Borrowed(&[]),
    dependencies: Cow::Borrowed(&[]),
    fix_ops: Cow::Borrowed(&[]),
    expect_matches: false,
    trace: TraceMeta::ASSERTION,
    nested_rules: false,
};

/// A kind no built-in registry has: it always reports one finding.
#[derive(Debug)]
struct MarkerRule {
    id: String,
    level: Level,
}

impl Rule for MarkerRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn level(&self) -> Level {
        self.level
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> alint_core::Result<Vec<Violation>> {
        Ok(vec![Violation::new("custom kind ran")])
    }
}

// The function must match the public `RuleBuilder` signature.
#[allow(clippy::unnecessary_wraps)]
fn build_marker(spec: &RuleSpec) -> alint_core::Result<Box<dyn Rule>> {
    Ok(Box::new(MarkerRule {
        id: spec.id.clone(),
        level: spec.level,
    }))
}

#[test]
fn a_session_runs_kinds_and_resolves_aliases_from_the_injected_registry() {
    let mut registry = alint_rules::builtin_registry();
    registry.register("custom_marker", &MARKER_META, build_marker);
    registry.register_alias("must_exist", "file_exists");

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        "version: 1\nrules:\n  - id: marker\n    kind: custom_marker\n    level: error\n  \
- id: readme\n    kind: must_exist\n    paths: README.md\n    level: error\n",
    )
    .unwrap();

    let check = super::config::check_workspace(&[dir.path().to_path_buf()], &[], None, &registry);
    let [(_, run)] = &check.runs[..] else {
        panic!("one config run: {:?}", check.runs.len());
    };
    let run = run
        .as_ref()
        .expect("the config runs with the injected registry");
    let findings = &run.by_path[&config_path];
    let finding = |id: &str| -> &Finding {
        findings
            .iter()
            .find(|finding| finding.rule_id == id)
            .unwrap_or_else(|| panic!("a finding for {id}: {findings:?}"))
    };

    let marker = finding("marker");
    assert_eq!(marker.message, "custom kind ran");
    assert_eq!(marker.docs_url, None, "the catalog has no page for it");
    assert_eq!(marker.description, None);

    let readme = finding("readme");
    assert_eq!(
        readme.docs_url,
        super::render::rule_docs_url("file_exists"),
        "the injected alias resolves to its canonical kind's page"
    );
    assert!(readme.description.is_some());

    let builtin = super::config::check_workspace(
        &[dir.path().to_path_buf()],
        &[],
        None,
        &alint_rules::builtin_registry(),
    );
    let [(_, run)] = &builtin.runs[..] else {
        panic!("one config run: {:?}", builtin.runs.len());
    };
    assert!(
        run.as_ref().is_err(),
        "the built-in registry knows neither kind"
    );
}
