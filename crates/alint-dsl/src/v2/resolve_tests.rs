//! Unit tests of per-source name resolution and the object gates
//! (`v2/resolve.rs`).

use std::path::PathBuf;

use alint_core::relation::plan::{Capabilities, RuleReference};
use alint_core::{Capability, ConfigSourceClass, GitOperation};
use ayaml::Value;

use super::*;
use crate::v2::model::SourceObjects;
use crate::v2::{ExtendsEntry, SourceId};

fn file(display: &str) -> PathBuf {
    PathBuf::from("/repo").join(display)
}

/// Decode the `selectors:`, `relations:` and `extends:` of `yaml` as the
/// loader does for the version 2 source `display`, and record each local
/// `extends:` entry's target as the loader would (`./x.yml` is `/repo/x.yml`).
fn source(display: &str, class: ConfigSourceClass, yaml: &str) -> SourceObjects {
    let document: Value = alint_core::yaml::from_str(yaml).expect("parse");
    let mut v2_keys = Vec::new();
    let mut extends: Vec<ExtendsEntry> = Vec::new();
    for (key, value) in document.as_mapping().expect("a mapping") {
        match key.as_str() {
            Some(name @ ("selectors" | "relations")) => {
                v2_keys.push((name.to_string(), value.clone()));
            }
            Some("extends") => {
                extends = crate::core_yaml::decode(value.clone()).expect("extends");
            }
            _ => {}
        }
    }
    let id = SourceId {
        display: display.into(),
        path: file(display),
        class,
    };
    let decoded = crate::v2::source_objects(id, &v2_keys, &extends).expect("decode");
    let mut one = Objects {
        sources: vec![decoded],
    };
    for (index, entry) in extends.iter().enumerate() {
        let url = entry.entry.url();
        let target = file(url.trim_start_matches("./"));
        one.record_extended(index, Some(&target));
    }
    one.sources.remove(0)
}

fn root(yaml: &str) -> SourceObjects {
    source(".alint.yml", ConfigSourceClass::Root, yaml)
}

fn extended(display: &str, yaml: &str) -> SourceObjects {
    source(display, ConfigSourceClass::LocalExtend, yaml)
}

fn plan(sources: Vec<SourceObjects>) -> Plan {
    resolve(&Objects { sources }).expect("resolves").plan
}

fn refused(sources: Vec<SourceObjects>) -> String {
    resolve(&Objects { sources })
        .map(drop)
        .expect_err("expected an error")
        .to_string()
}

const BASE: &str = r#"
selectors:
  manifests: { paths: "**/package.json" }
relations:
  deps:
    from: manifests
    entries: { format: json, at: $.dependencies }
    fields: { name: entry.key, version: entry.value }
"#;

fn from_of(plan: &Plan, relation: &str) -> Reached<SelectorId> {
    let named = plan
        .relations
        .iter()
        .find(|named| named.name == relation)
        .unwrap_or_else(|| panic!("no relation {relation}"));
    match &named.relation.rows {
        plan::Rows::Part(plan::Part {
            from: plan::SelectorRef::Named(reached),
            ..
        }) => reached.clone(),
        other => panic!("not a named `from:`: {other:?}"),
    }
}

fn reached<Id>(id: Id, alias: Option<&str>) -> Reached<Id> {
    Reached {
        id,
        alias: alias.map(str::to_string),
    }
}

#[test]
fn a_bare_name_resolves_in_its_own_source_and_a_qualified_one_through_the_alias() {
    let plan = plan(vec![
        extended("base.yml", BASE),
        root(
            r"
extends:
  - url: ./base.yml
    as: base
selectors:
  manifests: { paths: package.json }
relations:
  pinned:
    from: base.manifests
    read: { json: $.version }
    fields: { version: value }
  local:
    from: manifests
    read: { json: $.name }
    fields: { name: value }
",
        ),
    ]);
    // Nothing shadows: base's `deps` reads base's `manifests`, though the
    // root declares a selector of that name too.
    assert_eq!(from_of(&plan, "deps"), reached(SelectorId(0), None));
    assert_eq!(
        from_of(&plan, "pinned"),
        reached(SelectorId(0), Some("base"))
    );
    assert_eq!(from_of(&plan, "local"), reached(SelectorId(1), None));
    assert_eq!(plan.selector(SelectorId(1)).source, SourceIndex(1));
    assert_eq!(
        plan.sources
            .iter()
            .map(|source| source.display.as_str())
            .collect::<Vec<_>>(),
        ["base.yml", ".alint.yml"]
    );
    assert_eq!(
        plan.source(SourceIndex(1)).aliases,
        vec![PlanAlias {
            name: "base".into(),
            url: "./base.yml".into(),
            target: Some(SourceIndex(0)),
        }]
    );
    let consumers: Vec<_> = plan.aliases_by_source()[0]
        .iter()
        .map(|(source, alias)| (plan.source(*source).display.as_str(), alias.name.as_str()))
        .collect();
    assert_eq!(consumers, [(".alint.yml", "base")]);
}

