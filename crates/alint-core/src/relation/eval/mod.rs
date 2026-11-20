//! Executing relations (design `v0.23/selectors-and-constraints.md`, sections
//! 5 to 8; ADR-0034 decisions 5 to 7, as ADR-0037 decision 3 corrects them):
//! a [`Plan`]'s relation and a walked tree in, the relation's rows out.
//!
//! [`evaluate`] is what the `constraint` kind calls with a rule's
//! [`Context`], and what the engine calls, in plan order, for each relation
//! a rule that runs reads, before rules run. A run whose rules read no
//! relation evaluates none, reads nothing for one and allocates nothing for
//! one (the memo on the [`FileIndex`] is set by the first evaluation).
//!
//! - **Rows.** A relation's rows come from its parts in the order written (a
//!   `union:`'s parts, or its one `entries:` or `read:`), each part's
//!   resources in path order, each resource's rows in the order its adapter
//!   emits them; or from a `join:` (section 8). Each row holds one [`Cell`] per
//!   field, in the order the fields are written, and each cell keeps where its
//!   value came from.
//! - **Reads.** A selector picks resources as a rule's `paths:` and
//!   `scope_filter:` pick files, its manifest-derived predicates resolved
//!   here once per run. Each resource is read through the run's
//!   [`ReadCache`] inside the root and never in a Git directory, and decoded
//!   through its [`DecodeCache`], whose limits bound every decode
//!   (`eval/read.rs`).
//! - **Order of work.** A relation evaluates once per run: the relations it
//!   reads first, in an order where each comes after those it reads, then
//!   its rows, its `transform:` and its `where:` (`eval/ops.rs`).
//! - **Limits.** Every decode is bounded by its rows and the run's, and
//!   every row a relation builds, by its fields, takes its cells from the
//!   run's [`LimitKind::CellsPerRun`] before it is built, so a `union:` of
//!   many parts over one decode, or a join, fails at the cap. What a cell
//!   holds beyond its fixed size is shared or taken from the run's
//!   [`LimitKind::BytesPerRun`]: a value's text is shared by every field and
//!   join that copies it, a join shares its side's resources and a relation
//!   the failures of the one it read them from, and each decode, key path,
//!   path token, transformed value, note and failure the evaluator holds
//!   takes its bytes once. A run's relations then hold at most
//!   `cells_per_run` cells of at most 256 bytes each (a cell's share of its
//!   row, provenance and resource list included) and `bytes_per_run` bytes,
//!   whatever the input's shape; `eval/tests.rs` audits that bound. While
//!   one relation is evaluated, a step holds more for its length: a file's
//!   bytes, one decode before its bytes are taken, a join's key table
//!   (`join_inputs`), a `where:` list's set (`distinct_values`) and a
//!   transform's table of the texts it made (a row's worth each).
//! - **Failures.** A relation with a problem has no rows: it fails with an
//!   analysis failure per resource that shows one ([`RelationFailure`]), and
//!   so does every relation that reads it.

mod failure;
mod ops;
mod read;
mod spans;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use failure::Failures;

pub use failure::RelationFailure;
/// How a message shows a value, its type and a row's place, as the
/// evaluator's own messages do: for the `constraint` kind's findings.
pub use failure::{located, type_name};
pub use ops::render_value;
pub use read::path_token;
#[cfg(test)]
pub(crate) use spans::fingerprint;
pub(crate) use spans::fingerprint_text;
pub use spans::{BytesSpan, ReadSpan, find_spans, read_span_walks};

use super::cache::{DecodeCache, ReadCache};
use super::limits::{LimitExceeded, LimitKind, Limits};
use super::path::RelPath;
use super::plan::{Plan, RelationId};
use super::pointer::Pointer;
use super::resource::Resource;
use super::value::Value;
use crate::ByteSpan;
use crate::rule::Context;
use crate::walker::FileIndex;

