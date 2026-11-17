//! The evaluator over hand-built plans: limits through the test seam, the
//! read confinement, and the run memo. The behaviour a configuration
//! reaches is tested through the loader in `alint-dsl`
//! (`src/tests/version_two_rows.rs`), which writes plans as users do.

use std::path::PathBuf;

use super::*;
use crate::relation::limits::LimitKind;
use crate::relation::plan::{
    Entries, EntriesFormat, FieldDef, FieldSource, JoinDef, JoinSide, Member, NamedRelation, Part,
    PlanSource, Predicate, Reached, Read, Reader, RelationDef, Rows, SelectorDef, SelectorRef,
    SourceIndex,
};
use crate::{ConfigSourceClass, PathsSpec};

fn plan(relations: Vec<NamedRelation>) -> Arc<Plan> {
    Arc::new(Plan {
        sources: vec![PlanSource {
            display: ".alint.yml".into(),
            path: PathBuf::from(".alint.yml"),
            class: ConfigSourceClass::Root,
            aliases: Vec::new(),
        }],
        selectors: Vec::new(),
        relations,
        rules: Vec::new(),
    })
}

fn named(name: &str, rows: Rows, fields: &[(&str, FieldSource)]) -> NamedRelation {
    NamedRelation {
        name: name.into(),
        source: SourceIndex(0),
        relation: RelationDef {
            rows,
            fields: fields
                .iter()
                .map(|(name, source)| {
                    (
                        (*name).to_string(),
                        FieldDef {
                            source: source.clone(),
                            sensitive: false,
                        },
                    )
                })
                .collect(),
            transforms: Vec::new(),
            filters: Vec::new(),
        },
        capabilities: crate::relation::plan::Capabilities::default(),
        reads_environment: false,
    }
}

fn from(paths: &str) -> SelectorRef {
    SelectorRef::Inline(Box::new(SelectorDef {
        paths: PathsSpec::Single(paths.into()),
        scope_filter: None,
    }))
}

fn entries(paths: &str, at: &str) -> Rows {
    Rows::Part(Part {
        from: from(paths),
        reader: Reader::Entries(Entries {
            format: EntriesFormat::Json,
            at: vec![at.into()],
            leaves: false,
        }),
    })
}

fn read_json(paths: &str, query: &str) -> Rows {
    Rows::Part(Part {
        from: from(paths),
        reader: Reader::Read {
            read: Read::Json(query.into()),
            optional: false,
        },
    })
}

fn reached(id: usize) -> Reached<RelationId> {
    Reached {
        id: RelationId(id),
        alias: None,
    }
}

/// A tree of `files` under a temporary root, walked.
struct Tree {
    dir: tempfile::TempDir,
    index: FileIndex,
}

fn tree(files: &[(&str, &str)]) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    for (path, body) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let index = crate::walker::walk(dir.path(), &crate::walker::WalkOptions::default()).unwrap();
    Tree { dir, index }
}

fn run(plan: &Arc<Plan>, id: usize, tree: &Tree, limits: Limits) -> Outcome {
    let decodes = DecodeCache::with_run_limits(&limits);
    evaluate_with(
        plan,
        RelationId(id),
        tree.dir.path(),
        &tree.index,
        limits,
        &ReadCache::new(),
        &decodes,
        &RunMemo::new(Arc::clone(plan)),
    )
}

