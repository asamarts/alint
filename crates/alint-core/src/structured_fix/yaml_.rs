use super::PathSeg;
use saphyr::{Scalar, ScalarOwned};
use saphyr_parser::{Event, Parser, ScalarStyle, Span, Tag};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// The deepest collection nesting the resolver reads, block and flow levels
/// alike: the check side's limit, past which ayaml refuses the document too.
const MAX_DEPTH: usize = crate::yaml_depth::MAX_YAML_FLOW_DEPTH;

std::thread_local! {
    /// How many documents [`SpanIndex::read`] has parsed on this thread.
    static READS: Cell<usize> = const { Cell::new(0) };
}

/// How many YAML documents this thread has parsed for spans.
pub(super) fn reads() -> usize {
    READS.with(Cell::get)
}

/// The byte range of the scalar value at `path` (see [`SpanIndex::value_span`]).
/// `None` if the source does not parse, is a multi-document stream, nests past
/// [`MAX_DEPTH`], or the path does not resolve to a scalar.
pub(super) fn yaml_value_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
    SpanIndex::read(text)?.value_span(path)
}

/// [`yaml_value_span`] for each of `paths`, from one parse of `text`.
pub(super) fn yaml_value_spans(text: &str, paths: &[Vec<PathSeg>]) -> Vec<Option<Range<usize>>> {
    SpanIndex::read(text).map_or_else(
        || vec![None; paths.len()],
        |index| paths.iter().map(|path| index.value_span(path)).collect(),
    )
}

/// The byte range `remove_value` deletes for the block-mapping entry at `path`
/// (see [`SpanIndex::removal`]). `None` for a block mapping's only entry,
/// which a removal replaces with `{}` rather than deletes (see
/// [`yaml_removal_edits`]).
pub(super) fn yaml_removal_span(text: &str, path: &[PathSeg]) -> Option<Range<usize>> {
    SpanIndex::read(text)?.removal_span(path)
}

/// [`yaml_removal_span`] for each of `paths`, from one parse of `text`.
pub(super) fn yaml_removal_spans(text: &str, paths: &[Vec<PathSeg>]) -> Vec<Option<Range<usize>>> {
    SpanIndex::read(text).map_or_else(
        || vec![None; paths.len()],
        |index| paths.iter().map(|path| index.removal_span(path)).collect(),
    )
}

/// The edit `remove_value` makes for each of `paths`, from one parse of
/// `text`: the entry's line deleted, but where the paths remove every entry
/// of a block mapping, whose last entry becomes `{}` so the mapping stays one
/// (an empty block mapping reads as null; JSON keeps `{}`, TOML the table).
pub(super) fn yaml_removal_edits(
    text: &str,
    paths: &[Vec<PathSeg>],
) -> Vec<Option<(Range<usize>, &'static str)>> {
    SpanIndex::read(text).map_or_else(
        || vec![None; paths.len()],
        |index| index.removal_edits(paths),
    )
}

/// Serialize a scalar as YAML bytes. A JSON scalar is valid YAML with the same
/// value + type (YAML is a superset of JSON): a number/bool renders bare, `null`
/// as `null`, a string double-quoted + escaped -- and it re-parses to the same
/// value so the `equals` re-verify holds. `None` for a non-scalar (routed to a
/// Suggestion).
pub(super) fn yaml_serialize_scalar(value: &serde_json::Value) -> Option<Vec<u8>> {
    use serde_json::Value as J;
    match value {
        J::String(_) | J::Number(_) | J::Bool(_) | J::Null => serde_json::to_vec(value).ok(),
        J::Array(_) | J::Object(_) => None,
    }
}

/// One node of the document as the event walk records it. Collections hold
/// node indices, so neither building nor dropping the tree recurses, and an
/// alias stays a node of its own instead of a copy of the node it names.
enum Node {
    Scalar(ScalarNode),
    /// An alias, with the node its anchor names when that node was complete
    /// at the alias (saphyr's loader resolves only those).
    Alias {
        span: Range<usize>,
        target: Option<usize>,
    },
    /// A sequence and its elements. Not `plain` under a tag outside the core
    /// schema, which saphyr's loader reads as a tagged node, not a sequence.
    Sequence {
        elements: Vec<usize>,
        plain: bool,
    },
    /// A mapping; its scalar keys are in [`SpanIndex::keys`]. `plain` as for
    /// a sequence.
    Mapping {
        plain: bool,
        /// How many entries it holds, whatever their keys.
        entries: usize,
        /// The character where its start event's span ends: past the `{` of
        /// a flow mapping.
        opened: usize,
    },
}

