//! A document's key paths, each held once as its parent and its last
//! segment, and the entries an extraction walks.
//!
//! A walk that copied its whole key path into every row held W x D segments
//! for a document W keys wide at depth D: a 1 MB JSON file 120 objects deep
//! with 100,000 keys at the bottom took most of a gigabyte before an `at:`
//! path selected anything. Here the walk's rows name a node, a path shares
//! its prefix with every path below it, and a [`Pointer`] is built only for
//! a row an adapter emits, its bytes charged to the run as it is built
//! ([`PathBudget`]).

use std::cell::Cell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::limits::{LimitExceeded, LimitKind, Limits, RunBudget};
use super::pointer::{Pointer, Segment};
use crate::extract::{ByteSpan, EntryValue};

/// A key path in a [`KeyPaths`]: the document root, or a path one segment
/// longer than its parent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PathId(u32);

impl PathId {
    /// The document root: the empty key path.
    pub(crate) const ROOT: Self = Self(0);

    /// The path's number, which indexes a table of one entry per path.
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug)]
struct Node {
    parent: PathId,
    /// The path's last segment; the root's is never read.
    segment: Segment,
    depth: usize,
    /// What the path's [`Pointer`] holds on the heap
    /// ([`Pointer::heap_bytes`]).
    bytes: usize,
}

/// The key paths of one document, each once.
///
/// A path is numbered when a walk first reaches it, and a walk reaches a
/// member's path after the paths of the members written before it and before
/// every path below it, so the numbers are the paths' document order. Two
/// members with one key path (a repeated key) share its node, as they share
/// their key path.
#[derive(Debug)]
pub(crate) struct KeyPaths {
    nodes: Vec<Node>,
    children: HashMap<(PathId, Segment), PathId>,
}

impl Default for KeyPaths {
    fn default() -> Self {
        Self {
            nodes: vec![Node {
                parent: PathId::ROOT,
                segment: Segment::Index(0),
                depth: 0,
                bytes: 0,
            }],
            children: HashMap::new(),
        }
    }
}

impl KeyPaths {
    /// The path `parent` extended by `segment`, numbered the first time.
    pub(crate) fn child(&mut self, parent: PathId, segment: Segment) -> PathId {
        let next = self.nodes.len();
        let nodes = &mut self.nodes;
        match self.children.entry((parent, segment)) {
            Entry::Occupied(held) => *held.get(),
            Entry::Vacant(slot) => {
                // A walk numbers at most one path per entry and array element
                // it reads, and a document alint parses is far below 2^32 of
                // either.
                let id = PathId(u32::try_from(next).expect("fewer than 2^32 key paths"));
                let segment = slot.key().1.clone();
                let holder = &nodes[parent.index()];
                let key = match &segment {
                    Segment::Key(key) => key.len(),
                    Segment::Index(_) => 0,
                };
                let node = Node {
                    parent,
                    depth: holder.depth + 1,
                    bytes: holder.bytes + std::mem::size_of::<Segment>() + key,
                    segment,
                };
                nodes.push(node);
                slot.insert(id);
                id
            }
        }
    }

    /// The path `parent` extended by `segment`, when some walk reached it.
    pub(crate) fn get(&self, parent: PathId, segment: Segment) -> Option<PathId> {
        self.children.get(&(parent, segment)).copied()
    }

    /// How many paths there are, the root included: one past the highest
    /// [`PathId`].
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    /// The path of the object that holds `id`'s last segment; the root's is
    /// the root.
    pub(crate) fn parent(&self, id: PathId) -> PathId {
        self.nodes[id.index()].parent
    }

    /// What [`Self::pointer`] of `id` holds on the heap, without building it.
    pub(crate) fn pointer_bytes(&self, id: PathId) -> usize {
        self.nodes[id.index()].bytes
    }

    /// `id`'s segments, from the root's first.
    pub(crate) fn segments(&self, id: PathId) -> Vec<&Segment> {
        let mut segments = Vec::with_capacity(self.nodes[id.index()].depth);
        let mut at = id;
        while at != PathId::ROOT {
            let node = &self.nodes[at.index()];
            segments.push(&node.segment);
            at = node.parent;
        }
        segments.reverse();
        segments
    }

