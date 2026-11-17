//! Version 2 names through the loader: every source's namespace, the plan
//! the loaded config carries, and a rule's reference fields resolved in the
//! source that wrote each (design `v0.23/selectors-and-constraints.md`,
//! sections 8 and 10).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alint_core::ConfigSourceClass;
use alint_core::relation::plan::{Plan, Reached, RelationId, RuleReference, SourceIndex};

use super::*;
use crate::v2::REFERENCE_STUB_KIND;

fn write(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

fn stub_builder(_: &alint_core::RuleSpec) -> Result<Box<dyn alint_core::Rule>> {
    Err(Error::Other("the reference stub builds no rule".into()))
}

/// The built-in kinds, and a rule-shaped stub whose `input:` names a
/// relation (no kind of this build has a reference field before phase 7's
/// `constraint`). It reads like `file_exists`: no process, no Git.
fn stub_registry() -> &'static alint_core::RuleRegistry {
    static REGISTRY: std::sync::LazyLock<alint_core::RuleRegistry> =
        std::sync::LazyLock::new(|| {
            let mut registry = alint_core::RuleRegistry::new();
            alint_rules::register_builtin(&mut registry);
            let meta = registry
                .meta("file_exists")
                .expect("a built-in kind")
                .clone();
            registry.register_owned(REFERENCE_STUB_KIND, Arc::new(meta), stub_builder);
            registry
        });
    &REGISTRY
}

fn load_stub(path: &Path) -> Result<Config> {
    crate::load_with(path, &LoadOptions::with_catalog(stub_registry().catalog()))
}

fn plan_of(path: &Path) -> Arc<Plan> {
    load_stub(path)
        .expect("loads")
        .relation_plan
        .expect("a version 2 config has a plan")
}

fn load_error(path: &Path) -> String {
    load_stub(path)
        .expect_err("expected a load error")
        .to_string()
}

const BASE: &str = r#"version: 2
selectors:
  manifests:
    paths: "**/package.json"
relations:
  deps:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { name: entry.key, version: entry.value }
"#;

fn stub_rule(id: &str, input: &str) -> String {
    format!(
        "rules:\n  - id: {id}\n    kind: {REFERENCE_STUB_KIND}\n    paths: x\n    level: warning\n    \
         input: {input}\n"
    )
}

fn display(relative: &str) -> String {
    relative
        .split('/')
        .collect::<PathBuf>()
        .display()
        .to_string()
}

#[test]
fn the_plan_holds_every_source_with_its_aliases() {
    let tmp = tempfile::tempdir().unwrap();
    let base = write(tmp.path(), "rules/base.yml", BASE);
    write(tmp.path(), "rules/plain.yml", "version: 1\nrules: []\n");
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n  - ./rules/plain.yml\n\
         relations:\n  pinned:\n    from: base.manifests\n    read: { json: $.version }\n    \
         fields: { version: value }\n",
    );
    write(
        tmp.path(),
        ".alint.d/10-docs.yml",
        "selectors:\n  docs: { paths: 'docs/**' }\n",
    );
    let plan = plan_of(&path);
    let sources: Vec<_> = plan
        .sources
        .iter()
        .map(|source| (source.display.clone(), source.class))
        .collect();
    assert_eq!(
        sources,
        vec![
            (display("rules/base.yml"), ConfigSourceClass::LocalExtend),
            (".alint.yml".to_string(), ConfigSourceClass::Root),
            (display(".alint.d/10-docs.yml"), ConfigSourceClass::DropIn),
        ]
    );
    assert_eq!(plan.sources[0].path, base.canonicalize().unwrap());
    assert_eq!(plan.sources[1].aliases[0].name, "base");
    assert_eq!(plan.sources[1].aliases[0].target, Some(SourceIndex(0)));
    let pinned = &plan.relations[1];
    assert_eq!(
        (pinned.name.as_str(), pinned.source),
        ("pinned", SourceIndex(1))
    );
    assert_eq!(plan.selectors[1].name, "docs");
    assert_eq!(plan.selectors[1].source, SourceIndex(2));
}

#[test]
fn an_unknown_name_names_the_source_that_wrote_it() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/base.yml",
        "version: 2\nrelations:\n  r:\n    from: manifest\n    read: { json: $.a }\n    fields: \
         { a: value }\nselectors:\n  manifests: { paths: x }\n",
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n",
    );
    assert_eq!(
        load_error(&path),
        format!(
            "{}: `relations.r.from`: no selector named `manifest` in this config; the selectors \
             it can reach are `manifests`",
            display("rules/base.yml")
        )
    );
}

/// The reference rule `id` makes, as the loaded plan records it.
fn reference(plan: &Plan, id: &str) -> RuleReference {
    plan.rule(id)
        .expect("the rule references a relation")
        .references[0]
        .clone()
}