struct ScalarNode {
    /// The event's character span: the anchor and the tag, then the scalar.
    span: Range<usize>,
    style: ScalarStyle,
    /// Whether saphyr's loader reads the scalar as a value: not one under a
    /// tag outside the core schema (a tagged node), nor one its core tag
    /// cannot read (`!!int x`, a bad value).
    value: bool,
    /// The scalar's value as a mapping key, kept on a key and on an anchored
    /// scalar an alias key repeats. `None` where saphyr's loader does not read
    /// a value (see [`Self::value`]).
    key: Option<ScalarOwned>,
    /// What the parser read, kept when an anchor or a tag precedes it, to
    /// check that skipping them lands on the scalar.
    read: Option<String>,
    anchored: bool,
    tagged: bool,
}

/// One block-mapping entry a removal deletes ([`SpanIndex::removal`]), in
/// bytes of [`SpanIndex::text`].
struct Removal {
    parent: usize,
    key: usize,
    /// The entry's whole line, through its line break.
    line: Range<usize>,
    /// The entry itself, from its key's properties to its value's end.
    entry: Range<usize>,
}

/// One open collection of the walk.
struct Frame {
    node: usize,
    anchor: usize,
    /// In a mapping, the key whose value comes next.
    key: Option<usize>,
}

/// The state of one pass over the events.
#[derive(Default)]
struct Walk {
    nodes: Vec<Node>,
    stack: Vec<Frame>,
    keys: HashMap<(usize, ScalarOwned), (usize, usize)>,
    anchors: HashMap<usize, usize>,
    /// The nodes an alias names (see [`SpanIndex::referenced`]).
    referenced: HashSet<usize>,
    documents: usize,
    root: Option<usize>,
}

impl Walk {
    /// Record one event. `None` declines the document: a second document,
    /// nesting past [`MAX_DEPTH`], or a mapping that repeats a key.
    fn event(&mut self, event: Event<'_>, span: Span) -> Option<()> {
        let chars = span.start.index()..span.end.index();
        match event {
            Event::DocumentStart(_) => {
                self.documents += 1;
                if self.documents > 1 {
                    return None;
                }
            }
            Event::Scalar(read, style, anchor, tag) => {
                let at_key = self.stack.last().is_some_and(|frame| {
                    matches!(self.nodes[frame.node], Node::Mapping { .. }) && frame.key.is_none()
                });
                let value = scalar_value(&read, style, tag.as_ref());
                let readable = value.is_some();
                let key = value
                    .filter(|_| at_key || anchor > 0)
                    .map(Scalar::into_owned);
                let node = self.push(Node::Scalar(ScalarNode {
                    span: chars,
                    style,
                    value: readable,
                    key,
                    read: (anchor > 0 || tag.is_some()).then(|| read.into_owned()),
                    anchored: anchor > 0,
                    tagged: tag.is_some(),
                }));
                if anchor > 0 {
                    self.anchors.insert(anchor, node);
                }
                self.attach(node)?;
            }
            Event::Alias(anchor) => {
                let target = self.anchors.get(&anchor).copied();
                // An alias inside the collection its anchor names (which
                // ayaml refuses) names a node still open.
                let named = target.or_else(|| {
                    self.stack
                        .iter()
                        .rev()
                        .find(|frame| frame.anchor == anchor)
                        .map(|frame| frame.node)
                });
                self.referenced.extend(named);
                let node = self.push(Node::Alias {
                    span: chars,
                    target,
                });
                self.attach(node)?;
            }
            Event::SequenceStart(anchor, tag) => {
                let plain = plain_collection(tag.as_ref());
                self.open(
                    Node::Sequence {
                        elements: Vec::new(),
                        plain,
                    },
                    anchor,
                )?;
            }
            Event::MappingStart(anchor, tag) => {
                let plain = plain_collection(tag.as_ref());
                self.open(
                    Node::Mapping {
                        plain,
                        entries: 0,
                        opened: chars.end,
                    },
                    anchor,
                )?;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let frame = self.stack.pop()?;
                if frame.anchor > 0 {
                    self.anchors.insert(frame.anchor, frame.node);
                }
                self.attach(frame.node)?;
            }
            Event::StreamStart | Event::StreamEnd | Event::DocumentEnd | Event::Nothing => {}
        }
        Some(())
    }

    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn open(&mut self, node: Node, anchor: usize) -> Option<()> {
        if self.stack.len() >= MAX_DEPTH {
            return None;
        }
        let node = self.push(node);
        self.stack.push(Frame {
            node,
            anchor,
            key: None,
        });
        Some(())
    }