#[test]
fn join_sides_and_membership_references_resolve() {
    let plan = plan(vec![
        extended("base.yml", BASE),
        root(
            r"
extends:
  - url: ./base.yml
    as: base
relations:
  allowed:
    from: { paths: allowed.json }
    read: { json: '$[*]' }
    fields: { name: value }
  checked:
    join: { left: base.deps, right: allowed, on: { name: name }, unique: right }
    fields: { name: left.name }
    where:
      name: { not_in: [{ relation: allowed, field: name }, unlisted] }
",
        ),
    ]);
    let checked = &plan.relations[2];
    let plan::Rows::Join(join) = &checked.relation.rows else {
        panic!("not a join");
    };
    assert_eq!(join.left, reached(RelationId(0), Some("base")));
    assert_eq!(join.right, reached(RelationId(1), None));
    assert_eq!(join.on, vec![("name".to_string(), "name".to_string())]);
    assert_eq!(
        checked.relation.filters,
        vec![(
            "name".to_string(),
            vec![plan::Predicate::NotIn(vec![
                plan::Member::Relation {
                    relation: reached(RelationId(1), None),
                    field: "name".into(),
                },
                plan::Member::Literal(alint_core::relation::Value::String("unlisted".into())),
            ])]
        )]
    );
}

const MANY: &str = r"
selectors:
  alpha: { paths: a }
  beta: { paths: b }
  gamma: { paths: c }
  delta: { paths: d }
  epsilon: { paths: e }
  zeta: { paths: f }
  eta: { paths: g }
";

fn reading(from: &str) -> String {
    format!(
        "relations:\n  titles:\n    from: {from}\n    read: {{ regex: '^# (.*)' }}\n    \
         fields: {{ title: value }}\n"
    )
}

#[test]
fn an_unknown_bare_name_lists_what_the_source_can_reach() {
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            root(&format!(
                "extends:\n  - url: ./base.yml\n    as: base\nselectors:\n  docs: {{ paths: \
                 docs }}\n{}",
                reading("doc")
            )),
        ]),
        ".alint.yml: `relations.titles.from`: no selector named `doc` in this config; the \
         selectors it can reach are `docs`, `base.manifests`"
    );
    assert_eq!(
        refused(vec![root(&reading("docs"))]),
        ".alint.yml: `relations.titles.from`: no selector named `docs` in this config; it \
         declares no selectors and reaches none through an `extends:` alias"
    );
}

#[test]
fn an_unknown_bare_name_among_many_suggests_the_nearest() {
    assert_eq!(
        refused(vec![root(&format!("{MANY}{}", reading("gama")))]),
        ".alint.yml: `relations.titles.from`: no selector named `gama` in this config; did you \
         mean `gamma`?"
    );
    assert_eq!(
        refused(vec![root(&format!("{MANY}{}", reading("unrelated")))]),
        ".alint.yml: `relations.titles.from`: no selector named `unrelated` in this config; \
         there are 7 selectors it can reach, none named like `unrelated`"
    );
}

