//! Registry injection through the CLI: every surface that builds or describes
//! rules must use `Cli::registry`, so an embedder's own kind runs and is listed
//! while the built-in catalog stays the default.

use std::borrow::Cow;
use std::path::Path;
use std::process::ExitCode;

use alint_core::{
    ContentAccess, Context, DispatchClass, GitTrackedSupport, KindMeta, Level, PathsSupport, Rule,
    RuleRegistry, RuleSpec, ScopeFilterSupport, SinceSupport, TraceMeta, Violation,
};
use alint_output::Format;
use clap::Parser;

use super::{Cli, run};

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

/// The built-ins plus `custom_marker` (alias `custom_mark`).
fn registry_with_marker() -> RuleRegistry {
    let mut registry = alint_rules::builtin_registry();
    registry.register("custom_marker", &MARKER_META, build_marker);
    registry.register_alias("custom_mark", "custom_marker");
    registry
}

const MARKER_CONFIG: &str =
    "version: 1\nrules:\n  - id: marker\n    kind: custom_mark\n    level: error\n";

fn cli(args: &[&str], registry: Option<RuleRegistry>) -> Cli {
    let mut cli = Cli::try_parse_from(std::iter::once("alint").chain(args.iter().copied()))
        .expect("arguments parse");
    cli.registry = registry;
    cli
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("temporary paths are UTF-8")
}

#[test]
fn check_runs_a_custom_kind_from_the_injected_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".alint.yml"), MARKER_CONFIG).unwrap();
    let args = ["--format", "json", "check", path_arg(dir.path())];

    let injected = run(cli(&args, Some(registry_with_marker()))).expect("the config loads");
    assert_eq!(
        injected,
        ExitCode::from(1),
        "the custom rule's finding fails the check"
    );

    let error = run(cli(&args, None)).expect_err("the built-in registry has no such kind");
    assert!(format!("{error:#}").contains("custom_mark"), "{error:#}");
}

#[test]
fn policy_tests_run_a_custom_kind_from_the_injected_registry() {
    let dir = tempfile::tempdir().unwrap();
    let test_path = dir.path().join("marker.yml");
    std::fs::write(
        &test_path,
        "test_version: 1\nname: marker\ngiven:\n  tree: {}\n  config: |\n    version: 1\n    \
rules:\n      - id: marker\n        kind: custom_marker\n        level: error\nwhen: [check]\n\
expect:\n  - exit: policy_failure\n    violations:\n      - {rule: marker, level: error, message: \
{regex: \"^custom kind ran$\"}}\n",
    )
    .unwrap();
    let args = ["--format", "json", "test", path_arg(&test_path)];

    let injected = run(cli(&args, Some(registry_with_marker()))).expect("the runner runs");
    assert_eq!(injected, ExitCode::SUCCESS, "the expectation matches");

    let builtin = run(cli(&args, None)).expect("the runner runs");
    assert_eq!(
        builtin,
        ExitCode::from(1),
        "without the kind the config fails to load, so the expectation does not match"
    );
}

fn rules_output(command: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>) -> String {
    let mut out = Vec::new();
    command(&mut out).expect("the command succeeds");
    String::from_utf8(out).unwrap()
}

#[test]
fn rules_commands_describe_the_injected_registry() {
    let registry = registry_with_marker();

    let list = rules_output(|out| crate::rules::list(out, &registry, None, None, Format::Json));
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    let rules = list["rules"].as_array().unwrap();
    let marker = rules
        .iter()
        .find(|rule| rule["kind"] == "custom_marker")
        .expect("the custom kind is listed");
    assert_eq!(marker["categories"], serde_json::json!([]));
    assert_eq!(marker["aliases"], serde_json::json!(["custom_mark"]));
    assert_eq!(marker["summary"], serde_json::Value::Null);
    assert_eq!(
        rules.last().unwrap()["kind"],
        "custom_marker",
        "kinds the catalog does not describe follow the catalogued ones"
    );

    let show =
        rules_output(|out| crate::rules::show(out, &registry, "custom_mark", Format::Json, true));
    let show: serde_json::Value = serde_json::from_str(&show).unwrap();
    assert_eq!(show["rule_kind"], "custom_marker");
    assert_eq!(show["docs"], serde_json::Value::Null);

    let human = rules_output(|out| {
        crate::rules::show(out, &registry, "custom_marker", Format::Human, true)
    });
    assert!(human.contains("categories: uncategorized"), "{human}");
    assert!(!human.contains("docs:"), "{human}");
}

#[test]
fn rules_commands_list_only_kinds_the_registry_can_build() {
    let mut registry = RuleRegistry::new();
    registry.register("custom_marker", &MARKER_META, build_marker);

    let list = rules_output(|out| crate::rules::list(out, &registry, None, None, Format::Json));
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    let kinds: Vec<&str> = list["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| rule["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["custom_marker"]);

    let categories = rules_output(|out| crate::rules::categories(out, &registry, Format::Json));
    let categories: serde_json::Value = serde_json::from_str(&categories).unwrap();
    assert!(
        categories["categories"]
            .as_array()
            .unwrap()
            .iter()
            .all(|category| category["kinds"] == 0),
        "{categories}"
    );

    // A catalog kind the registry lacks is an error, not a panic.
    let mut out = Vec::new();
    let error = crate::rules::show(&mut out, &registry, "file_exists", Format::Human, true)
        .expect_err("the registry has no file_exists");
    assert!(error.to_string().contains("unknown rule kind"), "{error}");
}