    /// Add a complete node to the open collection, or make it the root.
    fn attach(&mut self, node: usize) -> Option<()> {
        let Some(frame) = self.stack.last_mut() else {
            self.root = Some(node);
            return Some(());
        };
        let parent = frame.node;
        if let Node::Sequence { elements, .. } = &mut self.nodes[parent] {
            elements.push(node);
            return Some(());
        }
        let Some(key) = frame.key.take() else {
            frame.key = Some(node);
            return Some(());
        };
        if let Node::Mapping { entries, .. } = &mut self.nodes[parent] {
            *entries += 1;
        }
        // A mapping that repeats a key declines the document, as saphyr's
        // loader refused it: which entry the check side reads is not this
        // resolver's to guess. Keys compare as saphyr's loader compared them,
        // by value (`1` repeats `0x1`), an alias key as the scalar it repeats;
        // a key under a tag outside the core schema, and a collection key,
        // are not compared.
        if let Some(value) = self.key_value(key) {
            if self.keys.contains_key(&(parent, value.clone())) {
                return None;
            }
            self.keys.insert((parent, value), (key, node));
        }
        Some(())
    }

    /// `key`'s value as a mapping key: a scalar's, or that of the anchored
    /// scalar an alias key repeats.
    fn key_value(&self, key: usize) -> Option<ScalarOwned> {
        let key = match &self.nodes[key] {
            Node::Alias {
                target: Some(target),
                ..
            } => &self.nodes[*target],
            node => node,
        };
        match key {
            Node::Scalar(scalar) => scalar.key.clone(),
            _ => None,
        }
    }
}

/// What saphyr's loader makes of a scalar: `None` for one it reads as a tagged
/// node (under a tag outside the core schema) or as a bad value (a core tag
/// it cannot read), neither of which the resolver edits or matches as a key.
fn scalar_value<'a>(
    read: &Cow<'a, str>,
    style: ScalarStyle,
    tag: Option<&Cow<'a, Tag>>,
) -> Option<Scalar<'a>> {
    if tag.is_some_and(|tag| !tag.is_yaml_core_schema()) {
        return None;
    }
    Scalar::parse_from_cow_and_metadata(read.clone(), style, tag)
}

/// Whether saphyr's loader reads a collection under `tag` as a plain sequence
/// or mapping (no tag, or a core-schema one) rather than a tagged node.
fn plain_collection(tag: Option<&Cow<'_, Tag>>) -> bool {
    tag.is_none_or(|tag| tag.is_yaml_core_schema())
}

/// One parsed document: its nodes, its scalar keys, and how to map saphyr's
/// character offsets to bytes.
struct SpanIndex<'t> {
    /// The text the parser read: the source without a leading BOM.
    text: &'t str,
    /// The length of the stripped BOM, added back to every returned range.
    bom_len: usize,
    offsets: CharOffsets,
    nodes: Vec<Node>,
    /// Each mapping's scalar entries, `(key node, value node)` by `(mapping
    /// node, key)`.
    keys: HashMap<(usize, ScalarOwned), (usize, usize)>,
    /// The nodes an alias or a merge names: an edit at one, or inside one,
    /// changes every alias of it as well, so none is edited.
    referenced: HashSet<usize>,
    root: usize,
}

