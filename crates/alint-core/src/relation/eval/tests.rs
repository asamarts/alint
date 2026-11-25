//! The evaluator over hand-built plans: limits through the test seam, the
//! read confinement, and the run memo. The behaviour a configuration
//! reaches is tested through the loader in `alint-dsl`
//! (`src/tests/version_two_rows.rs`), which writes plans as users do.

use std::path::PathBuf;

use super::*;
use crate::relation::limits::LimitKind;
use crate::relation::plan::{
    Entries, EntriesFormat, FieldDef, FieldSource, JoinDef, JoinSide, Member, NamedRelation, Part,
    PathToken, PlanSource, Predicate, Reached, Read, Reader, RelationDef, Rows, SelectorDef,
    SelectorRef, SourceIndex, Transform,
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
        definition_reads_environment: false,
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
    let index =
        crate::walker::walk(dir.path(), &crate::walker::WalkOptions::test_default()).unwrap();
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
    // The limit is each file's, not the run's: every file past it fails.
    let tree = self::tree(&[
        ("a.json", r#"{"deps": {"x": 1, "y": 2}}"#),
        ("b.json", r#"{"deps": {"x": 1, "y": 2}}"#),
    ]);
    let failed = messages(&run(&plan, 0, &tree, limit(LimitKind::RowsPerExtractor, 2)));
    let files: Vec<&str> = failed
        .iter()
        .map(|message| message.split_once(':').unwrap().0)
        .collect();
    assert_eq!(files, ["a.json", "b.json"], "{failed:?}");
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

/// A relation whose `where:` lists read two relations that fail gives the
/// failures of both: the first one's, shared until the second's arrive, and
/// then the second's beside them.
#[test]
fn a_where_that_reads_two_failing_relations_gives_both_their_failures() {
    let mut filtered = deps();
    let probe = |id: usize| {
        vec![Member::Relation {
            relation: reached(id),
            field: "name".into(),
        }]
    };
    filtered.relation.filters = vec![(
        "name".into(),
        vec![Predicate::NotIn(probe(1)), Predicate::In(probe(2))],
    )];
    let broken = |name: &str, paths: &str| {
        named(
            name,
            read_json(paths, "$.name"),
            &[("name", FieldSource::Value)],
        )
    };
    let plan = plan(vec![
        filtered,
        broken("first", "*.txt"),
        broken("second", "*.dat"),
    ]);
    let tree = tree(&[
        ("a.json", r#"{"deps": {"b": 1}}"#),
        ("b.txt", r#"{"name": "#),
        ("c.dat", r#"{"name": "#),
    ]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, Limits::default())),
        [
            "b.txt: relation `first` (.alint.yml): could not parse the file as JSON: EOF while \
             parsing a value at line 1 column 9",
            "c.dat: relation `second` (.alint.yml): could not parse the file as JSON: EOF while \
             parsing a value at line 1 column 9",
        ]
    );
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
    // Where the platform makes symlinks, one into `.git` joins the index.
    let linked: &[&str] = if cfg!(unix) { &["link.json"] } else { &[] };
    #[cfg(unix)]
    std::os::unix::fs::symlink(".git/config.json", dir.path().join("link.json")).unwrap();
    // An index built from entries vouches for nothing, so every read is
    // confined as a literal path's is.
    let index = FileIndex::from_entries(
        [".git/config.json", "ok.json"]
            .iter()
            .chain(linked)
            .map(|path| crate::walker::FileEntry {
                path: Arc::from(std::path::Path::new(path)),
                is_dir: false,
                size: 20,
            })
            .collect(),
    );
    let tree = Tree { dir, index };
    let expected: Vec<String> = [".git/config.json"]
        .iter()
        .chain(linked)
        .map(|path| {
            format!(
                "{path}: relation `configs` (.alint.yml): could not read file: path is in a Git \
                 directory; rules never read there"
            )
        })
        .collect();
    assert_eq!(messages(&run(&plan, 0, &tree, Limits::default())), expected);

    // The walk leaves both out, so a walked tree reads only `ok.json`.
    let walked = Tree {
        index: crate::walker::walk(tree.dir.path(), &crate::walker::WalkOptions::test_default())
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
    // The right side reads a file that does not parse. (A value that is not
    // a string is a note, not a failure.)
    let names = named(
        "names",
        read_json("*.txt", "$.name"),
        &[("name", FieldSource::Value)],
    );
    let plan = plan(vec![joined(), deps(), names]);
    let tree = tree(&[
        ("a.json", r#"{"deps": {"b": 1}}"#),
        ("b.txt", r#"{"name": "#),
    ]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, Limits::default())),
        [
            "b.txt: relation `names` (.alint.yml): could not parse the file as JSON: EOF while \
             parsing a value at line 1 column 9"
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

fn lines(paths: &str) -> Part {
    Part {
        from: from(paths),
        reader: Reader::Read {
            read: Read::Lines { comment: None },
            optional: false,
        },
    }
}

fn run_with_memo(
    plan: &Arc<Plan>,
    id: usize,
    tree: &Tree,
    limits: Limits,
    memo: &RunMemo,
) -> Outcome {
    evaluate_with(
        plan,
        RelationId(id),
        tree.dir.path(),
        &tree.index,
        limits,
        &ReadCache::new(),
        &DecodeCache::with_run_limits(&limits),
        memo,
    )
}

const CELLS_EXHAUSTED: &str = "so the relation is not evaluated rather than cut short; a run's \
    relations hold a cell per field of each row and one per file each part reads, so read fewer \
    rows (narrow the selector, the `at:` paths or the parts) or bind fewer fields";

/// A `union:` of identical parts decodes its file once, but each part's rows
/// take cells from the run as they are built: the relation fails at the cap,
/// not after building every part.
#[test]
fn a_unions_rows_take_cells_as_each_part_builds_them() {
    let body = (0..100)
        .map(|line| format!("l{line}\n"))
        .collect::<Vec<_>>()
        .concat();
    let tree = tree(&[("list.txt", &body)]);
    let parts = Rows::Union(vec![lines("list.txt"); 30]);
    let plan = plan(vec![named(
        "listed",
        parts,
        &[("line", FieldSource::Value)],
    )]);
    // Each part reads one file (a cell) and builds 100 one-field rows.
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::CellsPerRun, 150);
    assert_eq!(
        messages(&run_with_memo(&plan, 0, &tree, limits, &memo)),
        [format!(
            "list.txt: relation `listed` (.alint.yml): relation limit `cells_per_run` (150) \
             reached: 151 cells, {CELLS_EXHAUSTED}"
        )]
    );
    assert_eq!(memo.cells(), 151, "the union stopped at the cap");
    let all = 30 * 101;
    assert!(run(&plan, 0, &tree, limit(LimitKind::CellsPerRun, all - 1)).is_err());
    let rows = run(&plan, 0, &tree, limit(LimitKind::CellsPerRun, all)).unwrap();
    assert_eq!(rows.rows.len(), 30 * 100);
}

/// A row of F fields is F cells, and a join's rows take theirs as it builds
/// them; the run's relations share one count.
#[test]
fn every_field_of_every_row_takes_a_cell_and_so_do_a_joins_rows() {
    let plan = plan(vec![joined(), deps(), names()]);
    let tree = tree(&[
        ("a.json", r#"{"name": "a", "deps": {"b": 1, "a": 1}}"#),
        ("b.json", r#"{"name": "b", "deps": {"a": 1}}"#),
    ]);
    // `deps`: 2 files and 3 one-field rows; `names`: 2 files and 2 rows;
    // `joined`: 3 rows of 2 fields.
    let memo = RunMemo::new(Arc::clone(&plan));
    assert!(run_with_memo(&plan, 0, &tree, limit(LimitKind::CellsPerRun, 15), &memo).is_ok());
    assert_eq!(memo.cells(), 15);
    assert_eq!(
        messages(&run(&plan, 0, &tree, limit(LimitKind::CellsPerRun, 14))),
        [format!(
            "relation `joined` (.alint.yml): relation limit `cells_per_run` (14) reached: 15 cells, \
             {CELLS_EXHAUSTED}"
        )]
    );
    // Three fields, three cells a row: `a.json` and its 2 rows hold 7.
    let mut wide = deps();
    wide.relation.fields.extend([
        (
            "version".to_string(),
            FieldDef {
                source: FieldSource::EntryValue,
                sensitive: false,
            },
        ),
        (
            "file".to_string(),
            FieldDef {
                source: FieldSource::Path(crate::relation::plan::PathToken::Path),
                sensitive: false,
            },
        ),
    ]);
    let wide = self::plan(vec![wide]);
    let one = self::tree(&[("a.json", r#"{"deps": {"b": 1, "a": 1}}"#)]);
    assert_eq!(
        messages(&run(&wide, 0, &one, limit(LimitKind::CellsPerRun, 6))),
        [format!(
            "a.json: relation `deps` (.alint.yml): relation limit `cells_per_run` (6) reached: 7 \
             cells, {CELLS_EXHAUSTED}"
        )]
    );
    assert!(run(&wide, 0, &one, limit(LimitKind::CellsPerRun, 7)).is_ok());
    // The count is the run's: `deps`, evaluated first, leaves `names` 3.
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::CellsPerRun, 7);
    assert!(run_with_memo(&plan, 1, &tree, limits, &memo).is_ok());
    assert_eq!(
        messages(&run_with_memo(&plan, 2, &tree, limits, &memo)),
        [format!(
            "b.json: relation `names` (.alint.yml): relation limit `cells_per_run` (7) reached: 8 \
             cells, {CELLS_EXHAUSTED}"
        )]
    );
}

/// The most a cell holds, its share of its row, provenance and resource list
/// included: what the evaluator's docs promise per cell
/// ([`LimitKind::CellsPerRun`]).
const CELL_BYTES: usize = 256;

/// What the memo's relations hold, each shared allocation once: their rows
/// and cells, the text of each value, the key paths they keep, their
/// resource lists, notes and failures. The fixed sizes are the types', so a
/// type that grows shows here.
fn held(memo: &RunMemo) -> usize {
    use std::mem::size_of;
    let mut seen = std::collections::HashSet::new();
    let mut once = |at: *const u8, bytes: usize| {
        if seen.insert(at as usize) { bytes } else { 0 }
    };
    let mut total = 0;
    for outcome in memo.outcomes.iter().filter_map(OnceLock::get) {
        let rows = match outcome {
            Ok(rows) => rows,
            Err(failures) => {
                total += once(
                    failures.as_ptr().cast(),
                    failures.len() * size_of::<RelationFailure>(),
                );
                for failure in failures.iter() {
                    total += once(failure.message.as_ptr(), failure.message.len());
                }
                continue;
            }
        };
        total += size_of::<RelationRows>()
            + rows.rows.capacity() * size_of::<FieldRow>()
            + once(
                rows.resources.as_ptr().cast(),
                rows.resources.len() * size_of::<Resource>(),
            );
        for note in &rows.notes {
            let kinds = match &note.skipped {
                Skipped::NonLiteral => 0,
                Skipped::NonString { kinds, .. } => kinds.capacity() * size_of_val(&kinds[0]),
            };
            total += size_of::<RelationNote>() + note.message.capacity() + kinds;
        }
        for row in &rows.rows {
            total += row.cells.capacity() * size_of::<Cell>();
            for cell in &row.cells {
                let pointer = cell.at.pointer.as_ref();
                total += once(
                    Arc::as_ptr(&cell.at).cast(),
                    size_of::<Provenance>() + pointer.map_or(0, Pointer::heap_bytes),
                );
                if let Some(path) = &cell.path {
                    let bytes = size_of::<Pointer>() + path.heap_bytes();
                    total += once(Arc::as_ptr(path).cast(), bytes);
                }
                total += match &cell.value {
                    Some(Value::String(text)) => once(text.as_ptr(), text.len()),
                    Some(Value::Number(number)) => {
                        once(number.lexeme().as_ptr(), number.lexeme().len())
                    }
                    _ => 0,
                };
            }
        }
    }
    total
}

/// The bound the evaluator's docs promise: what a run's relations hold is
/// at most their cells at [`CELL_BYTES`] each, and the bytes they took.
#[track_caller]
fn assert_held_within_the_bound(memo: &RunMemo) {
    let held = held(memo);
    let bound = memo.cells() * CELL_BYTES + memo.bytes();
    assert!(
        held <= bound,
        "the relations hold {held} bytes, more than their {} cells and {} bytes allow ({bound})",
        memo.cells(),
        memo.bytes()
    );
}

/// The text of a string cell.
fn text(cell: &Cell) -> &Arc<str> {
    match &cell.value {
        Some(Value::String(text)) => text,
        other => panic!("not a string: {other:?}"),
    }
}

const BYTES_EXHAUSTED: &str = "so the relation is not evaluated rather than cut short; a run's \
    relations hold each file they decode and each key path, path token and transformed value \
    they build, so read fewer or smaller files (narrow the selector or the `at:` paths) or bind \
    fewer key and path fields";

/// A value copied into many fields is held once: 64 `value` fields over one
/// `whole_file` read take 65 cells and the decode's bytes, and no field
/// copies the file's text.
#[test]
fn a_value_bound_to_many_fields_is_held_once() {
    let body = "x".repeat(64 * 1024);
    let tree = tree(&[("big.txt", body.as_str())]);
    let names: Vec<String> = (0..64).map(|field| format!("f{field}")).collect();
    let fields: Vec<(&str, FieldSource)> = names
        .iter()
        .map(|name| (name.as_str(), FieldSource::Value))
        .collect();
    let whole_file = Rows::Part(Part {
        from: from("big.txt"),
        reader: Reader::Read {
            read: Read::WholeFile,
            optional: false,
        },
    });
    let plan = plan(vec![named("wide", whole_file, &fields)]);
    // The decode holds the text and one row: a little more than the text.
    let limits = limit(LimitKind::CellsPerRun, 65)
        .with(LimitKind::BytesPerRun, body.len() + 1024)
        .unwrap();
    let memo = RunMemo::new(Arc::clone(&plan));
    let rows = run_with_memo(&plan, 0, &tree, limits, &memo).unwrap();
    assert_eq!((memo.cells(), rows.rows.len()), (65, 1));
    assert_held_within_the_bound(&memo);
    let first = text(&rows.rows[0].cells[0]);
    assert_eq!(first.len(), body.len());
    for cell in &rows.rows[0].cells {
        assert!(Arc::ptr_eq(first, text(cell)), "a field copied the text");
    }
    // The decode alone takes more bytes than the text.
    let short = limit(LimitKind::BytesPerRun, body.len());
    assert_eq!(
        messages(&run(&plan, 0, &tree, short)),
        [format!(
            "big.txt: relation `wide` (.alint.yml): relation limit `bytes_per_run` ({}) reached: \
             {} bytes, {BYTES_EXHAUSTED}",
            body.len(),
            memo.bytes()
        )]
    );
}

/// A join copies its sides' cells without their text: 100 chained joins over
/// a 64 KiB value hold the text once.
#[test]
fn a_chain_of_joins_copies_values_without_their_text() {
    let body = "v".repeat(64 * 1024);
    let tree = tree(&[("big.txt", body.as_str())]);
    let whole_file = Rows::Part(Part {
        from: from("big.txt"),
        reader: Reader::Read {
            read: Read::WholeFile,
            optional: false,
        },
    });
    let joins = 100;
    let mut relations = vec![named("base", whole_file, &[("v", FieldSource::Value)])];
    for join in 0..joins {
        relations.push(named(
            &format!("j{join}"),
            Rows::Join(JoinDef {
                left: reached(join),
                right: reached(0),
                on: vec![("v".into(), "v".into())],
                unique: JoinSide::Right,
            }),
            &[("v", FieldSource::Side(JoinSide::Left, "v".into()))],
        ));
    }
    let plan = plan(relations);
    // The base reads one file into one row; each join builds one row.
    let limits = limit(LimitKind::CellsPerRun, 2 + joins)
        .with(LimitKind::BytesPerRun, body.len() + 1024)
        .unwrap();
    let memo = RunMemo::new(Arc::clone(&plan));
    let last = run_with_memo(&plan, joins, &tree, limits, &memo).unwrap();
    assert_held_within_the_bound(&memo);
    let base = memo.outcomes[0].get().unwrap().as_ref().unwrap();
    assert!(Arc::ptr_eq(
        text(&base.rows[0].cells[0]),
        text(&last.rows[0].cells[0])
    ));
}

/// The `entry.*` fields of a row share what they copy: a long key bound to
/// many fields, as `entry.key`, `entry.root` and `entry.path`, is built once
/// per source, and a path token once per resource.
#[test]
fn a_rows_entry_fields_share_each_source_once() {
    let key = "k".repeat(16 * 1024);
    let body = format!(r#"{{"{key}": {{"a": 1, "b": 2}}}}"#);
    let tree = tree(&[("a.json", body.as_str())]);
    let sources = [
        FieldSource::EntryKey,
        FieldSource::EntryRoot,
        FieldSource::EntryPath,
        FieldSource::Path(crate::relation::plan::PathToken::Path),
    ];
    let names: Vec<String> = (0..64).map(|field| format!("f{field}")).collect();
    let fields: Vec<(&str, FieldSource)> = names
        .iter()
        .enumerate()
        .map(|(field, name)| (name.as_str(), sources[field % 4].clone()))
        .collect();
    let leaves = Rows::Part(Part {
        from: from("a.json"),
        reader: Reader::Entries(Entries {
            format: EntriesFormat::Json,
            at: vec!["$".into()],
            leaves: true,
        }),
    });
    let plan = plan(vec![named("keys", leaves, &fields)]);
    // Two leaves under the long key: the decode holds the key in each
    // row's key path, and each row its own copy of the key path, `entry.key`
    // (one letter), `entry.root` and `entry.path`'s text and key path.
    let limits = limit(LimitKind::CellsPerRun, 1 + 2 * 64)
        .with(LimitKind::BytesPerRun, 12 * key.len())
        .unwrap();
    let memo = RunMemo::new(Arc::clone(&plan));
    let rows = run_with_memo(&plan, 0, &tree, limits, &memo).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_held_within_the_bound(&memo);
    for row in &rows.rows {
        for (field, cell) in row.cells.iter().enumerate() {
            let shared = &row.cells[field % 4];
            assert!(Arc::ptr_eq(text(shared), text(cell)), "field {field}");
            assert_eq!(
                shared.path.as_ref().map(Arc::as_ptr),
                cell.path.as_ref().map(Arc::as_ptr)
            );
        }
    }
    assert_eq!(text(&rows.rows[1].cells[0]).as_ref(), "b");
    assert_eq!(text(&rows.rows[1].cells[1]).as_ref(), key);
    assert!(Arc::ptr_eq(
        text(&rows.rows[0].cells[3]),
        text(&rows.rows[1].cells[3])
    ));
}

/// A union of 40 parts over 25 files and a chain of 1,000 joins over it: the
/// union lists each file once, the joins share its list, and the run holds
/// what its cells allow (as `cells_per_run` of 1,000, a cell per file each
/// part reads, and next to no bytes, admit).
#[test]
fn a_chain_of_joins_shares_its_sides_resources() {
    let files: Vec<String> = (0..25).map(|file| format!("f{file:02}.json")).collect();
    let tree = tree(
        &files
            .iter()
            .map(|path| (path.as_str(), "{}"))
            .collect::<Vec<_>>(),
    );
    let part = Part {
        from: from("*.json"),
        reader: Reader::Entries(Entries {
            format: EntriesFormat::Json,
            at: vec!["$.nope".into()],
            leaves: false,
        }),
    };
    let joins = 1_000;
    let mut relations = vec![named(
        "base",
        Rows::Union(vec![part; 40]),
        &[("k", FieldSource::EntryKey)],
    )];
    for join in 0..joins {
        relations.push(named(
            &format!("j{join}"),
            Rows::Join(JoinDef {
                left: reached(join),
                right: reached(0),
                on: vec![("k".into(), "k".into())],
                unique: JoinSide::Right,
            }),
            &[("k", FieldSource::Side(JoinSide::Left, "k".into()))],
        ));
    }
    let plan = plan(relations);
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::CellsPerRun, 40 * 25)
        .with(LimitKind::BytesPerRun, 1)
        .unwrap();
    let last = run_with_memo(&plan, joins, &tree, limits, &memo).unwrap();
    assert_held_within_the_bound(&memo);
    let base = memo.outcomes[0].get().unwrap().as_ref().unwrap();
    let listed: Vec<&str> = base.resources.iter().map(|r| r.path().as_str()).collect();
    assert_eq!(listed, files, "each file once, in path order");
    assert!(Arc::ptr_eq(&base.resources, &last.resources));
}

/// A chain of joins over a relation that fails shares its failures: each
/// join fails with the base's list, not a copy, and the run takes the
/// base's bytes once.
#[test]
fn a_chain_of_joins_over_a_failure_shares_its_failures() {
    let files: Vec<String> = (0..25).map(|file| format!("f{file:02}.json")).collect();
    let tree = tree(
        &files
            .iter()
            .map(|path| (path.as_str(), "{"))
            .collect::<Vec<_>>(),
    );
    let joins = 1_000;
    let mut relations = vec![named(
        "base",
        entries("*.json", "$"),
        &[("k", FieldSource::EntryKey)],
    )];
    for join in 0..joins {
        relations.push(named(
            &format!("j{join}"),
            Rows::Join(JoinDef {
                left: reached(join),
                right: reached(0),
                on: vec![("k".into(), "k".into())],
                unique: JoinSide::Right,
            }),
            &[("k", FieldSource::Side(JoinSide::Left, "k".into()))],
        ));
    }
    let plan = plan(relations);
    // The base's failures, alone, take this many bytes.
    let alone = RunMemo::new(Arc::clone(&plan));
    let base = run_with_memo(&plan, 0, &tree, Limits::default(), &alone).unwrap_err();
    assert_eq!(base.len(), 25);
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::BytesPerRun, alone.bytes());
    let last = run_with_memo(&plan, joins, &tree, limits, &memo).unwrap_err();
    assert_held_within_the_bound(&memo);
    assert_eq!(memo.bytes(), alone.bytes());
    let base = memo.outcomes[0].get().unwrap().as_ref().unwrap_err();
    assert!(
        Arc::ptr_eq(base, &last),
        "the last join copied the base's failures"
    );
    // A list the run has no bytes left for gives way to one failure that
    // says so.
    let short = limit(LimitKind::BytesPerRun, alone.bytes() - 1);
    assert_eq!(
        messages(&run(&plan, 0, &tree, short)),
        [format!(
            "relation `base` (.alint.yml): relation limit `bytes_per_run` ({}) reached: {} bytes, \
             {BYTES_EXHAUSTED}",
            alone.bytes() - 1,
            alone.bytes()
        )]
    );
}

/// A transform changes each text once, however many rows share it, and a
/// text it leaves as it was stays shared: 30 parts over one file lowered
/// take the lowered lines' bytes once.
#[test]
fn rows_that_share_a_value_share_what_a_transform_makes_of_it() {
    let body = (0..100)
        .map(|line| format!("Line{line}\nl{line}\n"))
        .collect::<Vec<_>>()
        .concat();
    let tree = tree(&[("list.txt", &body)]);
    let listed = named(
        "listed",
        Rows::Union(vec![lines("list.txt"); 30]),
        &[("line", FieldSource::Value)],
    );
    // Untransformed, the relation holds the decode's bytes.
    let untransformed = RunMemo::new(plan(vec![listed.clone()]));
    let as_read = Arc::clone(untransformed.plan());
    run_with_memo(&as_read, 0, &tree, Limits::default(), &untransformed).unwrap();
    let decode = untransformed.bytes();
    // Lowered, it holds each of the 100 capitalized lines lowered once.
    let mut lowering = listed;
    lowering.relation.transforms = vec![("line".into(), vec![Transform::Lower])];
    let plan = plan(vec![lowering]);
    let lowered: usize = (0..100).map(|line| format!("line{line}").len()).sum();
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::BytesPerRun, decode + lowered);
    let rows = run_with_memo(&plan, 0, &tree, limits, &memo).unwrap();
    assert_eq!(rows.rows.len(), 30 * 200);
    assert_held_within_the_bound(&memo);
    let (first, second) = (&rows.rows[0].cells[0], &rows.rows[200].cells[0]);
    assert_eq!(text(first).as_ref(), "line0");
    assert!(Arc::ptr_eq(text(first), text(second)), "lowered twice");
    // `l0` was lower case already: the decode's text, shared.
    assert!(Arc::ptr_eq(
        text(&rows.rows[1].cells[0]),
        text(&rows.rows[201].cells[0])
    ));
    assert_eq!(
        messages(&run(
            &plan,
            0,
            &tree,
            limit(LimitKind::BytesPerRun, decode + lowered - 1)
        )),
        [format!(
            "relation `listed` (.alint.yml): relation limit `bytes_per_run` ({}) reached: {} \
             bytes, {BYTES_EXHAUSTED}",
            decode + lowered - 1,
            decode + lowered
        )]
    );
}

/// Rows a join gives one key path share what `strip-root` makes of it: 100
/// rows matched to one entry strip its key path once.
#[test]
fn rows_that_share_a_key_path_share_what_strip_root_makes_of_it() {
    let refs: String = "x\n".repeat(100);
    let tree = tree(&[
        ("en.json", r#"{"en": {"title": "x"}}"#),
        ("refs.txt", &refs),
    ]);
    let keys = named(
        "keys",
        Rows::Part(Part {
            from: from("en.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$".into()],
                leaves: true,
            }),
        }),
        &[
            ("path", FieldSource::EntryPath),
            ("value", FieldSource::EntryValue),
        ],
    );
    let refs = named(
        "refs",
        Rows::Part(lines("refs.txt")),
        &[("value", FieldSource::Value)],
    );
    let mut used = named(
        "used",
        Rows::Join(JoinDef {
            left: reached(2),
            right: reached(1),
            on: vec![("value".into(), "value".into())],
            unique: JoinSide::Right,
        }),
        &[("path", FieldSource::Side(JoinSide::Right, "path".into()))],
    );
    let plan = plan(vec![used.clone(), keys, refs]);
    // Unstripped, the run holds the decodes and the entry's key path.
    let unstripped = RunMemo::new(Arc::clone(&plan));
    run_with_memo(&plan, 0, &tree, Limits::default(), &unstripped).unwrap();
    used.relation.transforms = vec![("path".into(), vec![Transform::StripRoot])];
    let plan = self::plan(vec![
        used,
        plan.relations[1].clone(),
        plan.relations[2].clone(),
    ]);
    let title = Pointer::root().key("title");
    let stripped = super::shared_pointer_bytes(&title) + "title".len();
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::BytesPerRun, unstripped.bytes() + stripped);
    let rows = run_with_memo(&plan, 0, &tree, limits, &memo).unwrap();
    assert_eq!(rows.rows.len(), 100);
    assert_held_within_the_bound(&memo);
    let first = rows.rows[0].cells[0].path.as_ref().unwrap();
    assert_eq!(**first, title);
    for row in &rows.rows {
        assert!(Arc::ptr_eq(first, row.cells[0].path.as_ref().unwrap()));
    }
    // A byte less, and the stripped key path is past the run's bytes.
    let memo = RunMemo::new(Arc::clone(&plan));
    let limits = limit(LimitKind::BytesPerRun, unstripped.bytes() + stripped - 1);
    let failed = messages(&run_with_memo(&plan, 0, &tree, limits, &memo));
    assert!(
        failed.len() == 1 && failed[0].contains("`bytes_per_run`"),
        "{failed:?}"
    );
}

/// A run with explicit caches, which the caller inspects.
fn run_with_caches(
    plan: &Arc<Plan>,
    id: usize,
    tree: &Tree,
    limits: Limits,
    (reads, decodes, memo): (&ReadCache, &DecodeCache, &RunMemo),
) -> Outcome {
    evaluate_with(
        plan,
        RelationId(id),
        tree.dir.path(),
        &tree.index,
        limits,
        reads,
        decodes,
        memo,
    )
}

/// A part whose decode the run holds reads nothing: a `union:` of 30
/// identical parts reads its file once, not once a part.
#[test]
fn a_part_whose_decode_is_held_reads_nothing() {
    let tree = tree(&[("list.txt", "a\nb\n")]);
    let plan = plan(vec![named(
        "listed",
        Rows::Union(vec![lines("list.txt"); 30]),
        &[("line", FieldSource::Value)],
    )]);
    let limits = Limits::default();
    let (reads, decodes) = (ReadCache::new(), DecodeCache::with_run_limits(&limits));
    let memo = RunMemo::new(Arc::clone(&plan));
    let rows = run_with_caches(&plan, 0, &tree, limits, (&reads, &decodes, &memo)).unwrap();
    assert_eq!(rows.rows.len(), 60);
    assert_eq!((reads.reads(), decodes.decodes()), (1, 1));
}

/// A decode the run makes takes what it holds from the run's bytes once:
/// its rows, its notes with their values, and its non-string kinds. A run
/// that finds the decode held takes nothing for it. The budget a run needs
/// is exactly what it takes, and the decode needs at least what it holds, by
/// count; the exact sums after pin the types' sizes too, so an overcount
/// shows as an undercount does.
#[test]
fn a_fresh_decode_takes_what_it_holds_from_the_runs_bytes() {
    use std::mem::size_of;
    let tree = tree(&[("a.json", r#"{"v": ["${A}", "${BB}", 1]}"#)]);
    let plan = plan(vec![named(
        "values",
        read_json("*.json", "$.v[*]"),
        &[("v", FieldSource::Value)],
    )]);
    let limits = Limits::default();
    let (reads, decodes) = (ReadCache::new(), DecodeCache::with_run_limits(&limits));
    let fresh = RunMemo::new(Arc::clone(&plan));
    let rows = run_with_caches(&plan, 0, &tree, limits, (&reads, &decodes, &fresh)).unwrap();
    let held = RunMemo::new(Arc::clone(&plan));
    run_with_caches(&plan, 0, &tree, limits, (&reads, &decodes, &held)).unwrap();
    assert_eq!(decodes.decodes(), 1);
    // The decode the cache holds, found by its adapter, path and limits.
    let adapter = crate::relation::ExtractAdapter::new(
        crate::extract::Extract::structured(crate::structured_format::Format::Json, "$.v[*]")
            .unwrap(),
    );
    let output = decodes
        .decode(&adapter, &rows.resources[0], b"", &limits)
        .unwrap();
    assert_eq!((output.rows.len(), output.notes.len()), (0, 2));
    assert_eq!(output.non_strings.len(), 1);
    // Exactly what a run takes is enough for it, and a byte less is not.
    let total = fresh.bytes();
    let within = |bytes: usize| {
        let memo = RunMemo::new(Arc::clone(&plan));
        run_with_memo(&plan, 0, &tree, limit(LimitKind::BytesPerRun, bytes), &memo)
    };
    assert!(within(total).is_ok());
    let failed = messages(&within(total - 1));
    assert!(
        failed.len() == 1 && failed[0].contains("`bytes_per_run`"),
        "{failed:?}"
    );
    // The decode needs at least each note, its value's text and each
    // non-string kind beyond what the notes take: a budget a byte short of
    // that stops the run.
    let held_by_decode = output.notes.len() * size_of::<crate::relation::AdapterNote>()
        + "${A}".len()
        + "${BB}".len()
        + output.non_strings.len() * size_of::<crate::extract::NonStringValueKind>();
    assert!(within(held.bytes() + held_by_decode - 1).is_err());
    let expected = output.rows.capacity() * size_of::<crate::relation::row::Row>()
        + output.notes.capacity() * size_of::<crate::relation::AdapterNote>()
        + "${A}".len()
        + "${BB}".len()
        + output.non_strings.capacity() * size_of::<crate::extract::NonStringValueKind>();
    assert_eq!(fresh.bytes() - held.bytes(), expected);
    // What the run that found the decode took is its notes: each note,
    // its message, and the kinds a non-string note counts.
    let notes: usize = rows
        .notes
        .iter()
        .map(|note| {
            let kinds = match &note.skipped {
                Skipped::NonLiteral => 0,
                Skipped::NonString { kinds, .. } => kinds.len() * size_of_val(&kinds[0]),
            };
            size_of::<RelationNote>() + note.message.len() + kinds
        })
        .sum();
    assert_eq!(rows.notes.len(), 3);
    assert_eq!(held.bytes(), notes);
}

/// Past the run's decoded rows the relation stops, as it does past its
/// cells: 50 files of one row each under `rows_per_run: 1` decode twice and
/// fail once, and a later relation decodes nothing.
#[test]
fn the_run_row_limit_stops_the_relation_and_every_later_decode() {
    let mut files: Vec<(String, String)> = (0..50)
        .map(|file| (format!("f{file:02}.txt"), format!("l{file}\n")))
        .collect();
    files.push(("other.md".into(), "x\n".into()));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let tree = tree(&files);
    let plan = plan(vec![
        named(
            "listed",
            Rows::Part(lines("*.txt")),
            &[("line", FieldSource::Value)],
        ),
        named(
            "other",
            Rows::Part(lines("*.md")),
            &[("line", FieldSource::Value)],
        ),
    ]);
    let limits = limit(LimitKind::RowsPerRun, 1);
    let (reads, decodes) = (ReadCache::new(), DecodeCache::with_run_limits(&limits));
    let memo = RunMemo::new(Arc::clone(&plan));
    let caches = (&reads, &decodes, &memo);
    let exhausted = "rows or values, so the relation is not evaluated rather than cut short; \
                     narrow what it reads (its selector, or its `at:` paths)";
    assert_eq!(
        messages(&run_with_caches(&plan, 0, &tree, limits, caches)),
        [format!(
            "f01.txt: relation `listed` (.alint.yml): relation limit `rows_per_run` (1) reached: \
             2 {exhausted}"
        )]
    );
    assert_eq!((reads.reads(), decodes.decodes()), (2, 2));
    // The budget never comes back, so the next relation's decode is refused
    // without being made.
    assert_eq!(
        messages(&run_with_caches(&plan, 1, &tree, limits, caches)),
        [format!(
            "other.md: relation `other` (.alint.yml): relation limit `rows_per_run` (1) reached: \
             2 {exhausted}"
        )]
    );
    assert_eq!(decodes.decodes(), 2);
}

fn versions() -> NamedRelation {
    named(
        "versions",
        Rows::Part(Part {
            from: from("crates/*/Cargo.toml"),
            reader: Reader::Read {
                read: Read::Toml("$.package.version".into()),
                optional: false,
            },
        }),
        &[
            ("version", FieldSource::Value),
            ("manifest", FieldSource::Path(PathToken::Path)),
        ],
    )
}

const WORKSPACE_VERSIONS: [(&str, &str); 3] = [
    ("crates/a/Cargo.toml", "[package]\nversion = \"1.0.0\"\n"),
    (
        "crates/b/Cargo.toml",
        "[package]\nversion.workspace = true\n",
    ),
    ("crates/c/Cargo.toml", "[package]\nversion = \"3.0.0\"\n"),
];

/// A `read:` that matches a value that is not a string notes the resource
/// once, naming the kinds, and keeps the other resources' rows, as
/// `cross_file` does.
#[test]
fn a_non_string_match_is_one_note_and_the_other_files_keep_their_rows() {
    let tree = tree(&WORKSPACE_VERSIONS);
    let plan = plan(vec![versions()]);
    let rows = run(&plan, 0, &tree, Limits::default()).unwrap();
    let values: Vec<(String, String)> = rows
        .rows
        .iter()
        .map(|row| {
            let text = |cell: &Cell| match &cell.value {
                Some(Value::String(text)) => text.to_string(),
                Some(Value::Path(path)) => path.as_str().to_string(),
                other => format!("{other:?}"),
            };
            (text(&row.cells[0]), text(&row.cells[1]))
        })
        .collect();
    assert_eq!(
        values,
        [
            ("1.0.0".to_string(), "crates/a/Cargo.toml".to_string()),
            ("3.0.0".to_string(), "crates/c/Cargo.toml".to_string()),
        ]
    );
    assert_eq!(
        rows.notes,
        [RelationNote {
            resource: rows.resources[1].clone(),
            skipped: Skipped::NonString {
                part: 0,
                kinds: vec![(crate::extract::NonStringValueKind::Object, 1)],
            },
            message: "`read.toml: $.package.version` matched non-string value(s) (object: 1); \
                      `read:` rows hold strings, so select string values, or read typed values \
                      with `entries:`"
                .into(),
        }]
    );
    assert_eq!(rows.resources[1].path().as_str(), "crates/b/Cargo.toml");
    // A configuration that reads the environment does not name its `read:`.
    let mut hidden = versions();
    hidden.reads_environment = true;
    let rows = run(&self::plan(vec![hidden]), 0, &tree, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert!(
        rows.notes[0]
            .message
            .starts_with("`read:` matched non-string value(s) (object: 1)"),
        "{}",
        rows.notes[0].message
    );
}

/// A non-literal value a sensitive `value` field reads is skipped without
/// being shown; a sensitive field of another source leaves it shown.
#[test]
fn a_sensitive_values_non_literal_note_does_not_show_it() {
    let tree = tree(&[("b.json", r#"{"name": "${NAME}"}"#)]);
    let notes = |relation: NamedRelation| -> Vec<String> {
        let rows = run(&plan(vec![relation]), 0, &tree, Limits::default()).unwrap();
        rows.notes.iter().map(|note| note.message.clone()).collect()
    };
    let mut hidden = names();
    hidden.relation.fields[0].1.sensitive = true;
    assert_eq!(notes(hidden), ["skipped a non-literal value"]);
    let mut shown = named(
        "names",
        read_json("*.json", "$.name"),
        &[
            ("name", FieldSource::Value),
            ("file", FieldSource::Path(PathToken::Path)),
        ],
    );
    shown.relation.fields[1].1.sensitive = true;
    assert_eq!(notes(shown), ["skipped non-literal value \"${NAME}\""]);
}

/// Each path token a relation binds holds its own text.
#[test]
fn each_path_token_a_relation_binds_holds_its_own_text() {
    let tree = tree(&[("a.json", r#"{"name": "x"}"#)]);
    let relation = named(
        "names",
        read_json("*.json", "$.name"),
        &[
            ("name", FieldSource::Value),
            ("ext", FieldSource::Path(PathToken::Ext)),
            ("path", FieldSource::Path(PathToken::Path)),
            ("stem", FieldSource::Path(PathToken::Stem)),
        ],
    );
    let rows = run(&plan(vec![relation]), 0, &tree, Limits::default()).unwrap();
    let cells: Vec<&str> = rows.rows[0]
        .cells
        .iter()
        .map(|cell| &**text(cell))
        .collect();
    assert_eq!(cells, ["x", "json", "a.json", "a"]);
}

/// A join and a `where:` list keep the notes of what they read: a value a
/// side skipped is a row the join lacks.
#[test]
fn joins_and_where_lists_keep_the_notes_of_what_they_read() {
    let tree = tree(&[
        ("a.json", r#"{"name": "a", "deps": {"a": 1, "b": 1}}"#),
        ("b.json", r#"{"name": "${NAME}", "deps": {}}"#),
        ("c.json", r#"{"name": 1, "deps": {}}"#),
    ]);
    let mut filtered = deps();
    filtered.relation.filters = vec![(
        "name".into(),
        vec![Predicate::NotIn(vec![Member::Relation {
            relation: reached(2),
            field: "name".into(),
        }])],
    )];
    let plan = plan(vec![joined(), deps(), names(), filtered]);
    let notes = |rows: &RelationRows| -> Vec<(String, String)> {
        rows.notes
            .iter()
            .map(|note| (note.resource.path().to_string(), note.message.clone()))
            .collect()
    };
    let read = run(&plan, 2, &tree, Limits::default()).unwrap();
    assert_eq!(read.notes.len(), 2);
    let joined = run(&plan, 0, &tree, Limits::default()).unwrap();
    assert_eq!(joined.rows.len(), 1);
    assert_eq!(notes(&joined), notes(&read));
    let filtered = run(&plan, 3, &tree, Limits::default()).unwrap();
    assert_eq!(filtered.rows.len(), 1, "`b` is not a name");
    assert_eq!(notes(&filtered), notes(&read));
    assert_eq!(
        notes(&read),
        [
            (
                "b.json".to_string(),
                "skipped non-literal value \"${NAME}\"".to_string()
            ),
            (
                "c.json".to_string(),
                "`read.json: $.name` matched non-string value(s) (number: 1); `read:` rows hold \
                 strings, so select string values, or read typed values with `entries:`"
                    .to_string()
            ),
        ]
    );
}

/// A `read:` row is its resource, its part and its ordinal: two lines of a
/// file are two rows, and a line keeps its identity when a `where:` drops an
/// earlier one.
#[test]
fn read_rows_are_told_apart_by_their_part_and_ordinal() {
    let tree = tree(&[("list.txt", "x\ny\nz\n")]);
    let line = &[("line", FieldSource::Value)];
    let mut dropped = named("dropped", Rows::Part(lines("list.txt")), line);
    dropped.relation.filters = vec![(
        "line".into(),
        vec![Predicate::NotIn(vec![Member::Literal(Value::String(
            "x".into(),
        ))])],
    )];
    let plan = plan(vec![
        named("all", Rows::Part(lines("list.txt")), line),
        dropped,
        named(
            "both",
            Rows::Union(vec![lines("list.txt"), lines("list.txt")]),
            line,
        ),
    ]);
    let at = |id: usize| -> Vec<(u32, u32)> {
        let rows = run(&plan, id, &tree, Limits::default()).unwrap();
        rows.rows
            .iter()
            .map(|row| (row.cells[0].at.part, row.cells[0].at.ordinal))
            .collect()
    };
    assert_eq!(at(0), [(0, 0), (0, 1), (0, 2)]);
    assert_eq!(at(1), [(0, 1), (0, 2)]);
    assert_eq!(at(2), [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2)]);
    let all = run(&plan, 0, &tree, Limits::default()).unwrap();
    assert_ne!(all.rows[0].cells[0].at, all.rows[1].cells[0].at);
}

fn roots(sensitive: bool) -> NamedRelation {
    let mut keys = named(
        "keys",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$".into()],
                leaves: true,
            }),
        }),
        &[("path", FieldSource::EntryPath)],
    );
    keys.relation.fields[0].1.sensitive = sensitive;
    keys.relation.transforms = vec![("path".into(), vec![Transform::StripRoot])];
    keys
}

/// `strip-root` over a file of 200,000 top-level keys fails at once with a
/// message that names three of them.
#[test]
fn strip_root_over_many_roots_fails_fast_naming_a_few() {
    let roots_in = |count: usize| {
        let members: Vec<String> = (0..count)
            .map(|root| format!("\"r{root}\":{{\"k\":1}}"))
            .collect();
        format!("{{{}}}", members.join(","))
    };
    let tree = tree(&[
        ("many.json", &roots_in(200_000)),
        ("two.json", &roots_in(2)),
    ]);
    let started = std::time::Instant::now();
    let failures = messages(&run(&plan(vec![roots(false)]), 0, &tree, Limits::default()));
    let elapsed = started.elapsed();
    let fixed = "; read one top-level object per file, or drop `strip-root`";
    assert_eq!(
        failures,
        [
            format!(
                "many.json: relation `keys` (.alint.yml): `strip-root` on `path` needs one \
                 top-level key in each resource, but this one has `r0`, `r1`, `r2` and 199997 \
                 more{fixed}"
            ),
            format!(
                "two.json: relation `keys` (.alint.yml): `strip-root` on `path` needs one \
                 top-level key in each resource, but this one has `r0`, `r1`{fixed}"
            ),
        ]
    );
    assert!(elapsed.as_secs() < 30, "{elapsed:?}");
    // A sensitive field names none.
    let failures = messages(&run(&plan(vec![roots(true)]), 0, &tree, Limits::default()));
    assert!(
        failures[0].contains("but this one has 200000;"),
        "{}",
        failures[0]
    );
    // Nor does a field of a relation whose keys another field holds
    // sensitive.
    let mut keyed = roots(false);
    keyed.relation.fields.push((
        "key".into(),
        FieldDef {
            source: FieldSource::EntryKey,
            sensitive: true,
        },
    ));
    let two = self::tree(&[("two.json", &roots_in(2))]);
    let failures = messages(&run(&plan(vec![keyed]), 0, &two, Limits::default()));
    assert!(
        failures[0].contains("but this one has 2;"),
        "{}",
        failures[0]
    );
}

/// A repeated join key is quoted to a prefix and its length: a key of a
/// megabyte never makes a message of one.
#[test]
fn a_repeated_join_key_is_quoted_to_a_prefix() {
    let long = "k".repeat(1 << 20);
    let manifest = format!(r#"{{"name": "{long}", "deps": {{}}}}"#);
    let tree = tree(&[("a.json", manifest.as_str()), ("b.json", manifest.as_str())]);
    let plan = plan(vec![joined(), deps(), names()]);
    let failures = messages(&run(&plan, 0, &tree, Limits::default()));
    let quoted = format!("\"{}\"... ({} bytes)", "k".repeat(64), 1 << 20);
    assert_eq!(
        failures,
        [format!(
            "b.json: relation `joined` (.alint.yml): the join's unique side (right, `names`) \
             repeats the key {quoted}: a.json and b.json; `unique: right` needs each key once \
             on that side, so make the key unique there, or name the other side unique"
        )]
    );
    // A short key, and one of exactly the most characters quoted, are whole.
    let render = |text: String| super::ops::render_value(&Value::String(text.into()));
    assert_eq!(render("é".repeat(64)), format!("\"{}\"", "é".repeat(64)));
    assert_eq!(
        render("é".repeat(65)),
        format!("\"{}\"... (130 bytes)", "é".repeat(64))
    );
}

/// A key path a run holds in an `Arc` counts the `Arc`'s two counts, the
/// pointer and its segments against the run's bytes: 16 bytes of counts on
/// a 64-bit target, the pointer, and each segment with its key's text.
#[test]
fn a_shared_key_path_counts_its_arc_its_pointer_and_its_segments() {
    let pointer = Pointer::root().key("dependencies").key("left-pad");
    let segments = 2 * std::mem::size_of::<crate::relation::Segment>() + "dependencies".len() + 8;
    assert_eq!(pointer.heap_bytes(), segments);
    assert_eq!(
        super::shared_pointer_bytes(&pointer),
        2 * std::mem::size_of::<usize>() + std::mem::size_of::<Pointer>() + segments
    );
    assert_eq!(
        super::shared_pointer_bytes(&Pointer::root()),
        2 * std::mem::size_of::<usize>() + std::mem::size_of::<Pointer>()
    );
}

/// A relation whose rows lack a field it names (which the plan check rules
/// out) fails saying so, never with an empty list of failures.
#[test]
fn a_field_the_rows_lack_fails_the_relation_as_a_bug() {
    let mut relation = deps();
    relation.relation.transforms = vec![("missing".into(), vec![Transform::Trim])];
    let plan = plan(vec![relation]);
    let tree = tree(&[("a.json", r#"{"deps": {"b": 1}}"#)]);
    assert_eq!(
        messages(&run(&plan, 0, &tree, Limits::default())),
        [
            "relation `deps` (.alint.yml): the relation names a field its rows do not have; this \
             is a bug in alint's plan check, so please report it"
        ]
    );
}

/// A relation selector with a literal directory prefix narrows to that subtree
/// rather than testing every entry in the index (#565).
#[test]
fn selector_with_directory_prefix_narrows_index_scan() {
    let Some(before) = crate::scope_match_count() else {
        return;
    };
    let mut files: Vec<(String, &'static str)> = Vec::new();
    for i in 0..60 {
        files.push((format!("pkg/d{i:03}/a.json"), r#"{"name": "ok"}"#));
    }
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, c)| (p.as_str(), *c)).collect();
    let tree = tree(&refs);
    let relation = named(
        "one_pkg",
        Rows::Part(Part {
            from: from("pkg/d000/*.json"),
            reader: Reader::Read {
                read: Read::Json("$.name".into()),
                optional: false,
            },
        }),
        &[("name", FieldSource::Value)],
    );
    let plan = plan(vec![relation]);
    let rows = run(&plan, 0, &tree, Limits::default()).unwrap();
    let tested = crate::scope_match_count().unwrap() - before;
    assert_eq!(rows.rows.len(), 1);
    assert!(
        (1..=4).contains(&tested),
        "expected narrowed positive scan on pkg/d000/*, tested {tested} entries"
    );
}

#[test]
fn optional_bound_skips_resource_or_emits_missing_cell() {
    use crate::relation::plan::BoundMissing;

    let files = tree(&[
        ("root.json", r#"{"deps": {"lodash": "1.0.0"}}"#),
        (
            "pkg-a.json",
            r#"{"name": "pkg-a", "version": "1.0.0", "deps": {"react": "18.0.0"}}"#,
        ),
    ]);

    // `on_missing: skip` skips `root.json` completely (rows AND `resources`),
    // even when a required `bound:` field (`$.version`) is declared BEFORE the
    // optional skip field (`$.name`).
    let skip_rel = named(
        "deps_skip",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
            }),
        }),
        &[
            (
                "version",
                FieldSource::Bound {
                    query: "$.version".into(),
                    optional: false,
                    on_missing: BoundMissing::Skip,
                },
            ),
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Skip,
                },
            ),
            ("dep", FieldSource::EntryKey),
        ],
    );
    let rows = run(&plan(vec![skip_rel]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.resources.len(), 1);
    assert_eq!(rows.resources[0].path().as_str(), "pkg-a.json");
    assert_eq!(
        rows.rows[0].cells[1].value,
        Some(Value::String("pkg-a".into()))
    );
}

#[test]
fn optional_bound_on_missing_missing_keeps_rows_for_entries_and_read() {
    use crate::relation::plan::BoundMissing;

    let files = tree(&[
        ("root.json", r#"{"deps": {"lodash": "1.0.0"}}"#),
        (
            "pkg-a.json",
            r#"{"name": "pkg-a", "version": "1.0.0", "deps": {"react": "18.0.0"}}"#,
        ),
    ]);

    // `on_missing: missing` keeps `root.json`'s row with `None` in the bound cell
    // (both for `entries:` and for `read:`).
    let missing_entries = named(
        "deps_missing",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
            }),
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Missing,
                },
            ),
            ("dep", FieldSource::EntryKey),
        ],
    );
    let rows = run(&plan(vec![missing_entries]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.resources.len(), 2);
    assert_eq!(
        rows.rows[0].cells[0].value,
        Some(Value::String("pkg-a".into()))
    );
    assert_eq!(rows.rows[1].cells[0].value, None);
    assert_eq!(
        rows.rows[1].cells[1].value,
        Some(Value::String("lodash".into()))
    );

    let missing_read = named(
        "read_missing",
        Rows::Part(Part {
            from: from("*.json"),
            reader: Reader::Read {
                read: Read::Json("$.deps.*".into()),
                optional: false,
            },
        }),
        &[
            (
                "pkg",
                FieldSource::Bound {
                    query: "$.name".into(),
                    optional: true,
                    on_missing: BoundMissing::Missing,
                },
            ),
            ("ver", FieldSource::Value),
        ],
    );
    let rows = run(&plan(vec![missing_read]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.rows[1].cells[0].value, None);
    assert_eq!(
        rows.rows[1].cells[1].value,
        Some(Value::String("1.0.0".into()))
    );
}

#[test]
fn optional_bound_still_fails_on_multi_match_or_non_scalar() {
    use crate::relation::plan::BoundMissing;

    let bad = tree(&[
        ("multi.json", r#"{"names": ["a", "b"], "deps": {"x": "1"}}"#),
        ("obj.json", r#"{"meta": {"a": 1}, "deps": {"x": "1"}}"#),
    ]);
    let multi_rel = named(
        "multi",
        Rows::Part(Part {
            from: from("multi.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
            }),
        }),
        &[(
            "pkg",
            FieldSource::Bound {
                query: "$.names[*]".into(),
                optional: true,
                on_missing: BoundMissing::Skip,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![multi_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("matches more than once"), "{errs:?}");

    let obj_rel = named(
        "obj",
        Rows::Part(Part {
            from: from("obj.json"),
            reader: Reader::Entries(Entries {
                format: EntriesFormat::Json,
                at: vec!["$.deps".into()],
                leaves: false,
            }),
        }),
        &[(
            "pkg",
            FieldSource::Bound {
                query: "$.meta".into(),
                optional: true,
                on_missing: BoundMissing::Missing,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![obj_rel]), 0, &bad, Limits::default()));
    assert!(errs[0].contains("a field holds a scalar"), "{errs:?}");
}
