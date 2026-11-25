//! The engine evaluates the relations the rules about to run read before
//! any rule runs (`Engine::evaluate_read_relations`), and only those: a
//! relation no rule reads, or only a rule whose `when:` is false, is never
//! evaluated. Under `--changed`, a rule's trigger scope reads the manifests
//! its selectors consult only when the change can run the rule.

use std::sync::{Arc, Mutex};

use super::*;
use crate::relation::plan::{
    Capabilities, Entries, EntriesFormat, FieldDef, FieldSource, NamedRelation, Part, Plan,
    PlanSource, Reader, RelationDef, RelationId, Rows, SelectorDef, SelectorRef, SourceIndex,
};
use crate::{ConfigSourceClass, PathsSpec};

/// A relation of the entries of `file`'s `$.deps`.
fn relation(name: &str, file: &str) -> NamedRelation {
    NamedRelation {
        name: name.into(),
        source: SourceIndex(0),
        relation: RelationDef {
            rows: Rows::Part(Part {
                from: SelectorRef::Inline(Box::new(SelectorDef {
                    paths: PathsSpec::Single(file.into()),
                    scope_filter: None,
                })),
                reader: Reader::Entries(Entries {
                    format: EntriesFormat::Json,
                    at: vec!["$.deps".into()],
                    leaves: false,
                    strict: false,
                }),
                fields: None,
            }),
            fields: vec![(
                "name".to_string(),
                FieldDef {
                    source: FieldSource::EntryKey,
                    sensitive: false,
                },
            )],
            transforms: Vec::new(),
            filters: Vec::new(),
        },
        capabilities: Capabilities::default(),
        reads_environment: false,
        definition_reads_environment: false,
    }
}

/// Reads relation `input` of `plan`, and records which of the plan's
/// relations the run had evaluated when it ran.
#[derive(Debug)]
struct ReadsRelation {
    id: &'static str,
    level: Level,
    plan: Arc<Plan>,
    input: [RelationId; 1],
    seen: Seen,
    /// What runs it under `--changed`, when it has a trigger scope.
    trigger: Option<crate::relation::TriggerScope>,
}

/// Each rule's id, with which relations of the plan were evaluated when it
/// ran.
type Seen = Arc<Mutex<Vec<(&'static str, Vec<bool>)>>>;

impl Rule for ReadsRelation {
    fn id(&self) -> &str {
        self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn uses_relations(&self) -> bool {
        true
    }
    fn requires_full_index(&self) -> bool {
        true
    }
    fn relation_inputs(&self) -> Option<(&Arc<Plan>, &[RelationId])> {
        Some((&self.plan, &self.input))
    }
    fn trigger_scope(&self) -> Option<&crate::relation::TriggerScope> {
        self.trigger.as_ref()
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let memo = ctx
            .index
            .relation_memo(&self.plan)
            .expect("the run's memo for the plan");
        let evaluated = (0..self.plan.relations.len())
            .map(|id| memo.evaluated(RelationId(id)))
            .collect();
        self.seen.lock().unwrap().push((self.id, evaluated));
        Ok(Vec::new())
    }
}

#[test]
fn the_relations_rules_read_are_evaluated_before_any_rule_runs_and_no_others() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c", "d"] {
        std::fs::write(
            dir.path().join(format!("{name}.json")),
            r#"{"deps": {"x": "1"}}"#,
        )
        .unwrap();
    }
    let index =
        crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
    let plan = Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            locator: crate::relation::plan::SourceLocator::Local(std::path::PathBuf::from(
                ".alint.yml",
            )),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![
            relation("read-last", "a.json"),
            relation("unread", "b.json"),
            relation("read-by-a-rule-that-is-off", "c.json"),
            relation("read-under-a-false-when", "d.json"),
        ],
        rules: Vec::new(),
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let rule = |id, level, input| ReadsRelation {
        id,
        level,
        plan: Arc::clone(&plan),
        input: [RelationId(input)],
        seen: Arc::clone(&seen),
        trigger: None,
    };
    let entries = vec![
        RuleEntry::new(Box::new(rule("reads-0", Level::Error, 0))),
        RuleEntry::new(Box::new(rule("off", Level::Off, 2))),
        RuleEntry::new(Box::new(rule("false-when", Level::Error, 3)))
            .with_when(crate::when::parse("false").unwrap()),
    ];
    let engine = Engine::from_entries(entries, RuleRegistry::new());
    engine.run(dir.path(), &index).unwrap();
    // The engine runs every entry it is given whose `when:` holds, one that
    // is off included (the CLI builds none), so each saw both relations the
    // run's rules read, whichever ran first.
    let mut seen = seen.lock().unwrap().clone();
    seen.sort_unstable();
    assert_eq!(
        seen,
        [
            ("off", vec![true, false, true, false]),
            ("reads-0", vec![true, false, true, false]),
        ]
    );
}