impl<'t> SpanIndex<'t> {
    /// Parse `text` in one pass over the parser's events. `None` when it does
    /// not parse, holds no document or more than one, nests past
    /// [`MAX_DEPTH`], or repeats a mapping key.
    fn read(text: &'t str) -> Option<Self> {
        // The check side's flow guard (parser-guard parity). The fixer runs
        // `Format::parse` (which applies it) before reaching here, but guard
        // directly so the resolver is self-safe. It scans the raw text, whose
        // reading of a BOM as zero width covers the stripped text saphyr parses.
        if !crate::yaml_depth::flow_depth_within_limit(text) {
            return None;
        }
        // Strip a leading UTF-8 BOM before parsing: the check side (`Format::parse`)
        // strips it, but `saphyr` rejects a `\u{FEFF}` prefix, so a BOM file would
        // else be advertised-fixable yet always skipped. Resolve against the stripped
        // text, then shift each span back into ORIGINAL byte coordinates by the
        // stripped length (the caller splices the raw file bytes, BOM included).
        let stripped = text.trim_start_matches('\u{feff}');
        if stripped.len() > crate::MAX_STRUCTURED_BYTES {
            return None;
        }
        READS.with(|reads| reads.set(reads.get() + 1));
        // This input's share of the YAML parse budget, held while the parser
        // runs (`yaml_budget`, ADR-0042).
        let _budget = crate::yaml_budget::reserve(stripped.len());
        // The parser's event iterator, not saphyr's loader: the loader's `load`
        // recurses once per nesting level (a deep document overflows the
        // stack) and copies an anchored subtree for every alias (an alias bomb
        // exhausts memory, as its budget does not count mapping keys). The
        // walk below is iterative and keeps an alias as one node.
        let mut walk = Walk::default();
        for parsed in Parser::new_from_str(stripped) {
            let (event, span) = parsed.ok()?;
            walk.event(event, span)?;
        }
        Some(Self {
            text: stripped,
            bom_len: text.len() - stripped.len(),
            offsets: CharOffsets::new(stripped),
            nodes: walk.nodes,
            keys: walk.keys,
            referenced: walk.referenced,
            root: walk.root?,
        })
    }

    /// The node at `path`, reached through mappings and sequences only: a path
    /// through an alias declines (saphyr's loader would follow a copy of the
    /// anchored node, whose spans point at the anchor's definition, so an edit
    /// there would change every alias of it), as does one through a non-string
    /// key or a collection under a tag outside the core schema. So does a path
    /// to or through a node an alias or a merge names, where the edit would
    /// change what each of them reads as well.
    fn find(&self, path: &[PathSeg]) -> Option<usize> {
        let mut node = self.root;
        for seg in path {
            if self.referenced.contains(&node) {
                return None;
            }
            node = match (seg, &self.nodes[node]) {
                (PathSeg::Key(key), Node::Mapping { plain: true, .. }) => {
                    self.keys.get(&(node, ScalarOwned::String(key.clone())))?.1
                }
                (
                    PathSeg::Index(i),
                    Node::Sequence {
                        elements,
                        plain: true,
                    },
                ) => *elements.get(*i)?,
                _ => return None,
            };
        }
        (!self.referenced.contains(&node)).then_some(node)
    }

    /// The byte range `set_value` replaces for the scalar at `path`: the scalar
    /// token (quotes included for a quoted scalar, trailing comment/whitespace
    /// excluded by the clamp below), after the scalar's anchor, which survives.
    /// A core-schema tag (`!!str`) is replaced with the value: the serialized
    /// value carries its own type, which a kept tag could contradict. `None`
    /// when the path does not resolve to such a scalar.
    fn value_span(&self, path: &[PathSeg]) -> Option<Range<usize>> {
        // Only a SCALAR leaf is a set_value target: a mapping, a sequence, and
        // an alias (`use: *anchor`) decline. A splice over an alias would
        // silently DE-ALIAS it (replace the reference with a literal), a
        // semantic change.
        let Node::Scalar(scalar) = &self.nodes[self.find(path)?] else {
            return None;
        };
        if !scalar.value || matches!(scalar.style, ScalarStyle::Literal | ScalarStyle::Folded) {
            return None;
        }
        let raw = self.bytes(&scalar.span)?;
        let at = self.text.get(raw.clone())?;
        // Decline two saphyr span shapes a flow-scalar splice cannot safely replace
        // (both are verify-gated anyway, but decline cleanly rather than emit an
        // un-appliable suggestion / trust a bogus span):
        //   - an EMPTY span: an IMPLICIT null (`x:`) -- saphyr places a zero-width
        //     span AT THE COLON, a bogus splice point (explicit `~` / `""` are fine);
        //   - a MULTI-LINE span: a block (`|` / `>`) or line-folded scalar -- the
        //     span is the block CONTENT, so splicing a flow scalar leaves the block
        //     indicator and changes meaning.
        if at.is_empty() || at.contains('\n') {
            return None;
        }
        let (start, content) = self.past_properties(scalar, raw.clone())?;
        if content >= raw.end {
            // Only an anchor or a tag (`x: &a`): there is no scalar to replace.
            return None;
        }
        // Tighten the trailing edge of a QUOTED scalar: saphyr's span end runs to
        // END-OF-LINE (over trailing whitespace + a `#` comment), whereas a plain
        // scalar's end is tight. Without this, a splice over `"old"  # keep` DELETES
        // the comment and COMMITS (a comment is not part of the value, so re-verify
        // cannot catch it) -- the audit's HIGH silent-data-loss finding.
        let end = clamp_quoted_scalar_span(self.text, content..raw.end).end;
        Some(start + self.bom_len..end + self.bom_len)
    }