/// A relation's rows, as [`evaluate`] returns them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationRows {
    /// The fields, in the order the relation writes them; every row has a
    /// cell for each, in this order.
    pub fields: Vec<FieldInfo>,
    pub rows: Vec<FieldRow>,
    /// Every resource the relation's parts read, rows or not, once each, in
    /// the order first read: a `set` grouped by path gives each a group. A
    /// join's are those of its side that is not unique, whose order its rows
    /// follow, shared with that side.
    pub resources: Arc<[Resource]>,
    /// What a `read:` skipped, in the order read: a computed value
    /// (`${VAR}`), one per value, and the values it matched that are not
    /// strings, one per resource and part, whose strings are still rows. A
    /// join keeps its sides' notes, and a relation those of the relations
    /// its `where:` lists read, since a value they skipped is a row they
    /// lack.
    pub notes: Vec<RelationNote>,
}

impl RelationRows {
    /// The position of field `name`, which indexes [`FieldRow::cells`].
    #[must_use]
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }
}

/// A relation's field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    /// Its value never reaches a message, the JSON report or a baseline:
    /// `sensitive: true`, a filter or transform of it the environment wrote,
    /// or, in a join, the field it copies being sensitive.
    pub sensitive: bool,
}

/// One row: a cell per field, and the entry or `read:` value the row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    pub cells: Vec<Cell>,
    /// Where the row itself came from: its entry, or its `read:` value. A
    /// `bound:` field's cell sits elsewhere, and so does a join's copy of a
    /// field of its unique side, which many rows share; a join row is the
    /// row of its side that is not unique, so it has that row's provenance.
    pub at: Arc<Provenance>,
}

/// One field's value in one row, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// `None` for a missing value: a `read:` that matched nothing in its
    /// resource. A missing value never joins, never forms a key and is in no
    /// set.
    pub value: Option<Value>,
    /// The resource and key path the value belongs to; the cells of one
    /// entry share it.
    pub at: Arc<Provenance>,
    /// Where the value is written: the key's span for `entry.key` (and for
    /// the `entry.parent`, `entry.root` and `entry.path` of the entry), the
    /// value's for `entry.value` and a `bound:` field (the key's for a YAML
    /// block scalar, which has none), and, for a `read:` value, wherever its
    /// reader places it, which is found only when a finding asks
    /// ([`Span::Read`]); none for a path token, a `whole_file` value and a
    /// missing value.
    pub span: Option<Span>,
    /// For an `entry.path` field, the key path's segments, after
    /// `strip-root`, whose [`Pointer::render`] the value is (until a string
    /// transform such as `lower` changes the value; the segments stay).
    /// Shared by the row's `entry.path` fields and their copies.
    pub path: Option<Arc<Pointer>>,
}

/// Where a cell's value is written ([`Cell::span`]). Placing a cell finds
/// it in the resource read again, and places the cell only where the text
/// still holds what it held when the relation read it ([`find_spans`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// These bytes of the cell's resource: an entry's key or value.
    Bytes(BytesSpan),
    /// Where the `read:` that read the value finds it in its resource
    /// ([`find_spans`]): a `lines:` value's trimmed line, a `regex:` value's
    /// capture, the value a JSON, YAML or TOML object member holds (its
    /// key's for a YAML block scalar). A value in an array, a repeated key's
    /// and a value of a file that is not UTF-8 are found nowhere. Reading
    /// finds no span, which would cost a structured read several times the
    /// read itself; a rule finds the spans of the values it places, one
    /// resource at a time.
    Read(ReadSpan),
}

impl Span {
    /// The bytes, when they are known without reading the resource again.
    #[must_use]
    pub const fn bytes(&self) -> Option<ByteSpan> {
        match self {
            Self::Bytes(bytes) => Some(bytes.bytes()),
            Self::Read(_) => None,
        }
    }
}

/// Where a row's value came from. A row is its resource and its pointer,
/// or, for a `read:` row, which has no pointer, its resource, its part and
/// its ordinal (design section 6, "Identity").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub resource: Resource,
    /// The entry's key path, or a `bound:` value's; none for a `read:` row.
    pub pointer: Option<Pointer>,
    /// The index of the part that read the row in its relation: its place
    /// in a `union:`, or 0.
    pub part: u32,
    /// The row's place among those the part's adapter emitted for the
    /// resource: the value's among the values a `read:` extracted, which a
    /// `where:` that drops other rows does not change (0 for a missing
    /// row).
    pub ordinal: u32,
    /// Whether the entry's value is null, an empty object or an empty array
    /// (false for a `read:` value and a `bound:` one). `strip-root` reads
    /// such a top-level key, a stub locale file's (`fr:`, `fr: {}`), as a
    /// file with no keys: the row is dropped, not a failure.
    pub empty: bool,
}