#[test]
fn a_trigger_scope_reads_no_manifest_when_the_change_cannot_run_its_rule() {
    // A selector of `workspace/**/member.json` narrowed to the members a
    // manifest lists, and the manifest malformed: reading it caches a
    // failure on the index.
    let mut members = relation("members", "workspace/**/member.json");
    let Rows::Part(part) = &mut members.relation.rows else {
        unreachable!("`relation` builds a part");
    };
    let SelectorRef::Inline(selector) = &mut part.from else {
        unreachable!("`relation` builds an inline selector");
    };
    selector.scope_filter = Some(
        crate::yaml::from_str(
            "include_manifest_paths:\n  source: members.json\n  \
             extract: { json: \"$.members[*]\" }\n",
        )
        .unwrap(),
    );
    let plan = Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            locator: crate::relation::plan::SourceLocator::Local(std::path::PathBuf::from(
                ".alint.yml",
            )),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![members],
        rules: Vec::new(),
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("workspace/w1")).unwrap();
    std::fs::write(dir.path().join("workspace/w1/member.json"), "{}").unwrap();
    std::fs::write(dir.path().join("members.json"), "{").unwrap();
    std::fs::write(dir.path().join("README.md"), "# repo\n").unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let failures_read = |changed: PathBuf| {
        let index =
            crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
        let rule = ReadsRelation {
            id: "members",
            level: Level::Error,
            plan: Arc::clone(&plan),
            input: [RelationId(0)],
            seen: Arc::clone(&seen),
            trigger: Some(crate::relation::TriggerScope::of(&plan, &[RelationId(0)])),
        };
        Engine::from_entries(vec![RuleEntry::new(Box::new(rule))], RuleRegistry::new())
            .with_changed_paths(HashSet::from([changed]))
            .run(dir.path(), &index)
            .unwrap();
        index.manifest_failures_map().map(HashMap::len)
    };
    // `README.md` is outside the selector's globs, so no manifest can bring
    // it in: the rule does not run, and the manifest is not read.
    assert_eq!(failures_read(PathBuf::from("README.md")), Some(0));
    assert!(seen.lock().unwrap().is_empty(), "the rule must not run");
    // A path the globs match runs the rule, and its manifest is read.
    let member = Path::new("workspace").join("w1").join("member.json");
    assert_eq!(failures_read(member), Some(1));
}

/// The run evaluates the relations its rules read in the order `explain`
/// shows them, [`Plan::evaluation_order`] of the ids they read: each after
/// those it reads.
#[test]
fn the_run_evaluates_relations_in_the_order_explain_shows() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(
            dir.path().join(format!("{name}.json")),
            r#"{"deps": {"x": "1"}}"#,
        )
        .unwrap();
    }
    let index =
        crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
    let side = |id| crate::relation::plan::Reached {
        id: RelationId(id),
        alias: None,
    };
    let mut joined = relation("joined", "a.json");
    joined.relation.rows = Rows::Join(crate::relation::plan::JoinDef {
        left: side(2),
        right: side(1),
        on: vec![("name".into(), "name".into())],
        unique: crate::relation::plan::JoinSide::Right,
    });
    joined.relation.fields[0].1.source =
        FieldSource::Side(crate::relation::plan::JoinSide::Left, "name".into());
    let plan = Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            locator: crate::relation::plan::SourceLocator::Local(std::path::PathBuf::from(
                ".alint.yml",
            )),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![
            joined,
            relation("names", "b.json"),
            relation("deps", "a.json"),
            relation("other", "c.json"),
        ],
        rules: Vec::new(),
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let rule = |id, input| ReadsRelation {
        id,
        level: Level::Error,
        plan: Arc::clone(&plan),
        input: [RelationId(input)],
        seen: Arc::clone(&seen),
        trigger: None,
    };
    let entries = vec![
        RuleEntry::new(Box::new(rule("reads-other", 3))),
        RuleEntry::new(Box::new(rule("reads-joined", 0))),
    ];
    Engine::from_entries(entries, RuleRegistry::new())
        .run(dir.path(), &index)
        .unwrap();
    let memo = index
        .relation_memo(&plan)
        .expect("the run's memo for the plan");
    let order = plan.evaluation_order(&[RelationId(3), RelationId(0)]);
    assert_eq!(memo.computed(), order);
    // Each after those it reads.
    assert_eq!(
        order,
        [RelationId(2), RelationId(1), RelationId(0), RelationId(3)]
    );
}

/// A plan of one relation over the entries of `locales/*.json`.
fn locales_plan() -> Arc<Plan> {
    Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            locator: crate::relation::plan::SourceLocator::Local(std::path::PathBuf::from(
                ".alint.yml",
            )),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations: vec![relation("locales", "locales/*.json")],
        rules: Vec::new(),
    })
}

/// A tree with `locales/en.json`, and `changed` (each path a file the tree
/// holds).
fn locales_tree(changed: &[PathBuf]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let locales = dir.path().join("locales");
    std::fs::create_dir_all(&locales).unwrap();
    std::fs::write(locales.join("en.json"), r#"{"deps": {"x": "1"}}"#).unwrap();
    for path in changed {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "changed\n").unwrap();
    }
    dir
}

