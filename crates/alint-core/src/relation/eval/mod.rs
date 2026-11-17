//! Executing relations (design `v0.23/selectors-and-constraints.md`, sections
//! 5 to 8; ADR-0034 decisions 5 to 7, as ADR-0037 decision 3 corrects them):
//! a [`Plan`]'s relation and a walked tree in, the relation's rows out.
//!
//! [`evaluate`] is what the `constraint` kind (phase 7) calls with a rule's
//! [`Context`]. Nothing else calls it in this build, so a version 2
//! configuration's relations stay inert: a run whose rules read no relation
//! evaluates none, reads nothing for one and allocates nothing for one (the
//! memo on the [`FileIndex`] is set by the first evaluation).
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
//! - **Failures.** A relation with a problem has no rows: it fails with an
//!   analysis failure per resource that shows one ([`RelationFailure`]), and
//!   so does every relation that reads it.

mod failure;
mod ops;
mod read;

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub use failure::RelationFailure;

use super::cache::{DecodeCache, ReadCache};
use super::limits::Limits;
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
    /// Every resource the relation's parts read, rows or not, in the order
    /// read: a `set` grouped by path gives each a group. A join's are those
    /// of its side that is not unique, whose order its rows follow.
    pub resources: Vec<Resource>,
    /// What a `read:` skipped: a computed value (`${VAR}`), one per value,
    /// in the order read.
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

/// One row: a cell per field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    pub cells: Vec<Cell>,
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
    /// block scalar, which has none), none for a path token or a `read:`
    /// value.
    pub span: Option<ByteSpan>,
    /// For an `entry.path` field, the key path's segments, after
    /// `strip-root`, whose [`Pointer::render`] the value is (until a string
    /// transform such as `lower` changes the value; the segments stay).
    pub path: Option<Pointer>,
}

/// Where a row's value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub resource: Resource,
    /// The entry's key path, or a `bound:` value's; none for a `read:` row.
    pub pointer: Option<Pointer>,
}

/// What a `read:` skipped in one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationNote {
    pub resource: Resource,
    /// `skipped non-literal value "${VAR}"`, without the value when the
    /// field is sensitive.
    pub message: String,
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
    Evaluator {
        plan,
        root: ctx.root,
        index: ctx.index,
        limits: Limits::default(),
        reads,
        decodes,
        memo,
    }
    .relation(id)
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
}

impl RunMemo {
    pub(crate) fn new(plan: Arc<Plan>) -> Self {
        let outcomes = plan.relations.iter().map(|_| OnceLock::new()).collect();
        Self {
            plan,
            outcomes,
            manifests: Mutex::default(),
        }
    }

    pub(crate) fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    /// Whether the memo holds `id`'s outcome. Test/diagnostic only.
    #[must_use]
    pub fn evaluated(&self, id: RelationId) -> bool {
        self.outcomes[id.0].get().is_some()
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
    /// `id`'s outcome, evaluating it and the relations it reads first when
    /// the memo does not hold it yet.
    fn relation(&self, id: RelationId) -> Outcome {
        for next in self.order(id) {
            self.memo.outcomes[next.0].get_or_init(|| self.compute(next));
        }
        self.memo.outcomes[id.0]
            .get()
            .expect("the order ends with the relation itself")
            .clone()
    }

    /// `id` and the relations it reads, transitively, each after those it
    /// reads; those the memo holds are left out. The walk keeps its own
    /// stack, so a long chain of joins cannot exhaust the thread's, and it
    /// visits each relation once (the plan check refused cycles).
    fn order(&self, id: RelationId) -> Vec<RelationId> {
        let mut order = Vec::new();
        let mut visited = vec![false; self.plan.relations.len()];
        let mut stack = vec![(id, false)];
        while let Some((next, expanded)) = stack.pop() {
            if expanded {
                order.push(next);
                continue;
            }
            if std::mem::replace(&mut visited[next.0], true) || self.memo.evaluated(next) {
                continue;
            }
            stack.push((next, true));
            for read in ops::reads(&self.plan.relation(next).relation)
                .into_iter()
                .rev()
            {
                if !visited[read.0] {
                    stack.push((read, false));
                }
            }
        }
        order
    }

    /// Whether a field that holds keys (`entry.key`, `entry.parent`,
    /// `entry.root`, `entry.path`) is sensitive in `id` or a relation it
    /// reads, transitively: then a key path in its messages could be that
    /// field's value, so none is shown.
    fn keys_sensitive(&self, id: RelationId) -> bool {
        use super::plan::FieldSource;
        let mut visited = vec![false; self.plan.relations.len()];
        let mut stack = vec![id];
        while let Some(next) = stack.pop() {
            if std::mem::replace(&mut visited[next.0], true) {
                continue;
            }
            let relation = &self.plan.relation(next).relation;
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
        let mut failures = failure::Failures::default();
        let rows = match &named.relation.rows {
            super::plan::Rows::Join(join) => self.join(&site, named, join, &mut failures),
            super::plan::Rows::Part(part) => {
                Some(self.parts(&site, named, std::slice::from_ref(part), &mut failures))
            }
            super::plan::Rows::Union(parts) => Some(self.parts(&site, named, parts, &mut failures)),
        };
        let rows = rows.and_then(|rows| self.refine(&site, named, rows, &mut failures));
        match rows {
            Some(rows) if failures.is_empty() => Ok(Arc::new(rows)),
            None if failures.is_empty() => {
                // The plan check rules this out: a field the relation names
                // that a row does not have. Never an empty failure list.
                failures.push(site.failure(
                    crate::rule::AnalysisFailureKind::Evaluation,
                    None,
                    "the relation names a field its rows do not have; this is a bug in alint's \
                     plan check, so please report it",
                ));
                Err(failures.into_vec().into())
            }
            _ => Err(failures.into_vec().into()),
        }
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
            message,
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