#[test]
fn a_bare_name_an_extended_source_declares_is_refused_with_the_qualified_one() {
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            root(&format!(
                "extends:\n  - url: ./base.yml\n    as: base\n{}",
                reading("manifests")
            )),
        ]),
        ".alint.yml: `relations.titles.from`: no selector named `manifests` in this config; \
         base.yml declares it, which this config extends as `base`: write `base.manifests` (an \
         extended config's objects are reached by a qualified name)"
    );
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            root(&format!("extends: [./base.yml]\n{}", reading("manifests"))),
        ]),
        ".alint.yml: `relations.titles.from`: no selector named `manifests` in this config; \
         base.yml declares it, which this config extends: an extended config's objects are \
         reached only by a qualified name, so give that `extends:` entry an alias (`as: \
         <alias>`) and write `<alias>.manifests`"
    );
}

#[test]
fn an_unknown_alias_is_refused() {
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            root(&format!(
                "extends: [./base.yml]\n{}",
                reading("base.manifests")
            )),
        ]),
        ".alint.yml: `relations.titles.from`: no `extends:` alias `base` in this config (in \
         `base.manifests`); this config gives no `extends:` entry an alias: write `as: base` on \
         the entry of the config that declares `manifests`"
    );
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            root(&format!(
                "extends:\n  - url: ./base.yml\n    as: shared\n{}",
                reading("base.manifests")
            )),
        ]),
        ".alint.yml: `relations.titles.from`: no `extends:` alias `base` in this config (in \
         `base.manifests`); its aliases are `shared`"
    );
}

#[test]
fn a_qualified_name_reaches_only_what_the_aliased_source_declares_itself() {
    let mid = || {
        extended(
            "mid.yml",
            "extends:\n  - url: ./base.yml\n    as: base\nselectors:\n  own: { paths: x }\n",
        )
    };
    let consumer = |from: &str| {
        root(&format!(
            "extends:\n  - url: ./mid.yml\n    as: mid\n{}",
            reading(from)
        ))
    };
    // A re-export: base's selector, reached through mid.
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            mid(),
            consumer("mid.manifests")
        ]),
        ".alint.yml: `relations.titles.from`: mid.yml (`mid`) declares no selector named \
         `manifests`; base.yml declares it, which mid.yml extends: a qualified name reaches \
         only the objects the aliased config declares itself (there is no re-export), so \
         extend base.yml from this config with an alias of its own and write `<alias>.manifests`"
    );
    // mid's own alias is no object of mid.
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            mid(),
            consumer("mid.base")
        ]),
        ".alint.yml: `relations.titles.from`: `base` is an `extends:` alias of mid.yml (`mid`), \
         not a selector: a qualified name reaches only the objects the aliased config declares \
         itself, so there is no chained name"
    );
    assert_eq!(
        refused(vec![
            extended("base.yml", BASE),
            mid(),
            consumer("mid.missing")
        ]),
        ".alint.yml: `relations.titles.from`: mid.yml (`mid`) declares no selector named \
         `missing`; the selectors it declares are `mid.own`"
    );
}

#[test]
fn a_reference_to_the_other_kind_is_refused() {
    let base = || extended("base.yml", BASE);
    let aliased = "extends:\n  - url: ./base.yml\n    as: base\n";
    assert_eq!(
        refused(vec![
            base(),
            root(&format!("{aliased}{}", reading("base.deps")))
        ]),
        ".alint.yml: `relations.titles.from`: `base.deps` is a relation; `from:` takes a \
         selector name or an inline selector map"
    );
    assert_eq!(
        refused(vec![base(), root(&format!("{aliased}{}", reading("base")))]),
        ".alint.yml: `relations.titles.from`: `base` is an `extends:` alias, not a selector; \
         write `base.<name>` to reach a selector the config it names declares"
    );
    assert_eq!(
        refused(vec![root(&format!(
            "selectors:\n  docs: {{ paths: x }}\n{}",
            reading("docs.x")
        ))]),
        ".alint.yml: `relations.titles.from`: `docs` in `docs.x` is a selector of this config, \
         not an `extends:` alias; a qualified name is `<alias>.<name>`, with an alias an \
         `extends:` entry gives with `as:`"
    );
    assert_eq!(
        refused(vec![
            base(),
            root(&format!(
                "{aliased}relations:\n  j:\n    join: {{ left: base.manifests, right: base.deps, \
                 on: {{ name: name }}, unique: right }}\n    fields: {{ name: left.name }}\n"
            )),
        ]),
        ".alint.yml: `relations.j.join.left`: `base.manifests` is a selector; a join's `left:` \
         and `right:` name relations"
    );
    assert_eq!(
        refused(vec![
            base(),
            root(&format!(
                "{aliased}relations:\n  r:\n    from: {{ paths: x }}\n    read: {{ json: $.a }}\n    \
                 fields: {{ a: value }}\n    where:\n      a: {{ in: [{{ relation: missing, \
                 field: a }}] }}\n"
            )),
        ]),
        ".alint.yml: `relations.r.where.a.in[0].relation`: no relation named `missing` in this \
         config; the relations it can reach are `r`, `base.deps`"
    );
}