    /// Where a set starts and where the scalar starts, past the anchor and
    /// the tag that saphyr's scalar span begins with: the anchor and the blanks
    /// after it are kept, a tag is replaced with the value. `None` when a tag
    /// precedes the anchor (no one range drops the one and keeps the other)
    /// or the text past the properties is not what the parser read.
    fn past_properties(&self, scalar: &ScalarNode, raw: Range<usize>) -> Option<(usize, usize)> {
        let (mut anchor, mut tag) = (scalar.anchored, scalar.tagged);
        let mut at = raw.start;
        let mut start = raw.start;
        let bytes = &self.text.as_bytes()[..raw.end];
        let blank = |b: &u8| *b == b' ' || *b == b'\t';
        while anchor || tag {
            let first = bytes.get(at).copied();
            // A property runs to the first blank; the scalar span is one line.
            at += bytes[at..]
                .iter()
                .position(blank)
                .unwrap_or(bytes.len() - at);
            at += bytes[at..].iter().take_while(|&b| blank(b)).count();
            match first {
                Some(b'&') if anchor => {
                    if scalar.tagged && !tag {
                        return None;
                    }
                    anchor = false;
                    start = at;
                }
                Some(b'!') if tag => tag = false,
                _ => return None,
            }
        }
        if at == raw.start {
            return Some((start, at));
        }
        let content = &self.text[at..raw.end];
        let fits = match scalar.style {
            ScalarStyle::Plain => scalar.read.as_deref() == Some(content),
            ScalarStyle::SingleQuoted => content.starts_with('\''),
            ScalarStyle::DoubleQuoted => content.starts_with('"'),
            ScalarStyle::Literal | ScalarStyle::Folded => false,
        };
        fits.then_some((start, at))
    }

    /// The edit `remove_value` makes for each of `paths` (see [`Self::removal`]):
    /// the entry's line deleted, or, for the last of the entries when the
    /// paths remove every entry of a block mapping, the entry replaced with
    /// `{}`, which keeps the mapping a mapping where deleting each line would
    /// leave an empty value, null. Ranges in the caller's bytes (BOM
    /// included).
    fn removal_edits(&self, paths: &[Vec<PathSeg>]) -> Vec<Option<(Range<usize>, &'static str)>> {
        let removals: Vec<Option<Removal>> = paths.iter().map(|path| self.removal(path)).collect();
        // Each mapping's removed keys, and the line of the last of them.
        let mut removed: HashMap<usize, (HashSet<usize>, usize, usize)> = HashMap::new();
        for removal in removals.iter().flatten() {
            let (keys, last_line, last_key) =
                removed
                    .entry(removal.parent)
                    .or_insert((HashSet::new(), 0, removal.key));
            keys.insert(removal.key);
            if removal.line.start >= *last_line {
                *last_line = removal.line.start;
                *last_key = removal.key;
            }
        }
        let emptied: HashSet<usize> = removed
            .into_iter()
            .filter(|(parent, (keys, ..))| match self.nodes[*parent] {
                Node::Mapping {
                    entries, opened, ..
                } => keys.len() == entries && !self.opens_flow(opened),
                _ => false,
            })
            .map(|(_, (_, _, last_key))| last_key)
            .collect();
        let shift = |range: Range<usize>| range.start + self.bom_len..range.end + self.bom_len;
        removals
            .into_iter()
            .map(|removal| {
                let removal = removal?;
                Some(if emptied.contains(&removal.key) {
                    (shift(removal.entry), "{}")
                } else {
                    (shift(removal.line), "")
                })
            })
            .collect()
    }