fn messages(outcome: &Outcome) -> Vec<String> {
    outcome
        .as_ref()
        .expect_err("the relation fails")
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn limit(kind: LimitKind, value: usize) -> Limits {
    Limits::default().with(kind, value).unwrap()
}

fn deps() -> NamedRelation {
    named(
        "deps",
        entries("*.json", "$.deps"),
        &[("name", FieldSource::EntryKey)],
    )
}

#[test]
fn rows_per_extractor_is_an_analysis_failure() {
    let plan = plan(vec![deps()]);
    let tree = tree(&[("a.json", r#"{"deps": {"x": 1, "y": 2}}"#)]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::RowsPerExtractor, 2))),
        [
            "a.json: relation `deps` (.alint.yml): relation limit `rows_per_extractor` (2) reached: \
          3 rows or values, so the relation is not evaluated rather than cut short; narrow what \
          it reads (its selector, or its `at:` paths)"
        ]
    );
    assert!(run(&plan, 0, &tree, limit(LimitKind::RowsPerExtractor, 3)).is_ok());
}

#[test]
fn rows_per_run_is_an_analysis_failure() {
    let plan = plan(vec![deps()]);
    let tree = tree(&[
        ("a.json", r#"{"deps": {"x": 1}}"#),
        ("b.json", r#"{"deps": {"y": 2}}"#),
    ]);
    // Each decode emits one row; the second takes the run past one.
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::RowsPerRun, 1))),
        [
            "b.json: relation `deps` (.alint.yml): relation limit `rows_per_run` (1) reached: 2 rows \
          or values, so the relation is not evaluated rather than cut short; narrow what it reads \
          (its selector, or its `at:` paths)"
        ]
    );
}

fn names() -> NamedRelation {
    named(
        "names",
        read_json("*.json", "$.name"),
        &[("name", FieldSource::Value)],
    )
}

#[test]
fn a_semi_join_set_is_bounded_by_the_distinct_value_limit() {
    let mut filtered = deps();
    filtered.relation.filters = vec![(
        "name".into(),
        vec![Predicate::NotIn(vec![Member::Relation {
            relation: reached(1),
            field: "name".into(),
        }])],
    )];
    let plan = plan(vec![filtered, names()]);
    let tree = tree(&[
        ("a.json", r#"{"name": "a", "deps": {"b": 1}}"#),
        ("b.json", r#"{"name": "b", "deps": {}}"#),
    ]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::DistinctValues, 1))),
        [
            "relation `deps` (.alint.yml): `where.name.not_in` reads `names.name`: relation limit \
          `distinct_values` (1) reached: 2 rows or values, so the relation is not evaluated \
          rather than cut short; narrow `names`"
        ]
    );
    let rows = run(&plan, 0, &tree, limit(LimitKind::DistinctValues, 2)).unwrap();
    assert!(rows.rows.is_empty(), "`b` is a workspace name");
}

fn joined() -> NamedRelation {
    named(
        "joined",
        Rows::Join(JoinDef {
            left: reached(1),
            right: reached(2),
            on: vec![("name".into(), "name".into())],
            unique: JoinSide::Right,
        }),
        &[
            (
                "dependency",
                FieldSource::Side(JoinSide::Left, "name".into()),
            ),
            (
                "manifest",
                FieldSource::Side(JoinSide::Right, "name".into()),
            ),
        ],
    )
}

#[test]
fn the_join_limits_bound_each_side_and_the_output() {
    let plan = plan(vec![joined(), deps(), names()]);
    let tree = tree(&[
        ("a.json", r#"{"name": "a", "deps": {"b": 1, "a": 1}}"#),
        ("b.json", r#"{"name": "b", "deps": {"a": 1}}"#),
    ]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::JoinInputs, 2))),
        [
            "relation `joined` (.alint.yml): the join's left side (`deps`): relation limit \
          `join_inputs` (2) reached: 3 rows or values, so the relation is not evaluated rather \
          than cut short; narrow that side"
        ]
    );
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::JoinOutputs, 2))),
        [
            "relation `joined` (.alint.yml): relation limit `join_outputs` (2) reached: 3 rows or \
          values, so the relation is not evaluated rather than cut short; narrow its sides"
        ]
    );
    let rows = run(&plan, 0, &tree, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 3);
}