#[test]
fn an_alias_of_a_version_one_config_names_no_objects() {
    assert_eq!(
        refused(vec![root(&format!(
            "extends:\n  - url: ./plain.yml\n    as: plain\n{}",
            reading("plain.docs")
        ))]),
        ".alint.yml: `relations.titles.from`: `plain` names ./plain.yml, a version 1 config, \
         which declares no selectors or relations"
    );
}

#[test]
fn a_fragment_has_a_namespace_of_its_own() {
    let fragment = |from: &str| {
        source(
            ".alint.d/10-docs.yml",
            ConfigSourceClass::DropIn,
            &reading(from),
        )
    };
    let root = || root("selectors:\n  docs: { paths: docs }\n");
    assert_eq!(
        refused(vec![root(), fragment("docs")]),
        ".alint.d/10-docs.yml: `relations.titles.from`: no selector named `docs` in this config; \
         .alint.yml declares it, the root config: an `.alint.d` fragment has a namespace of its \
         own and cannot name its root's selectors and relations, so declare `docs` in this \
         fragment"
    );
    // Each resolves its own names: the fragment's `docs` is its own.
    let plan = plan(vec![
        root(),
        source(
            ".alint.d/10-docs.yml",
            ConfigSourceClass::DropIn,
            &format!("selectors:\n  docs: {{ paths: docs }}\n{}", reading("docs")),
        ),
    ]);
    assert_eq!(from_of(&plan, "titles"), reached(SelectorId(1), None));
    assert_eq!(plan.selectors[1].source, SourceIndex(1));
    assert_eq!(plan.relations[0].source, SourceIndex(1));
    // The root does not reach a fragment's objects either.
    assert_eq!(
        refused(vec![
            source(".alint.yml", ConfigSourceClass::Root, &reading("docs")),
            source(
                ".alint.d/10-docs.yml",
                ConfigSourceClass::DropIn,
                "selectors:\n  docs: { paths: docs }\n",
            ),
        ]),
        ".alint.yml: `relations.titles.from`: no selector named `docs` in this config; it \
         declares no selectors and reaches none through an `extends:` alias"
    );
}

#[test]
fn a_relation_has_the_capabilities_of_what_it_reads() {
    let plan = plan(vec![root(
        r"
selectors:
  manifests:
    paths: '**/package.json'
  packages:
    paths: '**/*.json'
    scope_filter: { include_manifest_paths: { source: workspace.json, extract: { json: '$.packages[*]' } } }
relations:
  plain:
    from: manifests
    read: { json: $.name }
    fields: { name: value }
  listed:
    from: packages
    read: { json: $.name }
    fields: { name: value }
  joined:
    join: { left: plain, right: listed, on: { name: name }, unique: right }
    fields: { name: left.name }
",
    )]);
    assert_eq!(plan.selectors[0].capabilities, Capabilities::default());
    assert_eq!(
        plan.selectors[1].capabilities,
        Capabilities {
            reads_content: true,
            host: Vec::new(),
        },
        "a manifest-derived predicate reads its manifest"
    );
    for relation in &plan.relations {
        assert!(relation.capabilities.reads_content, "{}", relation.name);
        assert!(relation.capabilities.host.is_empty(), "{}", relation.name);
    }
}

fn spawning() -> Capabilities {
    Capabilities {
        reads_content: true,
        host: vec![Capability::SpawnUserCommand],
    }
}