    /// The range [`Self::removal`] deletes for `path` alone, in the caller's
    /// bytes: `None` for a block mapping's only entry, which becomes `{}`.
    fn removal_span(&self, path: &[PathSeg]) -> Option<Range<usize>> {
        let removal = self.removal(path)?;
        if let Node::Mapping {
            entries: 1, opened, ..
        } = self.nodes[removal.parent]
            && !self.opens_flow(opened)
        {
            return None;
        }
        Some(removal.line.start + self.bom_len..removal.line.end + self.bom_len)
    }

    /// Whether a mapping whose start event's span ends at character `opened`
    /// is a flow mapping, whose `{` that span ends with.
    fn opens_flow(&self, opened: usize) -> bool {
        self.offsets
            .byte(self.text, opened)
            .is_some_and(|at| self.text[..at].ends_with('{'))
    }

    /// The block-mapping entry `remove_value` deletes at `path`: the target
    /// key's WHOLE physical line (through its trailing newline), so the splice
    /// is a clean line removal. `saphyr` is read-only (no edit CST), so this is
    /// a hand-rolled line scan -- CONSERVATIVE to stay provably minimal (the
    /// over-deletion class): resolves ONLY when
    ///   - the last path segment is a mapping KEY (sequence-element removal declines);
    ///   - the value is a single-line SCALAR, or an alias of one (a block `|`/`>` or
    ///     nested mapping / sequence value spans lines -> declines, else the line
    ///     delete would orphan the continuation);
    ///   - the KEY OWNS its line (only indentation precedes it) -- so a FLOW-mapping
    ///     member (`x: {a: 1, b: 2}`, key not at line start) declines rather than
    ///     delete the whole `x:` line and eat its siblings;
    ///   - no alias or merge names the key, the value or a node on the path to
    ///     them, whose aliases the removal would change too.
    ///
    /// Deleting exactly that one line removes exactly the one entry (block mappings
    /// are one entry per line); a trailing `# comment` on the line goes with it.
    /// What the line scan cannot see, a neighbour the deletion changes (a line
    /// after it that joins a block scalar kept with `|+`, an explicit `?` key
    /// that takes the next entry's value), the fixer's check of the whole
    /// document after the edit declines (`structured_fix::checked_removal_edits`).
    fn removal(&self, path: &[PathSeg]) -> Option<Removal> {
        // The last segment must be a mapping KEY (sequence-element removal deferred).
        let (PathSeg::Key(target), parents) = path.split_last()? else {
            return None;
        };
        let parent = self.find(parents)?;
        let Node::Mapping { plain: true, .. } = self.nodes[parent] else {
            return None;
        };
        let (key, value) = *self
            .keys
            .get(&(parent, ScalarOwned::String(target.clone())))?;
        if self.referenced.contains(&key) || self.referenced.contains(&value) {
            return None;
        }
        // The value must be a SCALAR (a mapping/sequence value is multi-line); an
        // alias of an anchored scalar removes with its line like the scalar.
        let (value_span, scalar) = match &self.nodes[value] {
            Node::Scalar(scalar) if scalar.value => (&scalar.span, Some(scalar)),
            Node::Alias {
                span,
                target: Some(target),
            } if matches!(&self.nodes[*target], Node::Scalar(scalar) if scalar.value) => {
                (span, None)
            }
            _ => return None,
        };
        let key_start = match &self.nodes[key] {
            Node::Scalar(scalar) => scalar.span.start,
            Node::Alias { span, .. } => span.start,
            _ => return None,
        };
        let text = self.text;
        // The entry, from the key start to the value end, must be single-line.
        let entry = self.bytes(&(key_start..value_span.end))?;
        if text.get(entry.clone())?.contains('\n') {
            return None;
        }
        // The KEY must OWN its line: only indentation before it (excludes a flow
        // member, whose deletion would eat siblings on the same physical line).
        let line_start = text[..entry.start].rfind('\n').map_or(0, |i| i + 1);
        if !text[line_start..entry.start]
            .bytes()
            .all(|b| b == b' ' || b == b'\t')
        {
            return None;
        }
        // Delete the whole line, through its trailing newline (or to EOF).
        let line_end = text[entry.end..]
            .find('\n')
            .map_or(text.len(), |i| entry.end + i + 1);
        // The value must also be the LAST content on its physical line: the tail
        // after it may be only whitespace and/or a `#` comment. Without this, a
        // MULTI-LINE flow mapping (`x: {\n  a: 1, b: 2\n}` -- where the key starts a
        // physical line it SHARES with sibling members after the value) would have
        // the whole-line delete engulf those siblings: an over-deletion that still
        // parses, so the `Absent` re-verify is blind to it (the audit HIGH bug --
        // symmetric to the leading `line_start..entry.start` guard, mirroring HCL).
        let tail = text[entry.end..line_end]
            .trim_end_matches(['\r', '\n'])
            .trim_start_matches([' ', '\t']);
        if !(tail.is_empty() || tail.starts_with('#')) {
            return None;
        }
        // A quoted value's span runs over the comment after it; the entry ends
        // at its closing quote, past the value's properties.
        let entry_end = match scalar {
            Some(scalar) => {
                let raw = self.bytes(value_span)?;
                let content = self
                    .past_properties(scalar, raw.clone())
                    .map_or(raw.start, |(_, content)| content);
                if content < raw.end {
                    clamp_quoted_scalar_span(text, content..raw.end).end
                } else {
                    raw.end
                }
            }
            None => entry.end,
        };
        Some(Removal {
            parent,
            key,
            line: line_start..line_end,
            entry: entry.start..entry_end,
        })
    }