#[test]
fn reads_never_reach_a_git_directory() {
    let plan = plan(vec![named(
        "configs",
        read_json("**/*.json", "$.name"),
        &[("name", FieldSource::Value)],
    )]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git/config.json"), r#"{"name": "secret"}"#).unwrap();
    std::fs::write(dir.path().join("ok.json"), r#"{"name": "ok"}"#).unwrap();
    let mut entries = vec![(".git/config.json", false), ("ok.json", false)];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(".git/config.json", dir.path().join("link.json")).unwrap();
        entries.push(("link.json", false));
    }
    // An index built from entries vouches for nothing, so every read is
    // confined as a literal path's is.
    let index = FileIndex::from_entries(
        entries
            .iter()
            .map(|(path, is_dir)| crate::walker::FileEntry {
                path: Arc::from(std::path::Path::new(path)),
                is_dir: *is_dir,
                size: 20,
            })
            .collect(),
    );
    let tree = Tree { dir, index };
    let mut expected = vec![
        ".git/config.json: relation `configs` (.alint.yml): could not read file: path is in a Git \
         directory; rules never read there"
            .to_string(),
    ];
    #[cfg(unix)]
    expected.push(
        "link.json: relation `configs` (.alint.yml): could not read file: path is in a Git \
         directory; rules never read there"
            .to_string(),
    );
    assert_eq!(messages(&run(&plan, 0, &tree, Limits::default())), expected);

    // The walk leaves both out, so a walked tree reads only `ok.json`.
    let walked = Tree {
        index: crate::walker::walk(tree.dir.path(), &crate::walker::WalkOptions::default())
            .unwrap(),
        dir: tree.dir,
    };
    let rows = run(&plan, 0, &walked, Limits::default()).unwrap();
    let read: Vec<&str> = rows.resources.iter().map(|r| r.path().as_str()).collect();
    assert_eq!(read, ["ok.json"]);
}

/// The walk vouches for nothing at read time: a file swapped after it for a
/// symlink into `.git` or for a FIFO is refused, without blocking, as
/// `cross_file` refuses one its glob matched.
#[cfg(unix)]
#[test]
fn a_file_swapped_after_the_walk_is_confined_and_never_blocks() {
    let plan = plan(vec![named(
        "configs",
        read_json("*.json", "$.name"),
        &[("name", FieldSource::Value)],
    )]);
    let tree = tree(&[
        ("a.json", r#"{"name": "a"}"#),
        ("b.json", r#"{"name": "b"}"#),
        ("c.json", r#"{"name": "c"}"#),
        (".git/config.json", r#"{"name": "secret"}"#),
    ]);
    let root = tree.dir.path();
    std::fs::remove_file(root.join("a.json")).unwrap();
    std::os::unix::fs::symlink(".git/config.json", root.join("a.json")).unwrap();
    std::fs::remove_file(root.join("b.json")).unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(root.join("b.json"))
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = run(&plan, 0, &tree, Limits::default());
        let _ = sender.send((messages(&outcome), tree));
    });
    let (messages, _tree) = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a FIFO must be refused before it can block the read");
    assert_eq!(
        messages,
        [
            "a.json: relation `configs` (.alint.yml): could not read file: path is in a Git \
             directory; rules never read there",
            "b.json: relation `configs` (.alint.yml): could not read file: not a regular file",
        ]
    );
}

#[test]
fn a_relation_is_evaluated_once_per_run() {
    let plan = plan(vec![deps(), names()]);
    let tree = tree(&[("a.json", r#"{"name": "a", "deps": {"b": 1}}"#)]);
    let ctx = Context::new(tree.dir.path(), &tree.index);
    assert!(!tree.index.relation_memo_is_set());
    let first = evaluate(&plan, RelationId(0), &ctx).unwrap();
    let again = evaluate(&plan, RelationId(0), &ctx).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    assert!(tree.index.relation_memo_is_set());
    let decodes = tree.index.decode_cache().unwrap().decodes();
    assert_eq!(decodes, 1, "`names` was not read");
    // Inside a nested rule's iteration the memo and caches are its own.
    let inner = {
        let _scope = crate::relation::IterationScope::enter();
        evaluate(&plan, RelationId(0), &ctx).unwrap()
    };
    assert!(!Arc::ptr_eq(&first, &inner));
    assert_eq!(first.rows, inner.rows);
    assert_eq!(tree.index.decode_cache().unwrap().decodes(), 1);
    // Another plan gets a memo of its own.
    let other = Arc::new((*plan).clone());
    assert!(evaluate(&other, RelationId(0), &ctx).is_ok());
    // A fix that changes files forgets the memo with the caches.
    tree.index.clear_relation_caches();
    assert!(!tree.index.relation_memo_is_set());
}

#[test]
fn a_failed_side_fails_the_join_with_its_failures() {
    let plan = plan(vec![joined(), deps(), names()]);
    let tree = tree(&[("a.json", r#"{"name": 1, "deps": {"b": 1}}"#)]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, Limits::default())),
        [
            "a.json: relation `names` (.alint.yml): `read.json: $.name` matched non-string value(s) \
          (number: 1); `read:` rows hold strings, so select string values, or read typed values \
          with `entries:`"
        ]
    );
}

#[test]
fn failures_convert_to_analysis_failures_and_redact_when_sensitive() {
    let failure = RelationFailure {
        kind: crate::rule::AnalysisFailureKind::Evaluation,
        resource: Some(Arc::from(std::path::Path::new("a.json"))),
        message: "relation `deps` (.alint.yml): detail".into(),
        sensitive: false,
    };
    let violation = failure.to_violation("rule");
    assert_eq!(
        violation.message.as_ref(),
        "relation `deps` (.alint.yml): detail"
    );
    assert_eq!(
        violation.path.as_deref(),
        Some(std::path::Path::new("a.json"))
    );
    assert_eq!(
        violation.analysis_failure_kind(),
        Some(crate::rule::AnalysisFailureKind::Evaluation)
    );
    let hidden = RelationFailure {
        sensitive: true,
        ..failure
    }
    .to_violation("rule");
    assert!(
        hidden.message.starts_with("sensitive evidence "),
        "{}",
        hidden.message
    );
}