#[test]
fn capabilities_flow_from_what_a_relation_references() {
    // No selector of this build has a host capability, so the walk is tested
    // on a plan whose selector is given one.
    let mut plan = plan(vec![root(
        r"
selectors:
  s: { paths: x }
relations:
  a:
    from: s
    read: { json: $.a }
    fields: { a: value }
  b:
    from: { paths: y }
    read: { json: $.b }
    fields: { b: value }
    where:
      b: { in: [{ relation: a, field: a }] }
  c:
    join: { left: b, right: b, on: { b: b }, unique: right }
    fields: { b: left.b }
",
    )]);
    plan.selectors[0].capabilities.host = vec![Capability::Git {
        operations: std::borrow::Cow::Borrowed(&[GitOperation::Diff]),
    }];
    let order = super::super::plan_check::check(&plan).expect("checks");
    add_capabilities(&mut plan, &order);
    for relation in &plan.relations {
        assert_eq!(
            relation.capabilities.host,
            vec![Capability::Git {
                operations: std::borrow::Cow::Borrowed(&[GitOperation::Diff]),
            }],
            "{}",
            relation.name
        );
    }
}

#[test]
fn an_extended_source_may_not_declare_an_object_that_acts_beyond_its_rules() {
    assert_eq!(
        refused_by(&spawning(), ConfigSourceClass::LocalExtend),
        Some("starts a process")
    );
    assert_eq!(refused_by(&spawning(), ConfigSourceClass::Root), None);
    assert_eq!(refused_by(&spawning(), ConfigSourceClass::DropIn), None);
    let reads = |capability: Capability| Capabilities {
        reads_content: true,
        host: vec![capability],
    };
    assert_eq!(
        refused_by(
            &reads(Capability::OutOfRootRead),
            ConfigSourceClass::LocalExtend
        ),
        Some("reads outside the repository")
    );
    assert_eq!(
        refused_by(
            &reads(Capability::WritesDuringCheck),
            ConfigSourceClass::LocalExtend
        ),
        Some("writes files during a check")
    );
    assert_eq!(
        refused_by(
            &reads(Capability::Git {
                operations: std::borrow::Cow::Borrowed(&[GitOperation::Diff]),
            }),
            ConfigSourceClass::LocalExtend
        ),
        None,
        "an extended source's rules may read Git, so its objects may too"
    );

    let mut plan = plan(vec![extended("base.yml", BASE)]);
    gate_objects(&plan).expect("a plain object passes");
    plan.relations[0].capabilities = spawning();
    assert_eq!(
        gate_objects(&plan).unwrap_err().to_string(),
        "base.yml: `relations.deps`: this relation starts a process (itself or through what it \
         references), which a config reached through `extends:` may not do; declare it in the \
         top-level config or an `.alint.d` fragment"
    );
    plan.selectors[0].capabilities = spawning();
    assert_eq!(
        gate_objects(&plan).unwrap_err().to_string(),
        "base.yml: `selectors.manifests`: this selector starts a process (itself or through what \
         it references), which a config reached through `extends:` may not do; declare it in \
         the top-level config or an `.alint.d` fragment"
    );
}

fn refused_by(capabilities: &Capabilities, class: ConfigSourceClass) -> Option<&'static str> {
    super::refused(capabilities, class)
}

fn stub_rule(yaml: &str) -> ayaml::Mapping {
    alint_core::yaml::from_str::<Value>(yaml)
        .expect("parse")
        .as_mapping()
        .expect("a mapping")
        .clone()
}

/// The files the configuration of these tests was loaded from.
fn files() -> Vec<PathBuf> {
    vec![file("base.yml"), file(".alint.yml")]
}

/// `field` written by the source `display`, by its index among [`files`].
fn origin(field: &str, display: &str) -> (String, usize) {
    let index = files().iter().position(|held| *held == file(display));
    (field.to_string(), index.expect("one of the files"))
}