/// What a `read:` skipped in one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationNote {
    pub resource: Resource,
    pub skipped: Skipped,
    /// `skipped non-literal value "${VAR}"`, without the value when the
    /// field is sensitive; ``matched non-string value(s) (object: 1)`` with
    /// the `read:`, without it when the relation's configuration reads the
    /// environment. It holds no value that is not a string.
    pub message: String,
}

impl RelationNote {
    /// What tells notes apart: a relation that reads another (a join's side,
    /// or a list's relation) takes each of its notes once by this key, and a
    /// rule that reads both reports each once.
    #[must_use]
    pub fn key(&self) -> (RelPath, String, Option<u32>) {
        let part = match &self.skipped {
            Skipped::NonLiteral => None,
            Skipped::NonString { part, .. } => Some(*part),
        };
        (self.resource.path().clone(), self.message.clone(), part)
    }
}

/// What a [`RelationNote`] says a `read:` skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// A computed value (`${VAR}`).
    NonLiteral,
    /// The values the part at `part` (its place in a `union:`, or 0)
    /// matched that are not strings: each kind and how many times, null
    /// first, then booleans, numbers, arrays and objects. A rule reports
    /// them as one finding for the resource, as `cross_file` does.
    NonString {
        part: u32,
        kinds: Vec<(crate::extract::NonStringValueKind, usize)>,
    },
}

/// What evaluating one relation gave: its rows, or every analysis failure
/// that kept it from having any.
pub type Outcome = Result<Arc<RelationRows>, Arc<[RelationFailure]>>;

/// Evaluate relation `id` of `plan` over the tree `ctx` describes, once per
/// run: the first call evaluates it (and every relation it reads), and each
/// later call with the same plan returns what the first did.
///
/// The memo lives on `ctx.index`; inside a nested rule's iteration
/// ([`IterationScope`](super::IterationScope)) the evaluation uses one of
/// its own, as it uses read and decode caches of its own, and drops it on
/// return.
///
/// # Errors
///
/// Every analysis failure that keeps the relation from having rows: a
/// resource it cannot read or parse, a value of a type it cannot hold, a
/// limit reached, a repeated key on a join's unique side, and the failures
/// of the relations it reads. A rule reports each with
/// [`RelationFailure::to_violation`].
pub fn evaluate(plan: &Arc<Plan>, id: RelationId, ctx: &Context<'_>) -> Outcome {
    with_evaluator(plan, ctx, |evaluator| evaluator.relation(id))
}

/// Whether `where:` of relation `id` may keep a row of each resource its
/// rows list (`rows.resources`, in order), decided from each resource's path
/// alone: a predicate on a field whose value the path gives (`path` or a
/// path token, after the relation's transforms, or a join's copy of one from
/// the side whose resources it lists) is tested, and a predicate on any
/// other field is unknown. `where:` keeps a row only when all its predicates
/// hold, so a resource is dropped (`false`) only when a predicate it tests is
/// false; one it cannot test (a type error) or one on another field keeps
/// it. A `{ relation: R, field: F }` list is read as the relation's own
/// `where:` reads it. A relation that does not evaluate keeps every
/// resource: its rule reports the failure.
#[must_use]
pub fn resources_kept(
    plan: &Arc<Plan>,
    id: RelationId,
    rows: &RelationRows,
    ctx: &Context<'_>,
) -> Vec<bool> {
    with_evaluator(plan, ctx, |evaluator| match evaluator.relation(id) {
        Ok(_) => evaluator.resources_kept(id, &rows.resources),
        Err(_) => vec![true; rows.resources.len()],
    })
}