    /// Roughly what the paths hold on the heap: each node, its entry in
    /// the map of children, and its key's text, held by both.
    pub(crate) fn heap_bytes(&self) -> usize {
        let keys: usize = self
            .nodes
            .iter()
            .map(|node| match &node.segment {
                Segment::Key(key) => key.len(),
                Segment::Index(_) => 0,
            })
            .sum();
        self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.children.capacity() * (std::mem::size_of::<((PathId, Segment), PathId)>() + 1)
            + 2 * keys
    }

    /// `id` as a pointer of its own.
    pub(crate) fn pointer(&self, id: PathId) -> Pointer {
        Pointer::from_segments(self.segments(id).into_iter().cloned().collect())
    }
}

/// Where a walk is: the steps from the root to the value it reads. A step
/// is numbered as a path when a row at or below it needs one, so the
/// elements of an array of scalars, which no row is below, take none.
#[derive(Debug, Default)]
pub(crate) struct PathStack {
    steps: Vec<Step>,
}

#[derive(Debug)]
enum Step {
    Path(PathId),
    Pending(Segment),
}

impl PathStack {
    /// Step into `segment`, numbered when a row needs it.
    pub(crate) fn enter(&mut self, segment: Segment) {
        self.steps.push(Step::Pending(segment));
    }

    /// Step into the path `id`, numbered already (a member's row's own).
    pub(crate) fn enter_path(&mut self, id: PathId) {
        self.steps.push(Step::Path(id));
    }

    pub(crate) fn leave(&mut self) {
        self.steps.pop();
    }

    /// The path of where the walk is, numbering it and the steps above it
    /// the first time. Each step is numbered once, so a walk spends one
    /// number per step whatever its depth.
    pub(crate) fn path(&mut self, paths: &mut KeyPaths) -> PathId {
        let first = self
            .steps
            .iter()
            .rposition(|step| matches!(step, Step::Path(_)))
            .map_or(0, |numbered| numbered + 1);
        let mut at = match first.checked_sub(1).map(|numbered| &self.steps[numbered]) {
            Some(Step::Path(id)) => *id,
            _ => PathId::ROOT,
        };
        for step in &mut self.steps[first..] {
            if let Step::Pending(segment) = std::mem::replace(step, Step::Path(PathId::ROOT)) {
                at = paths.child(at, segment);
            }
            *step = Step::Path(at);
        }
        at
    }
}

/// One member of a document, as a walk reads it: its key path, whose last
/// segment is its key, and its spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryAt {
    pub(crate) path: PathId,
    pub(crate) key_span: ByteSpan,
    /// The value's byte range, when the format gives one.
    pub(crate) value_span: Option<ByteSpan>,
}

/// A document's members in source order, repeated keys kept, with the key
/// paths they name and, when the walk was asked, each one's value
/// (`values[i]` is `rows[i]`'s).
#[derive(Debug, Default)]
pub(crate) struct EntryTable {
    pub(crate) paths: KeyPaths,
    pub(crate) rows: Vec<EntryAt>,
    pub(crate) values: Vec<EntryValue>,
    /// Roughly what the parse the walk read held on the heap, by its nodes
    /// (YAML's events, TOML's document; not counted for JSON, whose
    /// members placing reads from its syntax tree without a table): let go
    /// once the table is built, it was held with the table while the walk
    /// built it.
    pub(crate) parsed: usize,
}

/// The key-path bytes a decode builds, charged as it builds each [`Pointer`]
/// against the run's [`LimitKind::BytesPerRun`] budget (`run`) and, for
/// `key_parity`'s key rows ([`Self::for_key_rows`]), against
/// [`LimitKind::KeyPathBytesPerDecode`] (default 32 MiB) so a single deep, wide
/// file fails early rather than holding hundreds of megabytes under the 1 GiB
/// run cap.
#[derive(Debug)]
pub(crate) struct PathBudget<'b> {
    run: &'b RunBudget,
    decode_used: Cell<usize>,
    decode_limit: Option<usize>,
}

impl<'b> PathBudget<'b> {
    pub(crate) fn new(run: &'b RunBudget) -> Self {
        Self {
            run,
            decode_used: Cell::new(0),
            decode_limit: None,
        }
    }