#[test]
fn a_rule_field_resolves_in_the_source_that_wrote_it() {
    let objects = Objects {
        sources: vec![
            extended("base.yml", BASE),
            root(&format!(
                "extends:\n  - url: ./base.yml\n    as: base\n{}",
                reading("{ paths: x }")
            )),
        ],
    };
    let mut resolved = resolve(&objects).expect("resolves");
    let rule = stub_rule(&format!("id: r\nkind: {REFERENCE_STUB}\ninput: deps\n"));
    // `input: deps` written by base.yml names base's own relation ...
    resolved
        .resolve_rule("r", &rule, &[origin("input", "base.yml")], &files())
        .expect("resolves in base.yml");
    // ... and the same text written by the root names nothing there.
    assert_eq!(
        resolved
            .resolve_rule("r2", &rule, &[origin("input", ".alint.yml")], &files())
            .unwrap_err()
            .to_string(),
        "rule \"r2\": `input` (written by .alint.yml): no relation named `deps` in this config; \
         base.yml declares it, which this config extends as `base`: write `base.deps` (an \
         extended config's objects are reached by a qualified name)"
    );
    let rule = stub_rule(&format!(
        "id: r3\nkind: {REFERENCE_STUB}\ninput: base.deps\n"
    ));
    resolved
        .resolve_rule("r3", &rule, &[origin("input", ".alint.yml")], &files())
        .expect("resolves through the alias");
    assert_eq!(
        resolved.plan.rules,
        vec![
            RuleReferences {
                rule: "r".into(),
                references: vec![RuleReference {
                    field: "input".into(),
                    source: SourceIndex(0),
                    relation: reached(RelationId(0), None),
                }],
            },
            RuleReferences {
                rule: "r3".into(),
                references: vec![RuleReference {
                    field: "input".into(),
                    source: SourceIndex(1),
                    relation: reached(RelationId(0), Some("base")),
                }],
            },
        ]
    );
}

#[test]
fn a_rule_field_no_version_two_source_wrote_names_nothing() {
    let objects = Objects {
        sources: vec![root(&reading("{ paths: x }"))],
    };
    let mut resolved = resolve(&objects).expect("resolves");
    let rule = stub_rule(&format!("id: r\nkind: {REFERENCE_STUB}\ninput: titles\n"));
    assert_eq!(
        resolved
            .resolve_rule("r", &rule, &[], &files())
            .unwrap_err()
            .to_string(),
        "rule \"r\": `input` names a relation, but a version 1 config wrote it; only a version 2 \
         config's rules reference selectors and relations"
    );
    let rule = stub_rule(&format!("id: r\nkind: {REFERENCE_STUB}\ninput: [titles]\n"));
    assert_eq!(
        resolved
            .resolve_rule("r", &rule, &[origin("input", ".alint.yml")], &files())
            .unwrap_err()
            .to_string(),
        "rule \"r\": `input` (written by .alint.yml): expected a relation name, found a list"
    );
    let rule = stub_rule(&format!("id: r\nkind: {REFERENCE_STUB}\ninput: a.b.c\n"));
    assert_eq!(
        resolved
            .resolve_rule("r", &rule, &[origin("input", ".alint.yml")], &files())
            .unwrap_err()
            .to_string(),
        "rule \"r\": `input` (written by .alint.yml): `a.b.c` is not a valid reference: write a \
         name, or `alias.name` for an object of a source this config extends with `as: alias`; \
         a name starts with an ASCII letter or `_` and holds only ASCII letters, digits, `_` \
         and `-` (there is no chained name such as `a.b.c`)"
    );
    // A kind without reference fields records nothing.
    resolved
        .resolve_rule(
            "plain",
            &stub_rule("id: plain\nkind: file_exists\ninput: x\n"),
            &[],
            &files(),
        )
        .expect("no reference field");
    assert_eq!(resolved.plan.rules, Vec::new());
}

#[test]
fn a_rule_an_extended_source_shapes_is_judged_with_what_it_references() {
    let objects = Objects {
        sources: vec![root(&reading("{ paths: x }"))],
    };
    let mut resolved = resolve(&objects).expect("resolves");
    let rule = stub_rule(&format!("id: r\nkind: {REFERENCE_STUB}\ninput: titles\n"));
    resolved
        .resolve_rule("r", &rule, &[origin("input", ".alint.yml")], &files())
        .expect("resolves");
    resolved
        .judge_rule("r", Some("base.yml"))
        .expect("reading files is allowed");
    resolved.plan.relations[0].capabilities = spawning();
    resolved.judge_rule("r", None).expect("the user's own rule");
    assert_eq!(
        resolved
            .judge_rule("r", Some("base.yml"))
            .unwrap_err()
            .to_string(),
        "rule \"r\": a relation this rule references starts a process, and an extended config \
         (base.yml) shapes the rule; such a rule must be declared entirely in your own top-level \
         config"
    );
}