/// Run `then` with an evaluator over the run's memo and caches (or, inside a
/// nested rule's iteration, ones of its own).
fn with_evaluator<T>(
    plan: &Arc<Plan>,
    ctx: &Context<'_>,
    then: impl FnOnce(&Evaluator<'_>) -> T,
) -> T {
    // The run's caches, so a resource another relation or rule reads is read
    // and decoded once; inside an iteration they stay out of reach.
    ctx.index.enable_relation_caches();
    let memo = ctx.index.relation_memo(plan);
    let local;
    let memo = if let Some(memo) = &memo {
        memo.as_ref()
    } else {
        local = RunMemo::new(Arc::clone(plan));
        &local
    };
    let local_reads;
    let reads = if let Some(cache) = ctx.index.read_cache() {
        cache
    } else {
        local_reads = ReadCache::new();
        &local_reads
    };
    let local_decodes;
    let decodes = if let Some(cache) = ctx.index.decode_cache() {
        cache
    } else {
        local_decodes = DecodeCache::new();
        &local_decodes
    };
    then(&Evaluator {
        plan,
        root: ctx.root,
        index: ctx.index,
        limits: Limits::default(),
        reads,
        decodes,
        memo,
    })
}

/// Whether a field that holds keys (`entry.key`, `entry.parent`,
/// `entry.root`, `entry.path`) is sensitive in relation `id` of `plan` or a
/// relation it reads, transitively: then a key path in a message about its
/// rows could be that field's value, so none is shown.
#[must_use]
pub fn keys_sensitive(plan: &Plan, id: RelationId) -> bool {
    use super::plan::FieldSource;
    let mut visited = vec![false; plan.relations.len()];
    let mut stack = vec![id];
    while let Some(next) = stack.pop() {
        if std::mem::replace(&mut visited[next.0], true) {
            continue;
        }
        let relation = &plan.relation(next).relation;
        if relation.fields.iter().any(|(_, field)| {
            field.sensitive
                && matches!(
                    field.source,
                    FieldSource::EntryKey
                        | FieldSource::EntryParent
                        | FieldSource::EntryRoot
                        | FieldSource::EntryPath
                )
        }) {
            return true;
        }
        stack.extend(ops::reads(relation));
    }
    false
}

/// The relations one run has evaluated, kept on the [`FileIndex`] and set by
/// the first evaluation. It belongs to one plan: an evaluation with another
/// plan uses a memo of its own.
#[derive(Debug)]
pub struct RunMemo {
    plan: Arc<Plan>,
    outcomes: Vec<OnceLock<Outcome>>,
    /// Each manifest-derived `scope_filter:` predicate's set, by the key the
    /// engine resolves rules' predicates under, resolved once per run when
    /// the engine has not.
    manifests: Mutex<std::collections::HashMap<String, read::ManifestOutcome>>,
    /// The cells this memo's evaluations have built
    /// ([`LimitKind::CellsPerRun`]); a failed take still counts.
    cells: AtomicUsize,
    /// The bytes they hold beyond their cells ([`LimitKind::BytesPerRun`]);
    /// a failed take still counts.
    bytes: AtomicUsize,
    /// The relations evaluated, in the order they were. Test only.
    #[cfg(test)]
    computed: Mutex<Vec<RelationId>>,
}

impl RunMemo {
    pub(crate) fn new(plan: Arc<Plan>) -> Self {
        let outcomes = plan.relations.iter().map(|_| OnceLock::new()).collect();
        Self {
            plan,
            outcomes,
            manifests: Mutex::default(),
            cells: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            #[cfg(test)]
            computed: Mutex::default(),
        }
    }

    pub(crate) fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    /// Whether the memo holds `id`'s outcome: an evaluation's order of work
    /// leaves out a relation it holds.
    #[must_use]
    pub fn evaluated(&self, id: RelationId) -> bool {
        self.outcomes[id.0].get().is_some()
    }

    /// The cells taken so far. Test only.
    #[cfg(test)]
    pub(crate) fn cells(&self) -> usize {
        self.cells.load(Ordering::Relaxed)
    }

    /// The bytes taken so far ([`LimitKind::BytesPerRun`]): what the engine
    /// records, with the decode cache's, as what the relations took
    /// ([`DecodeCache::record_relations`](super::DecodeCache::record_relations)).
    pub(crate) fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Whether an evaluation read a `scope_filter:` manifest itself, from
    /// its path on disk, rather than finding the engine's set: a file a fix
    /// creates may be that manifest, though the index does not list it.
    pub(crate) fn read_manifests(&self) -> bool {
        !self.manifests.lock().expect("manifest set lock").is_empty()
    }

    /// The relations evaluated so far, in the order they were. Test only.
    #[cfg(test)]
    pub(crate) fn computed(&self) -> Vec<RelationId> {
        self.computed.lock().expect("the memo's order lock").clone()
    }
}

/// One evaluation's view of the run: the plan, the tree, the limits and the
/// caches every read and decode goes through.
pub(crate) struct Evaluator<'a> {
    plan: &'a Plan,
    root: &'a Path,
    index: &'a FileIndex,
    limits: Limits,
    reads: &'a ReadCache,
    decodes: &'a DecodeCache,
    memo: &'a RunMemo,
}