#[test]
fn a_rule_field_resolves_in_the_source_that_wrote_it() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/base.yml",
        &format!("{BASE}{}", stub_rule("r", "deps")),
    );
    let extends = "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n";

    // The consumer overrides another field: `input:` stays base's, so `deps`
    // is base's relation.
    let path = write(
        tmp.path(),
        ".alint.yml",
        &format!("{extends}rules:\n  - id: r\n    level: error\n"),
    );
    let plan = plan_of(&path);
    assert_eq!(
        reference(&plan, "r"),
        RuleReference {
            field: "input".into(),
            source: SourceIndex(0),
            relation: Reached {
                id: RelationId(0),
                alias: None,
            },
        }
    );

    // The consumer overrides `input:` itself: it resolves in the consumer,
    // through the alias ...
    std::fs::write(
        &path,
        format!("{extends}rules:\n  - id: r\n    input: base.deps\n"),
    )
    .unwrap();
    let plan = plan_of(&path);
    assert_eq!(
        reference(&plan, "r"),
        RuleReference {
            field: "input".into(),
            source: SourceIndex(1),
            relation: Reached {
                id: RelationId(0),
                alias: Some("base".into()),
            },
        }
    );

    // ... so the bare name base wrote names nothing there.
    std::fs::write(
        &path,
        format!("{extends}rules:\n  - id: r\n    input: deps\n"),
    )
    .unwrap();
    assert_eq!(
        load_error(&path),
        format!(
            "rule \"r\": `input` (written by .alint.yml): no relation named `deps` in this \
             config; {} declares it, which this config extends as `base`: write `base.deps` (an \
             extended config's objects are reached by a qualified name)",
            display("rules/base.yml")
        )
    );
}

#[test]
fn a_template_field_resolves_in_the_template_source() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/base.yml",
        &format!(
            "{BASE}templates:\n  - id: t\n    kind: {REFERENCE_STUB_KIND}\n    paths: x\n    \
             level: warning\n    input: deps\n"
        ),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\nrules:\n  - id: r\n    \
         extends_template: t\n",
    );
    assert_eq!(reference(&plan_of(&path), "r").source, SourceIndex(0));
    // An instance that writes the field resolves it itself.
    std::fs::write(
        &path,
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\nrules:\n  - id: r\n    \
         extends_template: t\n    input: base.deps\n",
    )
    .unwrap();
    let reference = reference(&plan_of(&path), "r");
    assert_eq!(reference.source, SourceIndex(1));
    assert_eq!(reference.relation.alias.as_deref(), Some("base"));
}

#[test]
fn a_fragment_rule_resolves_in_the_fragment() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", BASE);
    write(
        tmp.path(),
        ".alint.d/10.yml",
        &format!(
            "relations:\n  local:\n    from: {{ paths: x }}\n    read: {{ json: $.a }}\n    \
             fields: {{ a: value }}\n{}",
            stub_rule("r", "local")
        ),
    );
    let reference = reference(&plan_of(&path), "r");
    assert_eq!(reference.source, SourceIndex(1));
    assert_eq!(reference.relation.id, RelationId(1));
}

#[test]
fn a_reference_field_a_version_one_config_wrote_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/plain.yml",
        &format!("version: 1\n{}", stub_rule("r", "deps")),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        &format!("{BASE}extends: [./rules/plain.yml]\n"),
    );
    assert_eq!(
        load_error(&path),
        "rule \"r\": `input` names a relation, but a version 1 config wrote it; only a version 2 \
         config's rules reference selectors and relations"
    );
}

#[test]
fn a_version_one_config_records_no_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 1\n{}", stub_rule("r", "deps")),
    );
    let config = load_stub(&path).expect("version 1 resolves nothing");
    assert!(config.relation_plan.is_none());
}

/// The suggestion for an unknown name reads that name once, however many
/// names a config declares and however long the unknown one is, as a value
/// can be (no YAML key limit applies): two thousand selectors and a `from:`
/// two hundred thousand characters long, which a suggestion that copied the
/// unknown name once per selector read four hundred million characters of.
/// An alias a long unknown name is weighed against is read as far as it can
/// come within two edits, once for all the names it prefixes.
#[test]
fn an_unknown_name_is_read_once_however_many_names_it_is_weighed_against() {
    use std::fmt::Write as _;

    let tmp = tempfile::tempdir().unwrap();
    let mut selectors = String::from("version: 2\nselectors:\n");
    for index in 0..2_000 {
        writeln!(selectors, "  s{index}: {{ paths: a }}").unwrap();
    }
    write(tmp.path(), "base.yml", &selectors);
    let wanted = format!("z{}", "y".repeat(200_000));
    let relation = format!(
        "relations:\n  r:\n    from: {wanted}\n    read: {{ whole_file: {{}} }}\n    fields: {{ \
         v: value }}\nrules: []\n"
    );
    let alias = "a".repeat(200_000);
    for (config, reach) in [
        (format!("{selectors}{relation}"), "selectors it can reach"),
        (
            format!("version: 2\nextends:\n  - url: ./base.yml\n    as: {alias}\n{relation}"),
            "selectors it can reach",
        ),
    ] {
        let path = write(tmp.path(), ".alint.yml", &config);
        let (error, read) =
            crate::tests::on_thread(8 << 20, std::time::Duration::from_secs(20), move || {
                crate::interp::DISTANCE_CHARS.with(|read| read.set(0));
                let error = load_error(&path);
                (
                    error,
                    crate::interp::DISTANCE_CHARS.with(std::cell::Cell::get),
                )
            });
        assert!(
            error.ends_with(&format!(
                "no selector named `{wanted}` in this config; there are 2000 {reach}, none \
                 named like `{wanted}`"
            )),
            "{}",
            error.chars().take(300).collect::<String>()
        );
        assert!(read <= wanted.len() + 3, "{read} characters read");
    }
}