    /// A character span as a byte range of [`Self::text`].
    fn bytes(&self, chars: &Range<usize>) -> Option<Range<usize>> {
        if chars.start > chars.end {
            return None;
        }
        Some(self.offsets.byte(self.text, chars.start)?..self.offsets.byte(self.text, chars.end)?)
    }
}

/// Character offsets to byte offsets. `saphyr-parser`'s `Marker::index` counts
/// characters (not bytes; a bump must re-verify this). ASCII text maps one to
/// one; other text keeps the byte offset of every [`Self::STRIDE`]th character, so a lookup steps over fewer
/// than that many characters, and a document of many matches maps each span
/// without a pass over the text per span.
enum CharOffsets {
    Ascii,
    Utf8 { marks: Vec<usize>, chars: usize },
}

impl CharOffsets {
    const STRIDE: usize = 64;

    fn new(text: &str) -> Self {
        if text.is_ascii() {
            return Self::Ascii;
        }
        let mut marks = Vec::with_capacity(text.len() / Self::STRIDE + 1);
        let mut chars = 0;
        for (index, (byte, _)) in text.char_indices().enumerate() {
            if index % Self::STRIDE == 0 {
                marks.push(byte);
            }
            chars = index + 1;
        }
        Self::Utf8 { marks, chars }
    }

    /// The byte offset of character `index` of `text`; the character count is
    /// the end of the text.
    fn byte(&self, text: &str, index: usize) -> Option<usize> {
        match self {
            Self::Ascii => (index <= text.len()).then_some(index),
            Self::Utf8 { marks, chars } => {
                if index == *chars {
                    return Some(text.len());
                }
                let mark = *marks.get(index / Self::STRIDE)?;
                let (byte, _) = text[mark..].char_indices().nth(index % Self::STRIDE)?;
                Some(mark + byte)
            }
        }
    }
}

/// Clamp a quoted scalar's span end to just after its closing quote. `saphyr`
/// over-extends a quoted scalar's `span.end` to end-of-line (past trailing
/// whitespace + a comment); a plain / block scalar's end is already tight, so
/// its span is returned unchanged. Byte-scanning is UTF-8-safe: the quote and
/// backslash bytes are ASCII and never collide with a multibyte continuation.
fn clamp_quoted_scalar_span(text: &str, span: Range<usize>) -> Range<usize> {
    let bytes = text.as_bytes();
    let open = bytes[span.start];
    if open != b'"' && open != b'\'' {
        return span; // plain / block scalar: `span.end` is already tight.
    }
    let mut i = span.start + 1;
    while i < span.end {
        match bytes[i] {
            // A double-quote escape (`\"`, `\\`, `\n`, ... all ASCII) skips 2.
            b'\\' if open == b'"' => i += 2,
            b if b == open => {
                // In a single-quoted scalar `''` is a literal quote, not a close.
                if open == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                } else {
                    return span.start..i + 1; // just after the closing quote
                }
            }
            _ => i += 1,
        }
    }
    span // unterminated within the span (unreachable for a valid parsed scalar)
}

#[cfg(test)]
mod tests;