impl Evaluator<'_> {
    /// Take `cells` cells from the run's [`LimitKind::CellsPerRun`], before
    /// building what holds them. A failed take still counts, so every later
    /// take fails too.
    fn take_cells(&self, cells: usize) -> Result<(), LimitExceeded> {
        take(
            &self.memo.cells,
            cells,
            &self.limits,
            LimitKind::CellsPerRun,
        )
    }

    /// Take `bytes` bytes from the run's [`LimitKind::BytesPerRun`] for what
    /// a relation holds beyond its cells, as it builds it. A failed take
    /// still counts, so every later take fails too.
    fn take_bytes(&self, bytes: usize) -> Result<(), LimitExceeded> {
        take(
            &self.memo.bytes,
            bytes,
            &self.limits,
            LimitKind::BytesPerRun,
        )
    }

    /// `id`'s outcome, evaluating it and the relations it reads first when
    /// the memo does not hold it yet.
    fn relation(&self, id: RelationId) -> Outcome {
        for next in self.order(id) {
            self.memo.outcomes[next.0].get_or_init(|| {
                #[cfg(test)]
                self.memo
                    .computed
                    .lock()
                    .expect("the memo's order lock")
                    .push(next);
                self.compute(next)
            });
        }
        self.memo.outcomes[id.0]
            .get()
            .expect("the order ends with the relation itself")
            .clone()
    }

    /// `id` and the relations it reads, transitively, each after those it
    /// reads; those the memo holds are left out ([`Plan::post_order`]).
    fn order(&self, id: RelationId) -> Vec<RelationId> {
        let mut order = Vec::new();
        let mut visited = vec![false; self.plan.relations.len()];
        self.plan.post_order(
            id,
            &mut visited,
            &|next| self.memo.evaluated(next),
            &mut order,
        );
        order
    }

    /// [`keys_sensitive`] of relation `id`.
    fn keys_sensitive(&self, id: RelationId) -> bool {
        keys_sensitive(self.plan, id)
    }

    /// The outcome of a relation whose reads the memo holds.
    fn computed(&self, id: RelationId) -> Outcome {
        self.memo.outcomes[id.0]
            .get()
            .cloned()
            .expect("a relation is evaluated after the relations it reads")
    }

    fn compute(&self, id: RelationId) -> Outcome {
        let named = self.plan.relation(id);
        let site = Site {
            prefix: format!(
                "relation `{}` ({})",
                named.name,
                self.plan.source(named.source).display
            ),
            sensitive: named.reads_environment,
            keys_sensitive: self.keys_sensitive(id),
        };
        let mut failures = Failures::default();
        let rows = match &named.relation.rows {
            super::plan::Rows::Join(join) => self.join(&site, named, join, &mut failures),
            super::plan::Rows::Part(part) => {
                Some(self.parts(&site, named, std::slice::from_ref(part), &mut failures))
            }
            super::plan::Rows::Union(parts) => Some(self.parts(&site, named, parts, &mut failures)),
        };
        let rows = rows.and_then(|rows| self.refine(&site, named, rows, &mut failures));
        match rows {
            Some(mut rows) if failures.is_empty() => {
                // The memo holds the rows for the run: with no spare room,
                // what they hold stays within their cells' budget however
                // few rows a relation has.
                rows.rows.shrink_to_fit();
                Ok(Arc::new(rows))
            }
            None if failures.is_empty() => {
                // The plan check rules this out: a field the relation names
                // that a row does not have. Never an empty failure list.
                failures.push(site.failure(
                    crate::rule::AnalysisFailureKind::Evaluation,
                    None,
                    "the relation names a field its rows do not have; this is a bug in alint's \
                     plan check, so please report it",
                ));
                Err(self.held_failures(&site, failures))
            }
            _ => Err(self.held_failures(&site, failures)),
        }
    }

    /// The failures a relation keeps in the memo, their bytes taken from the
    /// run: those of the one relation it read them from are shared and take
    /// none. When the bytes run out, the relation keeps one failure that says
    /// a limit was reached: its own, or this one.
    fn held_failures(&self, site: &Site, failures: Failures) -> Arc<[RelationFailure]> {
        match self.take_bytes(failures.bytes()) {
            Ok(()) => failures.into_shared(),
            Err(limit) => Arc::new([failures.into_limit().unwrap_or_else(|| {
                site.failure(
                    crate::rule::AnalysisFailureKind::Evaluation,
                    None,
                    limit_reached(&limit),
                )
            })]),
        }
    }
}