use super::super::origins::REFERENCE_STUB_KIND as REFERENCE_STUB;

/// A suggestion for a name nothing declares weighs it against every name it
/// could mean, each only near the diagonal of its edit distance and not at
/// all past a length two edits cannot reach: work linear in the names, not
/// the product of two long ones a hostile config can write. What it reads
/// counts as well as what it computes: reading the unknown name once per name
/// it was weighed against, or an alias once per name it prefixes, computed no
/// cell and was quadratic.
#[test]
fn a_suggestion_does_work_linear_in_the_names_it_weighs() {
    let weighed = |wanted: &str, offered: &Offered<'_>| {
        crate::interp::DISTANCE_CELLS.with(|cells| cells.set(0));
        crate::interp::DISTANCE_CHARS.with(|read| read.set(0));
        let message = suggest(wanted, offered, "its names are", "names");
        (
            message,
            crate::interp::DISTANCE_CELLS.with(std::cell::Cell::get),
            crate::interp::DISTANCE_CHARS.with(std::cell::Cell::get),
        )
    };
    let stem = "n".repeat(398);
    let mut names: Vec<String> = (0..64).map(|index| format!("{stem}{index:02}")).collect();
    names[40] = format!("{stem}nx");
    let offered = Offered::bare(names.iter().map(String::as_str).collect());
    let (message, cells, read) = weighed(&format!("{stem}nn"), &offered);
    assert_eq!(message, format!("did you mean `{stem}nx`?"));
    // At most five cells a character of each name, and each character read
    // once: the unknown name's and each name's.
    assert!(cells <= names.len() * 5 * 400, "{cells} cells");
    assert!(read <= 400 + names.len() * 400, "{read} characters");

    // Many short names and one long unknown one, as a hostile config writes
    // them: the unknown name is read once, and no name at all.
    let long = format!("z{}", "y".repeat(100_000));
    let short: Vec<String> = (0..2_000).map(|index| format!("s{index}")).collect();
    let offered = Offered::bare(short.iter().map(String::as_str).collect());
    let (message, cells, read) = weighed(&long, &offered);
    assert_eq!(
        message,
        format!("there are 2000 names, none named like `{long}`")
    );
    assert_eq!((cells, read), (0, long.len()));

    // The same names through a long alias: the alias is read once for all of
    // them, and each name after it once.
    let alias = "a".repeat(100_000);
    let mut offered = Offered::default();
    offered.push(Some(&alias), short.iter().map(String::as_str).collect());
    let wanted = format!("{alias}.s1x");
    let (message, cells, read) = weighed(&wanted, &offered);
    assert_eq!(message, format!("did you mean `{alias}.s1`?"));
    let names_read: usize = short.iter().map(String::len).sum();
    assert!(
        read <= wanted.len() + alias.len() + 1 + names_read,
        "{read} characters"
    );
    assert!(cells <= 5 * read, "{cells} cells");
    // A long unknown name against the long alias: neither is read past the
    // point two edits cannot come back from.
    let (message, _, read) = weighed(&long, &offered);
    assert_eq!(
        message,
        format!("there are 2000 names, none named like `{long}`")
    );
    assert!(read <= long.len() + 3, "{read} characters");
}

/// The nearest name, weighed a group at a time with each alias read once, is
/// the one the full edit distance of each written name finds: the first of
/// the closest within two edits.
#[test]
fn the_nearest_name_through_aliases_is_the_nearest_written_one() {
    let words = [
        "", "a", "b", "ab", "ba", "abc", "a.b", "b.a", "abab", "é", "aé", "éb",
    ];
    let aliases = [None, Some("a"), Some("b"), Some("ab"), Some("é")];
    for wanted in words {
        for (first, second) in aliases
            .iter()
            .flat_map(|a| aliases.iter().map(move |b| (*a, *b)))
        {
            let names: Vec<&str> = words
                .iter()
                .copied()
                .filter(|name| !name.is_empty())
                .collect();
            let mut offered = Offered::default();
            offered.push(first, names[..5].to_vec());
            offered.push(second, names[5..].to_vec());
            let written: Vec<String> = [(first, &names[..5]), (second, &names[5..])]
                .into_iter()
                .flat_map(|(alias, group)| {
                    group.iter().map(move |name| {
                        alias.map_or_else(|| (*name).to_string(), |a| format!("{a}.{name}"))
                    })
                })
                .collect();
            let expected = written
                .iter()
                .map(|name| (crate::interp::levenshtein(wanted, name), name))
                .filter(|(distance, _)| *distance <= 2)
                .min_by_key(|(distance, _)| *distance)
                .map(|(_, name)| name.clone());
            assert_eq!(
                nearest(wanted, &offered),
                expected,
                "{wanted:?} {first:?} {second:?}"
            );
        }
    }
}

