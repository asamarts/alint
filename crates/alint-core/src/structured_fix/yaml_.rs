use super::PathSeg;
use crate::yaml::{CoreScalar, is_core_collection_tag, resolve_core_scalar};
use ayaml::{CollectionStyle, Event, ScalarStyle, Span};
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
    /// at the alias.
    Alias {
        span: Range<usize>,
        target: Option<usize>,
    },
    /// A sequence and its elements. Not `plain` under a tag outside the core
    /// schema, which ayaml reads as a tagged node, not a plain sequence.
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
        /// Whether this mapping is written in flow style (`{...}`).
        flow: bool,
    },
}

struct ScalarNode {
    /// The scalar token's byte span in [`SpanIndex::text`] (excluding leading
    /// `&anchor` / `!tag` properties and trailing whitespace/comments).
    span: Range<usize>,
    style: ScalarStyle,
    /// Whether ayaml reads the scalar as a plain core-schema value: not one
    /// under a tag outside the core schema, nor one its core tag cannot read
    /// (`!!int x`).
    value: bool,
    /// The scalar's value as a mapping key, kept on a key and on an anchored
    /// scalar an alias key repeats. `None` where ayaml does not read a core
    /// value (see [`Self::value`]).
    key: Option<CoreScalar>,
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
    anchor: Option<String>,
    /// In a mapping, the key whose value comes next.
    key: Option<usize>,
}

/// The state of one pass over the events.
#[derive(Default)]
struct Walk {
    nodes: Vec<Node>,
    stack: Vec<Frame>,
    keys: HashMap<(usize, CoreScalar), (usize, usize)>,
    anchors: HashMap<String, usize>,
    /// The nodes an alias names (see [`SpanIndex::referenced`]).
    referenced: HashSet<usize>,
    documents: usize,
    root: Option<usize>,
}