/// What a key path held in an [`Arc`] holds: the [`Arc`]'s counts, the
/// pointer and its segments.
fn shared_pointer_bytes(pointer: &Pointer) -> usize {
    2 * std::mem::size_of::<usize>() + std::mem::size_of::<Pointer>() + pointer.heap_bytes()
}

/// Add `amount` to `used`, failing once the total exceeds `kind`'s limit. A
/// take of nothing holds nothing, so it never fails.
fn take(
    used: &AtomicUsize,
    amount: usize,
    limits: &Limits,
    kind: LimitKind,
) -> Result<(), LimitExceeded> {
    if amount == 0 {
        return Ok(());
    }
    let total = used
        .fetch_add(amount, Ordering::Relaxed)
        .saturating_add(amount);
    limits.check(kind, total)
}

/// What a relation says when the run's cells or bytes run out
/// ([`Evaluator::take_cells`], [`Evaluator::take_bytes`]), or a decode's or
/// the run's decoded rows do.
fn limit_reached(limit: &LimitExceeded) -> String {
    match limit.kind {
        LimitKind::BytesPerRun => format!(
            "{limit}, so the relation is not evaluated rather than cut short; a run's relations \
             hold each file they decode and each key path, path token and transformed value \
             they build, so read fewer or smaller files (narrow the selector or the `at:` paths) \
             or bind fewer key and path fields"
        ),
        LimitKind::CellsPerRun => format!(
            "{limit}, so the relation is not evaluated rather than cut short; a run's relations \
             hold a cell per field of each row and one per file each part reads, so read fewer \
             rows (narrow the selector, the `at:` paths or the parts) or bind fewer fields"
        ),
        _ => format!(
            "{limit}, so the relation is not evaluated rather than cut short; narrow what it \
             reads (its selector, or its `at:` paths)"
        ),
    }
}

/// How one relation's failures read: their common prefix, whether its
/// configuration reads the environment, and whether they may show key
/// paths.
pub(crate) struct Site {
    prefix: String,
    sensitive: bool,
    /// Key paths are hidden ([`Evaluator::keys_sensitive`]).
    keys_sensitive: bool,
}

impl Site {
    /// A failure of this relation, at `resource`. A relation whose
    /// configuration reads the environment shows none of the detail: the
    /// key path and what the configuration selected may echo the value.
    fn failure(
        &self,
        kind: crate::rule::AnalysisFailureKind,
        resource: Option<Arc<Path>>,
        detail: impl std::fmt::Display,
    ) -> RelationFailure {
        let message = if self.sensitive {
            format!(
                "{}: this relation's configuration reads the environment, so the detail of its \
                 analysis failure is not shown",
                self.prefix
            )
        } else {
            format!("{}: {detail}", self.prefix)
        };
        RelationFailure {
            kind,
            resource,
            message: message.into(),
            sensitive: self.sensitive,
        }
    }
}

/// The test seam: evaluate with explicit limits and caches, and a memo of the
/// caller's.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_with(
    plan: &Arc<Plan>,
    id: RelationId,
    root: &Path,
    index: &FileIndex,
    limits: Limits,
    reads: &ReadCache,
    decodes: &DecodeCache,
    memo: &RunMemo,
) -> Outcome {
    Evaluator {
        plan,
        root,
        index,
        limits,
        reads,
        decodes,
        memo,
    }
    .relation(id)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
