//! The engine evaluates the relations the rules about to run read before
//! any rule runs (`Engine::evaluate_read_relations`), and only those: a
//! relation no rule reads, or only a rule whose `when:` is false, is never
//! evaluated.

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
                }),
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
            path: std::path::PathBuf::from(".alint.yml"),
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