impl Walk {
    /// Record one event. `None` declines the document: a second document,
    /// nesting past [`MAX_DEPTH`], or a mapping that repeats a key.
    fn event(&mut self, event: Event<'_>) -> Option<()> {
        match event {
            Event::DocumentStart { .. } => {
                self.documents += 1;
                if self.documents > 1 {
                    return None;
                }
            }
            Event::Scalar {
                value,
                style,
                tag,
                anchor,
                span,
                ..
            } => {
                let at_key = self.stack.last().is_some_and(|frame| {
                    matches!(self.nodes[frame.node], Node::Mapping { .. }) && frame.key.is_none()
                });
                let resolved = resolve_core_scalar(&value, style, tag.as_ref());
                let readable = resolved.is_some();
                let key = resolved.filter(|_| at_key || anchor.is_some());
                let node = self.push(Node::Scalar(ScalarNode {
                    span: span_range(span),
                    style,
                    value: readable,
                    key,
                    anchored: anchor.is_some(),
                    tagged: tag.is_some(),
                }));
                if let Some(anchor) = anchor {
                    self.anchors.insert(anchor.as_str().to_owned(), node);
                }
                self.attach(node)?;
            }
            Event::Alias { anchor, span } => {
                let name = anchor.as_str();
                let target = self.anchors.get(name).copied();
                // An alias inside the collection its anchor names (which
                // ayaml refuses during composition) names a node still open.
                let named = target.or_else(|| {
                    self.stack
                        .iter()
                        .rev()
                        .find(|frame| frame.anchor.as_deref() == Some(name))
                        .map(|frame| frame.node)
                });
                self.referenced.extend(named);
                let node = self.push(Node::Alias {
                    span: span_range(span),
                    target,
                });
                self.attach(node)?;
            }
            Event::SequenceStart { anchor, tag, .. } => {
                let plain = plain_collection(tag.as_ref());
                self.open(
                    Node::Sequence {
                        elements: Vec::new(),
                        plain,
                    },
                    anchor.map(|a| a.as_str().to_owned()),
                )?;
            }
            Event::MappingStart {
                anchor, tag, style, ..
            } => {
                let plain = plain_collection(tag.as_ref());
                self.open(
                    Node::Mapping {
                        plain,
                        entries: 0,
                        flow: style == CollectionStyle::Flow,
                    },
                    anchor.map(|a| a.as_str().to_owned()),
                )?;
            }
            Event::SequenceEnd { .. } | Event::MappingEnd { .. } => {
                let frame = self.stack.pop()?;
                if let Some(anchor) = frame.anchor {
                    self.anchors.insert(anchor, frame.node);
                }
                self.attach(frame.node)?;
            }
            Event::StreamStart { .. }
            | Event::StreamEnd { .. }
            | Event::DocumentEnd { .. }
            | Event::Directive { .. } => {}
            _ => return None,
        }
        Some(())
    }

    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn open(&mut self, node: Node, anchor: Option<String>) -> Option<()> {
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
        // A mapping that repeats a key declines the document: which entry the
        // check side reads is not this resolver's to guess. Keys compare by
        // resolved core-schema value (`1` repeats `0x1`), an alias key as the
        // scalar it repeats; a key under a tag outside the core schema, and a
        // collection key, are not compared.
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
    fn key_value(&self, key: usize) -> Option<CoreScalar> {
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

fn span_range(span: Span) -> Range<usize> {
    span.start()..span.end()
}

/// Whether ayaml reads a collection under `tag` as a plain sequence or mapping
/// (no tag, or a core-schema collection tag) rather than a tagged node.
fn plain_collection(tag: Option<&ayaml::Tag>) -> bool {
    tag.is_none_or(is_core_collection_tag)
}

/// Leading `&anchor` and/or `!tag` properties immediately before a scalar on
/// the same physical line.
struct LeadingProperties {
    /// Byte offset of the earliest property (`&anchor` or `!tag`) on the line.
    earliest: usize,
    /// Byte offset of the `!tag` token, if the scalar is tagged.
    tag_start: Option<usize>,
    /// Byte offset of the `&anchor` token, if the scalar is anchored.
    anchor_start: Option<usize>,
}

/// Scan backward from `scalar_start` on the same physical line for the
/// `&anchor` and/or `!tag` properties attached to a scalar. Returns `None` if
/// an expected property sits on an earlier line or cannot be matched cleanly.
fn leading_properties(
    text: &str,
    scalar_start: usize,
    has_anchor: bool,
    has_tag: bool,
) -> Option<LeadingProperties> {
    if !has_anchor && !has_tag {
        return Some(LeadingProperties {
            earliest: scalar_start,
            tag_start: None,
            anchor_start: None,
        });
    }
    let bytes = text.as_bytes();
    let line_start = text[..scalar_start].rfind('\n').map_or(0, |i| i + 1);
    let mut cursor = scalar_start;
    let mut need_anchor = has_anchor;
    let mut need_tag = has_tag;
    let mut anchor_start = None;
    let mut tag_start = None;
    while need_anchor || need_tag {
        let before_blanks = cursor;
        while cursor > line_start && matches!(bytes[cursor - 1], b' ' | b'\t') {
            cursor -= 1;
        }
        if cursor == before_blanks || cursor == line_start {
            return None;
        }
        let tok_end = cursor;
        let tok_start = if bytes[tok_end - 1] == b'>' {
            let mut pos = tok_end - 1;
            let mut verbatim = None;
            while pos > line_start {
                pos -= 1;
                if bytes[pos] == b'<' && pos > line_start && bytes[pos - 1] == b'!' {
                    verbatim = Some(pos - 1);
                    break;
                }
            }
            verbatim.unwrap_or_else(|| scan_property_start(bytes, line_start, tok_end))
        } else {
            scan_property_start(bytes, line_start, tok_end)
        };
        let tok = text.get(tok_start..tok_end)?;
        if need_tag && tok.starts_with('!') {
            need_tag = false;
            tag_start = Some(tok_start);
            cursor = tok_start;
        } else if need_anchor && tok.starts_with('&') {
            need_anchor = false;
            anchor_start = Some(tok_start);
            cursor = tok_start;
        } else {
            return None;
        }
    }
    Some(LeadingProperties {
        earliest: cursor,
        tag_start,
        anchor_start,
    })
}

fn scan_property_start(bytes: &[u8], line_start: usize, tok_end: usize) -> usize {
    let mut pos = tok_end;
    while pos > line_start && !matches!(bytes[pos - 1], b' ' | b'\t' | b'[' | b'{' | b',') {
        pos -= 1;
    }
    pos
}

/// One parsed document: its nodes and its scalar keys, with native UTF-8 byte
/// spans from `ayaml`.
struct SpanIndex<'t> {
    /// The text the parser read: the source without a leading BOM.
    text: &'t str,
    /// The length of the stripped BOM, added back to every returned range.
    bom_len: usize,
    nodes: Vec<Node>,
    /// Each mapping's scalar entries, `(key node, value node)` by `(mapping
    /// node, key)`.
    keys: HashMap<(usize, CoreScalar), (usize, usize)>,
    /// The nodes an alias or a merge names: an edit at one, or inside one,
    /// changes every alias of it as well, so none is edited.
    referenced: HashSet<usize>,
    root: usize,
}

impl<'t> SpanIndex<'t> {
    /// Parse `text` in one pass over `ayaml`'s spanned events. `None` when it
    /// does not parse, holds no document or more than one, nests past
    /// [`MAX_DEPTH`], or repeats a mapping key.
    fn read(text: &'t str) -> Option<Self> {
        // The check side's flow guard (parser-guard parity). The fixer runs
        // `Format::parse` (which applies it) before reaching here, but guard
        // directly so the resolver is self-safe.
        if !crate::yaml_depth::flow_depth_within_limit(text) {
            return None;
        }
        // Strip leading UTF-8 BOMs to match `Format::parse`, then shift each
        // returned byte span back into original byte coordinates (the caller
        // splices the raw file bytes, BOM included).
        let stripped = text.trim_start_matches('\u{feff}');
        if stripped.len() > crate::MAX_STRUCTURED_BYTES {
            return None;
        }
        READS.with(|reads| reads.set(reads.get() + 1));
        // This input's share of the YAML parse budget, held while the parser
        // runs (`yaml_budget`, ADR-0042).
        let _budget = crate::yaml_budget::reserve(stripped.len());
        // Walk `ayaml`'s lazy event stream iteratively, keeping each alias as
        // a single node so alias bombs and deep nesting never blow memory or
        // the stack.
        let mut walk = Walk::default();
        for parsed in crate::yaml::parse_events_stream(stripped).ok()? {
            let event = parsed.ok()?;
            walk.event(event)?;
        }
        Some(Self {
            text: stripped,
            bom_len: text.len() - stripped.len(),
            nodes: walk.nodes,
            keys: walk.keys,
            referenced: walk.referenced,
            root: walk.root?,
        })
    }

    /// The node at `path`, reached through mappings and sequences only: a path
    /// through an alias declines (an edit at the anchor's definition would
    /// change every alias of it), as does one through a non-string key or a
    /// collection under a tag outside the core schema. So does a path to or
    /// through a node an alias or a merge names, where the edit would change
    /// what each of them reads as well.
    fn find(&self, path: &[PathSeg]) -> Option<usize> {
        let mut node = self.root;
        for seg in path {
            if self.referenced.contains(&node) {
                return None;
            }
            node = match (seg, &self.nodes[node]) {
                (PathSeg::Key(key), Node::Mapping { plain: true, .. }) => {
                    self.keys.get(&(node, CoreScalar::String(key.clone())))?.1
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
    /// already excluded by `ayaml`), after the scalar's anchor, which survives.
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
        let raw = scalar.span.clone();
        let at = self.text.get(raw.clone())?;
        // Decline empty spans (implicit null `x:`) and multi-line scalars (e.g.
        // line-folded plain/quoted scalars where a single-line flow splice
        // would alter surrounding layout).
        if at.is_empty() || at.contains('\n') {
            return None;
        }
        let props = leading_properties(self.text, raw.start, scalar.anchored, scalar.tagged)?;
        let start = match (props.anchor_start, props.tag_start) {
            (Some(anchor), Some(tag)) => {
                // A core tag before the anchor cannot be dropped while keeping
                // the anchor in one contiguous edit.
                if tag < anchor {
                    return None;
                }
                tag
            }
            (None, Some(tag)) => tag,
            (_, None) => raw.start,
        };
        Some(start + self.bom_len..raw.end + self.bom_len)
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
                    entries,
                    flow: false,
                    ..
                } => keys.len() == entries,
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
            entries: 1,
            flow: false,
            ..
        } = self.nodes[removal.parent]
        {
            return None;
        }
        Some(removal.line.start + self.bom_len..removal.line.end + self.bom_len)
    }

    /// The block-mapping entry `remove_value` deletes at `path`: the target
    /// key's WHOLE physical line (through its trailing newline), so the splice
    /// is a clean line removal. Conservative to stay provably minimal:
    /// resolves ONLY when
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
            .get(&(parent, CoreScalar::String(target.clone())))?;
        if self.referenced.contains(&key) || self.referenced.contains(&value) {
            return None;
        }
        let (key_start, key_end) = match &self.nodes[key] {
            Node::Scalar(scalar) => (
                leading_properties(self.text, scalar.span.start, scalar.anchored, scalar.tagged)?
                    .earliest,
                scalar.span.end,
            ),
            Node::Alias { span, .. } => (span.start, span.end),
            _ => return None,
        };
        let text = self.text;
        // The KEY must OWN its line: only indentation before it (excludes a flow
        // member, whose deletion would eat siblings on the same physical line).
        let line_start = text[..key_start].rfind('\n').map_or(0, |i| i + 1);
        if !text[line_start..key_start]
            .bytes()
            .all(|b| b == b' ' || b == b'\t')
        {
            return None;
        }
        // The value must be a SCALAR (a mapping/sequence value is multi-line); an
        // alias of an anchored scalar removes with its line like the scalar.
        let (entry_end, line_end) = match &self.nodes[value] {
            Node::Scalar(scalar)
                if scalar.value
                    && !matches!(scalar.style, ScalarStyle::Literal | ScalarStyle::Folded)
                    && scalar.span.is_empty() =>
            {
                // Implicit empty mapping value (`k:\n`, `k: &a  # c\n`): ayaml
                // places its zero-width span at the start of the next line.
                // Verify the `:` and any attached `&anchor`/`!tag` properties
                // sit cleanly on the key's physical line with only whitespace
                // or a `#` comment after them.
                if key_start > key_end || text.get(key_start..key_end)?.contains('\n') {
                    return None;
                }
                let line_end = text[key_end..]
                    .find('\n')
                    .map_or(text.len(), |i| key_end + i + 1);
                let end_of_entry =
                    empty_value_entry_end(text, key_end, line_end, scalar.anchored, scalar.tagged)?;
                (end_of_entry, line_end)
            }
            Node::Scalar(scalar)
                if scalar.value
                    && !matches!(scalar.style, ScalarStyle::Literal | ScalarStyle::Folded) =>
            {
                let value_end = scalar.span.end;
                if key_start > value_end || text.get(key_start..value_end)?.contains('\n') {
                    return None;
                }
                let line_end = text[value_end..]
                    .find('\n')
                    .map_or(text.len(), |i| value_end + i + 1);
                let tail = text[value_end..line_end]
                    .trim_end_matches(['\r', '\n'])
                    .trim_start_matches([' ', '\t']);
                if !(tail.is_empty() || tail.starts_with('#')) {
                    return None;
                }
                (value_end, line_end)
            }
            Node::Alias {
                span,
                target: Some(target),
            } if matches!(&self.nodes[*target], Node::Scalar(scalar) if scalar.value) => {
                let value_end = span.end;
                if key_start > value_end || text.get(key_start..value_end)?.contains('\n') {
                    return None;
                }
                let line_end = text[value_end..]
                    .find('\n')
                    .map_or(text.len(), |i| value_end + i + 1);
                let tail = text[value_end..line_end]
                    .trim_end_matches(['\r', '\n'])
                    .trim_start_matches([' ', '\t']);
                if !(tail.is_empty() || tail.starts_with('#')) {
                    return None;
                }
                (value_end, line_end)
            }
            _ => return None,
        };
        Some(Removal {
            parent,
            key,
            line: line_start..line_end,
            entry: key_start..entry_end,
        })
    }
}

/// Find the end byte offset of a single-line mapping entry whose value is an
/// implicit empty scalar (`k:`, `k: &a`, `k: !!null # c`). Returns `None` if
/// the `:` or properties do not stay on `key_end..line_end` or if non-comment
/// tokens (such as flow-mapping siblings) follow on the same line.
fn empty_value_entry_end(
    text: &str,
    key_end: usize,
    line_end: usize,
    mut need_anchor: bool,
    mut need_tag: bool,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let content_end = text[..line_end].trim_end_matches(['\r', '\n']).len();
    let mut cursor = key_end;
    while cursor < content_end && matches!(bytes[cursor], b' ' | b'\t') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b':') {
        return None;
    }
    cursor += 1;
    let mut entry_end = cursor;
    while need_anchor || need_tag {
        let before = cursor;
        while cursor < content_end && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if cursor == before || cursor >= content_end {
            return None;
        }
        let tok_start = cursor;
        if bytes[tok_start] == b'!' && bytes.get(tok_start + 1) == Some(&b'<') {
            while cursor < content_end && bytes[cursor] != b'>' {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'>') {
                return None;
            }
            cursor += 1;
        } else {
            while cursor < content_end && !matches!(bytes[cursor], b' ' | b'\t') {
                cursor += 1;
            }
        }
        let tok = text.get(tok_start..cursor)?;
        if need_tag && tok.starts_with('!') {
            need_tag = false;
            entry_end = cursor;
        } else if need_anchor && tok.starts_with('&') {
            need_anchor = false;
            entry_end = cursor;
        } else {
            return None;
        }
    }
    let tail = text[entry_end..content_end].trim_start_matches([' ', '\t']);
    if !(tail.is_empty() || tail.starts_with('#')) {
        return None;
    }
    Some(entry_end)
}

#[cfg(test)]
mod tests;