/// Recording where each `extends:` entry of a source leads finds the entry's
/// alias by its index: a source of many aliases records them all looking at
/// a logarithm of them per entry, not each of them per entry (80,000 aliased
/// entries once took a minute here).
#[test]
fn recording_many_aliases_looks_at_few_of_them() {
    use super::super::model::{ALIAS_PROBES, Alias};

    const ENTRIES: usize = 20_000;
    let target = |entry: usize| PathBuf::from(format!("/repo/p{}.yml", entry % 7));
    // Every other entry has an alias.
    let aliases = (0..ENTRIES)
        .step_by(2)
        .map(|entry| {
            let alias = Alias {
                url: "./p.yml".into(),
                entry,
                target: None,
            };
            (format!("a{entry}"), alias)
        })
        .collect();
    let mut objects = Objects {
        sources: vec![SourceObjects {
            source: SourceId {
                display: ".alint.yml".into(),
                path: file(".alint.yml"),
                class: ConfigSourceClass::Root,
            },
            selectors: Vec::new(),
            relations: Vec::new(),
            aliases,
            extended: Vec::new(),
        }],
    };
    ALIAS_PROBES.with(|probes| probes.set(0));
    for entry in 0..ENTRIES {
        objects.record_extended(entry, Some(&target(entry)));
    }
    let probes = ALIAS_PROBES.with(std::cell::Cell::get);
    // A binary search of 10,000 aliases looks at about 14 of them; looking at
    // each would be 10,000.
    assert!(probes <= ENTRIES * 20, "{probes} aliases looked at");
    let source = &objects.sources[0];
    assert_eq!(source.extended.len(), ENTRIES);
    for (name, alias) in &source.aliases {
        assert_eq!(name, &format!("a{}", alias.entry));
        assert_eq!(alias.target, Some(target(alias.entry)), "{name}");
    }
}

/// A bare name nothing declares is weighed against each config the source
/// reaches through an alias once, under its first alias, and the count says
/// how many objects there are: 1,000 aliases of one config of 1,000
/// selectors are 1,000 selectors, not a million pairs.
#[test]
fn an_unknown_name_weighs_each_aliased_config_once() {
    let names = (0..1_000)
        .map(|index| format!("  s{index}: {{ paths: x }}\n"))
        .collect::<Vec<_>>()
        .concat();
    let base = extended("base.yml", &format!("selectors:\n{names}"));
    let entries = (0..1_000)
        .map(|index| format!("  - {{ url: ./base.yml, as: b{index} }}\n"))
        .collect::<Vec<_>>()
        .concat();
    let consumer = root(&format!(
        "extends:\n{entries}selectors:\n  own: {{ paths: y }}\nrelations:\n  r: {{ from: nope, \
         read: {{ whole_file: {{}} }}, fields: {{ f: value }} }}\n"
    ));
    assert_eq!(
        refused(vec![base.clone(), consumer]),
        ".alint.yml: `relations.r.from`: no selector named `nope` in this config; there are \
         1001 selectors it can reach, none named like `nope`"
    );
    // A name the aliased config declares is named through the first alias.
    let declared = root(&format!(
        "extends:\n{entries}relations:\n  r: {{ from: s7, read: {{ whole_file: {{}} }}, \
         fields: {{ f: value }} }}\n"
    ));
    let error = refused(vec![base, declared]);
    assert!(error.contains("write `b0.s7`"), "{error}");
}