/// A rule that `--changed` skips evaluates no relation before the rules
/// run: the memo records none (`Engine::evaluate_read_relations`).
#[test]
fn a_change_outside_the_trigger_scope_evaluates_no_relation() {
    let plan = locales_plan();
    let changed = PathBuf::from("README.md");
    let dir = locales_tree(std::slice::from_ref(&changed));
    let index =
        crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let rule = ReadsRelation {
        id: "locales",
        level: Level::Error,
        plan: Arc::clone(&plan),
        input: [RelationId(0)],
        seen: Arc::clone(&seen),
        trigger: Some(crate::relation::TriggerScope::of(&plan, &[RelationId(0)])),
    };
    let report = Engine::from_entries(vec![RuleEntry::new(Box::new(rule))], RuleRegistry::new())
        .with_changed_paths(HashSet::from([changed]))
        .run(dir.path(), &index)
        .unwrap();
    assert!(report.results.is_empty(), "{:?}", report.results);
    assert!(seen.lock().unwrap().is_empty(), "the rule must not run");
    let memo = index
        .relation_memo(&plan)
        .expect("the run's memo for the plan");
    assert_eq!(memo.computed(), Vec::<RelationId>::new());
}

/// Under `--changed`, a pass decides once what a rule's trigger scope takes
/// of the changed paths, and every gate of the pass reads that decision: a
/// selector tests each changed path below the directory its `paths:` start
/// with against its globs once a pass, however many gates ask (manifest
/// resolution, relation evaluation, dispatch; for `fix`, its own), and a
/// path outside that directory not at all.
#[test]
fn a_trigger_scope_tests_each_changed_path_against_its_globs_at_most_once_a_pass() {
    let plan = locales_plan();
    // Below `locales/`, but not what `locales/*.json` matches.
    let below: Vec<PathBuf> = (0..20)
        .map(|n| Path::new("locales").join("notes").join(format!("{n}.txt")))
        .collect();
    // Outside `locales/`.
    let outside: Vec<PathBuf> = (0..50)
        .map(|n| Path::new("src").join(format!("{n}.rs")))
        .collect();
    let changed: Vec<PathBuf> = below.iter().chain(&outside).cloned().collect();
    let dir = locales_tree(&changed);
    let seen = Arc::new(Mutex::new(Vec::new()));
    // The glob tests of one `check` run, and of one `fix` preview, with
    // `changed` changed.
    let glob_tests = |changed: &[PathBuf], fix: bool| {
        let index =
            crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
        let trigger = crate::relation::TriggerScope::of(&plan, &[RelationId(0)]);
        let rule = ReadsRelation {
            id: "locales",
            level: Level::Error,
            plan: Arc::clone(&plan),
            input: [RelationId(0)],
            seen: Arc::clone(&seen),
            trigger: Some(trigger.clone()),
        };
        let engine =
            Engine::from_entries(vec![RuleEntry::new(Box::new(rule))], RuleRegistry::new())
                .with_changed_paths(changed.iter().cloned().collect());
        if fix {
            engine
                .fix(
                    dir.path(),
                    &index,
                    &crate::WalkOptions::test_default(),
                    true,
                    crate::Applicability::Safe,
                )
                .unwrap();
        } else {
            engine.run(dir.path(), &index).unwrap();
        }
        trigger.glob_tests()
    };
    assert_eq!(glob_tests(&outside, false), 0);
    assert_eq!(glob_tests(&outside, true), 0);
    assert_eq!(glob_tests(&changed, false), below.len());
    assert_eq!(glob_tests(&changed, true), below.len());
    assert!(seen.lock().unwrap().is_empty(), "the rule must not run");
    // A change the selector takes runs the rule, still testing no path twice.
    let mut taken = changed.clone();
    taken.push(Path::new("locales").join("en.json"));
    assert!(glob_tests(&taken, false) <= taken.len() - outside.len());
    assert_eq!(seen.lock().unwrap().len(), 1, "the rule runs once");
}

/// `with_changed_paths` replaces the changed set whole: a directory an
/// earlier `with_changed_set` marked is a plain path after it, so a change
/// Git listed as a directory no longer runs the rules that read below it.
#[test]
fn with_changed_paths_forgets_the_directories_an_earlier_set_marked() {
    let plan = locales_plan();
    let dir = locales_tree(&[]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let locales = PathBuf::from("locales");
    let engine = || {
        let rule = ReadsRelation {
            id: "locales",
            level: Level::Error,
            plan: Arc::clone(&plan),
            input: [RelationId(0)],
            seen: Arc::clone(&seen),
            trigger: Some(crate::relation::TriggerScope::of(&plan, &[RelationId(0)])),
        };
        Engine::from_entries(vec![RuleEntry::new(Box::new(rule))], RuleRegistry::new())
            .with_changed_set(crate::git::ChangedSet::new(
                HashSet::from([locales.clone()]),
                HashSet::from([locales.clone()]),
            ))
    };
    let run = |engine: Engine| {
        let index =
            crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
        engine.run(dir.path(), &index).unwrap();
    };
    run(engine());
    assert_eq!(seen.lock().unwrap().len(), 1, "a directory runs the rule");
    run(engine().with_changed_paths(HashSet::from([locales.clone()])));
    assert_eq!(seen.lock().unwrap().len(), 1, "a plain path runs nothing");
}