    pub(crate) fn for_key_rows(run: &'b RunBudget, limits: &Limits) -> Self {
        Self {
            run,
            decode_used: Cell::new(0),
            decode_limit: Some(limits.get(LimitKind::KeyPathBytesPerDecode)),
        }
    }

    /// Charge `bytes`, failing once either the per-decode key-row cap (when
    /// enabled) or the run's byte budget is exceeded.
    pub(crate) fn take(&self, bytes: usize) -> Result<(), LimitExceeded> {
        if let Some(limit) = self.decode_limit {
            let next = self.decode_used.get().saturating_add(bytes);
            if next > limit {
                return Err(LimitExceeded {
                    kind: LimitKind::KeyPathBytesPerDecode,
                    limit,
                    reached: next,
                });
            }
            self.run.take(bytes)?;
            self.decode_used.set(next);
            return Ok(());
        }
        self.run.take(bytes)
    }

    /// `id`'s pointer, its bytes charged first.
    pub(crate) fn pointer(&self, paths: &KeyPaths, id: PathId) -> Result<Pointer, LimitExceeded> {
        self.take(paths.pointer_bytes(id))?;
        Ok(paths.pointer(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::limits::{LimitKind, Limits};

    #[test]
    fn a_path_is_held_once_and_numbered_in_the_order_reached() {
        let mut paths = KeyPaths::default();
        let a = paths.child(PathId::ROOT, Segment::Key("a".into()));
        let b = paths.child(a, Segment::Key("b".into()));
        let again = paths.child(PathId::ROOT, Segment::Key("a".into()));
        assert_eq!(a, again, "a repeated key shares its path");
        assert!(a < b);
        assert_eq!(paths.len(), 3);
        assert_eq!(paths.pointer(b).render(), "a.b");
        assert_eq!(paths.pointer_bytes(b), paths.pointer(b).heap_bytes());
        assert_eq!(paths.parent(b), a);
        assert_eq!(paths.get(a, Segment::Key("b".into())), Some(b));
        assert_eq!(paths.get(PathId::ROOT, Segment::Key("b".into())), None);
    }

    #[test]
    fn a_stack_numbers_a_step_only_when_a_row_needs_it() {
        let mut paths = KeyPaths::default();
        let mut stack = PathStack::default();
        let list = paths.child(PathId::ROOT, Segment::Key("list".into()));
        stack.enter_path(list);
        for element in 0..1000 {
            stack.enter(Segment::Index(element));
            stack.leave();
        }
        assert_eq!(paths.len(), 2, "no element holds a row");
        stack.enter(Segment::Index(7));
        stack.enter(Segment::Index(3));
        let inner = stack.path(&mut paths);
        assert_eq!(paths.pointer(inner).render(), "list[7][3]");
        assert_eq!(stack.path(&mut paths), inner, "numbered once");
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn a_budget_fails_once_the_decodes_key_paths_exceed_it() {
        let mut paths = KeyPaths::default();
        let deep = (0..10).fold(PathId::ROOT, |at, _| {
            paths.child(at, Segment::Key("k".into()))
        });
        let limits = Limits::default()
            .with(LimitKind::BytesPerRun, 3 * paths.pointer_bytes(deep))
            .unwrap();
        let run = RunBudget::bytes(&limits);
        let budget = PathBudget::new(&run);
        for _ in 0..3 {
            budget.pointer(&paths, deep).unwrap();
        }
        let limit = budget.pointer(&paths, deep).unwrap_err();
        assert_eq!(limit.kind, LimitKind::BytesPerRun);
        assert_eq!(limit.reached, 4 * paths.pointer_bytes(deep));

        let per_decode = Limits::default()
            .with(
                LimitKind::KeyPathBytesPerDecode,
                2 * paths.pointer_bytes(deep),
            )
            .unwrap();
        let run_default = RunBudget::bytes(&per_decode);
        let decode_budget = PathBudget::for_key_rows(&run_default, &per_decode);
        for _ in 0..2 {
            decode_budget.pointer(&paths, deep).unwrap();
        }
        let decode_limit = decode_budget.pointer(&paths, deep).unwrap_err();
        assert_eq!(decode_limit.kind, LimitKind::KeyPathBytesPerDecode);
        assert_eq!(decode_limit.reached, 3 * paths.pointer_bytes(deep));
    }
}
