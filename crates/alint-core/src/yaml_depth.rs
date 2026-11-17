//! Pre-parse bounds on YAML flow-collection nesting depth and alias use.
//!
//! The parser bounds both itself: ayaml stops at 128 levels of nesting, block
//! and flow, and at an alias-expansion budget, in time linear in the input,
//! under the limits [`crate::yaml`] sets from this module's constants. These
//! guards scan the raw text before any parse and refuse a document past bounds
//! of their own with alint's own error: flow nesting past the same 128 levels,
//! and an alias expansion the guard charges more strictly than ayaml (see
//! [`MAX_YAML_FLOW_DEPTH`] and [`MAX_YAML_EXPANSION_BYTES`]). They were written
//! for the parser v0.22 used (libyaml-rs through `yaml_serde`), whose tokenizer
//! was super-linear on deep flow nesting (a 40 KB `[`x20000 document took
//! seconds) and which did not bound one anchor replayed many times. They stay
//! as defense in depth (`docs/design/yaml-parser.md`,
//! "Security and resource invariants"). The flow guard reads the raw text
//! itself, so it refuses a flow bomb before any parser reads it. The alias
//! guard measures with ayaml, so it vouches only for a text ayaml reads: when
//! ayaml refuses the text first, the guard says so ([`Expansion::Unparsed`]),
//! and a caller that reads the text with another parser must refuse it too.
//! saphyr, which reads YAML for source spans and structured fixes, refuses a
//! 256th level of flow nesting of its own (its flow level is a `u8`), but does
//! not bound block nesting, and accepts documents ayaml refuses, an alias to a
//! collection that contains it among them.
//!
//! **The scan follows the parser's token boundaries** (`Lexer`), not a
//! heuristic. Both guards are only as sound as the scanner's agreement with the
//! parser on where a scalar or comment ENDS: any region the scanner wrongly
//! treats as opaque hides the structure inside it (a `#` comment in a flow
//! collection whose `"` a heuristic read as a string to EOF; a block-scalar line
//! starting with `"`; a plain-scalar continuation line) -- and a hidden `[[[…`
//! or `*alias` run is exactly what the guards exist to stop. The lexer began as
//! a port of libyaml-rs 0.3.0's scanner and keeps its structure: token dispatch,
//! comments at any token start, plain-scalar end rules (incl. multi-line
//! continuation bounded by the block indent), quoted-scalar escapes,
//! indentation-delimited block scalars (auto-detected or explicit indent), and
//! the indent stack it rolls on `-` / `?` / simple keys. Where ayaml's
//! compatibility reader, which alint parses with, reads YAML differently from
//! libyaml, the lexer follows ayaml: NEL, LS and PS are content, not line
//! breaks; in a flow collection a `:` is the value indicator before a blank,
//! `,`, `]` or `}`, or right after a quoted scalar or a closed collection, and
//! a `?` is the key indicator, as in libyaml (ayaml refuses a `:` before `[`
//! or `{` and a `?` before any flow indicator, and the lexer reads the
//! indicator there: see `is_key_indicator_at` and `is_value_indicator_at`); an
//! anchor or alias name runs to a blank or a flow indicator; a tag runs over
//! ayaml's tag characters, `#` among them; an implicit key's `:` may come up to
//! 1024 characters after the key, counted in characters, not bytes; and a byte
//! order mark that starts a line is read in each of three ways (see `lex`). Where
//! the parser would raise a syntax error the lexer keeps going: the parser
//! stops there, so nothing after the error point is ever parsed, and continuing
//! (rather than stopping) never hides input. A differential property test
//! against ayaml pins the parity.

/// Real config/manifest YAML nests a handful of flow levels; anything past this
/// is a bomb. It is also the parser's depth limit ([`crate::yaml`] sets it): the
/// parser allows 128 nested collections (block or flow) and fails the 129th,
/// whatever the document decodes into, so a document whose flow nesting alone
/// exceeds 128 never decodes anyway. The guard counts only flow levels, so the
/// parser still refuses documents the guard passes, nested deep in block
/// collections.
///
/// The guard refuses a few documents the parser reads, where the lexer counts
/// more than ayaml nests (see the module docs): after an anchor name that
/// ayaml reads two ways it counts every later `[` and `{`, quoted or not, so
/// `- &a:, x\n- '[[[…'` is refused; and it reads a `...` right after the byte
/// order mark that starts the text as a document end, where ayaml reads
/// `\u{feff}... [[[…` as one plain scalar. Both are known false refusals.
pub const MAX_YAML_FLOW_DEPTH: usize = 128;

/// `true` when the YAML text's flow-collection nesting stays within
/// [`MAX_YAML_FLOW_DEPTH`]. A cheap single-pass scan with the parser's own
/// lexical rules (see the module docs), so brackets inside scalars and comments don't
/// count and no construct can hide real nesting from it.
#[must_use]
pub fn flow_depth_within_limit(text: &str) -> bool {
    flow_depth_within_limit_with(text, MAX_YAML_FLOW_DEPTH)
}

/// [`flow_depth_within_limit`] with an explicit ceiling (tests probe parity with
/// a ceiling below the real parser's recursion limit).
fn flow_depth_within_limit_with(text: &str, max: usize) -> bool {
    // A flow collection needs a `[` or `{` byte; without one the depth is 0.
    if !text.bytes().any(|b| b == b'[' || b == b'{') {
        return true;
    }
    let mut within = true;
    lex(text, |tok| match tok {
        Tok::FlowOpen(depth) if depth > max => {
            within = false;
            false
        }
        _ => true,
    });
    within
}

/// The fixed part of the alias-expansion budget: the estimated bytes a YAML
/// document's tree may occupy once aliases are replayed (see
/// [`YAML_NODE_COST`] for the estimate, [`expansion_budget`] for the whole
/// budget).
///
/// Decoding materializes each `*alias` by COPYING its anchor's whole subtree --
/// every node and every scalar string. The parser v0.22 used did not bound
/// this: its recursion limit (128) only caught *nested* aliases (classic
/// billion-laughs), and its alias-DEREF counter (`jumpcount > events.len()*100`)
/// was never reached by a SINGLE anchor referenced many times -- N flat `*a`
/// refs make only N derefs but N x (anchor size) materializations. ayaml
/// charges an alias budget of its own, set from these constants by
/// [`crate::yaml`], but not the same one: it charges only what each alias
/// replays, where this guard's pass charges every node of the expanded tree,
/// the document's own nodes too. The guard is deliberately the stricter of the
/// two, and refuses some alias-bearing documents ayaml would decode (a large
/// document with one small alias). Measured on the parser v0.22 used (release,
/// peak RSS, both the `ayaml::Value` config path and the
/// `serde_json::Value` structured-query path): an 18.5 KB `a: &a [{k: v} x 500]` +
/// 5000 refs (7.5M nodes, ~235 B/node) took 1.6-1.8 GB and 2-3 s, and a 1 MB
/// `a: &a "<1 MB>"` + 4000 refs (only 4000 nodes, but 4 GB of string copies)
/// took 3.8 GB. So the budget is charged in BYTES, not nodes: a node-count limit
/// is blind to a big anchored scalar. Reachable from any untrusted YAML (a
/// `yaml_path_*` / `json_schema_passes` / `extract` target, or a config /
/// `extends:` / `suggest` body).
///
/// The heaviest LEGIT alias use measured is ~4 MB by this estimate (`GitLab`'s own
/// `rules.gitlab-ci.yml`: 1100 aliases, 14.6K expanded nodes, 400 KB of scalar
/// copies; large docker-compose / Helm / GitHub-Actions files with anchors are far
/// below that), so 256 MiB keeps a >60x margin while a bomb is cut off at roughly
/// 256 MiB of tree.
pub const MAX_YAML_EXPANSION_BYTES: usize = 256 * 1024 * 1024;

/// Bytes charged per expanded node (sequence, mapping, scalar, tag) on top of
/// its scalar text. An upper estimate of the per-node tree cost: measured ~235
/// B/node for a map-heavy tree (each `{k: v}` is a map + key + value), ~70 B for
/// a flat integer sequence.
pub const YAML_NODE_COST: usize = 256;

/// Budget bytes allowed per byte of input, on top of [`MAX_YAML_EXPANSION_BYTES`].
/// A LARGE alias-bearing document is not an amplification attack just because
/// its tree is big -- an alias-free document of the same size costs the same and
/// is bounded only by the structured-parse byte cap. So the budget grows with
/// the input at one node's cost per 16 input bytes (real YAML runs ~20-30 bytes
/// per node), and only the AMPLIFIED part is capped by the fixed allowance.
pub const YAML_EXPANSION_BYTES_PER_INPUT_BYTE: usize = YAML_NODE_COST / 16;

/// Upper bounds on the budget constants, enforced at compile time: set them
/// absurdly high (or `usize::MAX`) and the guard would never fire, silently
/// re-opening the alias-bomb `DoS` while the small-budget mechanism tests still
/// pass. At the 32 MiB structured-parse cap the whole budget stays <= ~1.3 GB.
const _: () = assert!(
    MAX_YAML_EXPANSION_BYTES <= 512 * 1024 * 1024
        && YAML_EXPANSION_BYTES_PER_INPUT_BYTE <= 32
        && YAML_NODE_COST >= 128,
    "the YAML alias-expansion budget is too loose to meaningfully bound expansion"
);

/// The alias-expansion budget for a document of `len` input bytes: the fixed
/// [`MAX_YAML_EXPANSION_BYTES`] amplification allowance plus a share
/// proportional to the input (see [`YAML_EXPANSION_BYTES_PER_INPUT_BYTE`]),
/// counted up to the structured-parse byte cap. [`crate::yaml`] sets that cap
/// as the parser's input cap for every document, config bodies included, so a
/// longer text never decodes; counting only up to it keeps the budget at
/// ~768 MiB at most whatever `len` a caller passes.
#[must_use]
pub fn expansion_budget(len: usize) -> usize {
    let len = len.min(crate::structured_format::MAX_STRUCTURED_BYTES);
    MAX_YAML_EXPANSION_BYTES + len * YAML_EXPANSION_BYTES_PER_INPUT_BYTE
}

/// What the alias-expansion guard found ([`expansion`], [`GuardScan::expansion`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expansion {
    /// The text holds no alias, or the guard read it whole and its alias
    /// expansion fits the [`expansion_budget`].
    WithinLimit,
    /// The alias expansion exceeds the budget: refuse the text.
    OverLimit,
    /// The text holds an alias, but the parser the guard measures with
    /// (ayaml, which alint decodes with) refused it before the expansion was
    /// counted, for the reason this holds: a syntax error, or a limit other
    /// than the alias budget, such as the input cap. The guard does not know
    /// what the aliases expand to. A caller whose real parse is that same
    /// parser may go on, as the parse refuses the text with the proper error;
    /// a caller that reads the text with another parser (saphyr), which may
    /// accept what ayaml refused, must refuse the text, or it reads aliases
    /// nothing has bounded.
    Unparsed(String),
}

impl Expansion {
    /// `false` only for [`Expansion::OverLimit`]: the verdict for a caller
    /// whose real parse is ayaml, which refuses an [`Expansion::Unparsed`]
    /// text itself.
    #[must_use]
    pub const fn within_limit_for_ayaml(&self) -> bool {
        !matches!(self, Self::OverLimit)
    }
}

/// The alias-expansion verdict for `text` (see [`Expansion`]). Only a document
/// that actually uses an alias (`*name`) can amplify, so alias-free text
/// short-circuits to [`Expansion::WithinLimit`] at zero cost (its node count is
/// ~linear in bytes, already bounded by the read cap). For alias-bearing text a
/// cheap DISCARD-ONLY pass drives the parser's deserializer, which replays
/// anchored events through the visitor -- so the expansion is charged
/// ([`YAML_NODE_COST`] per node plus every scalar's bytes, exactly the copies
/// the real parse would make) and the pass bails once the budget is exceeded
/// (measured, release: both reported bombs -- 7.5M map nodes, and 4000 copies
/// of a 1 MB scalar -- are rejected in well under a second without
/// allocating). It builds NO value, so it cannot change the real parse's
/// output. A document ayaml refuses for another reason (malformed YAML, a
/// limit other than the alias budget) is [`Expansion::Unparsed`]: ayaml reads
/// the whole document before the visitor sees a node, so after a syntax error
/// anywhere nothing was counted, and the guard says so rather than vouch for
/// it. A real bomb is ordinary scalars / sequences / maps (and tagged nodes,
/// via `visit_enum`) and is counted.
#[must_use]
pub fn expansion(text: &str) -> Expansion {
    expansion_with(text, expansion_budget(text.len()))
}

/// `true` unless [`expansion`] is [`Expansion::OverLimit`]: the guard for a
/// caller whose real parse is ayaml, which refuses an [`Expansion::Unparsed`]
/// text with the proper error. A caller that parses with anything else uses
/// [`expansion`] and refuses [`Expansion::Unparsed`] too.
#[must_use]
pub fn expansion_within_limit(text: &str) -> bool {
    expansion(text).within_limit_for_ayaml()
}

/// [`expansion`] with an explicit byte budget, so tests can exercise the
/// counting + bail + alias-gate logic with a small budget (fast) instead of
/// materializing millions of nodes at the production ceiling.
fn expansion_with(text: &str, max: usize) -> Expansion {
    if contains_alias(text) {
        expansion_budget_pass(text, max)
    } else {
        Expansion::WithinLimit
    }
}

/// [`expansion_within_limit`] with an explicit byte budget (see
/// [`expansion_with`]).
#[cfg(test)]
fn expansion_within_limit_with(text: &str, max: usize) -> bool {
    expansion_with(text, max).within_limit_for_ayaml()
}

/// The discard-only counting pass of [`expansion`], for text that holds an
/// alias.
fn expansion_budget_pass(text: &str, max: usize) -> Expansion {
    let remaining = std::cell::Cell::new(max);
    let exceeded = std::cell::Cell::new(false);
    let seed = NodeBudget {
        remaining: &remaining,
        exceeded: &exceeded,
    };
    match crate::yaml::ayaml_from_str_seed(text, seed) {
        Ok(()) if !exceeded.get() => Expansion::WithinLimit,
        Ok(()) => Expansion::OverLimit,
        // D28: ayaml enforces its own alias budget before the visitor sees
        // every node. Its alias-expansion limit error therefore means the
        // document is over budget, exactly like a NodeBudget overflow.
        Err(error)
            if exceeded.get()
                || error
                    .limit()
                    .is_some_and(|limit| limit.kind() == ayaml::LimitKind::AliasExpansion) =>
        {
            Expansion::OverLimit
        }
        // Any other refusal (a syntax error, the input cap, the document
        // count) came before the expansion was counted.
        Err(error) => Expansion::Unparsed(error.to_string()),
    }
}

/// Both YAML guards from one lexer pass. Callers that checked
/// [`flow_depth_within_limit`] and then [`expansion_within_limit`] tokenized the
/// whole document twice; this scans it once (skipping the scan entirely when
/// neither `[`/`{` nor `*` occurs) and defers the recursive expansion budget
/// pass to [`GuardScan::expansion_within_limit`], so a caller can still run it
/// only after its own depth checks, exactly where it ran before.
#[derive(Debug, Clone, Copy)]
pub struct GuardScan {
    flow_within_limit: bool,
    has_alias: bool,
}

impl GuardScan {
    /// Same verdict as [`flow_depth_within_limit`].
    #[must_use]
    pub const fn flow_within_limit(&self) -> bool {
        self.flow_within_limit
    }

    /// Same verdict as [`expansion_within_limit`] for the scanned `text`: for
    /// a caller whose real parse is ayaml.
    #[must_use]
    pub fn expansion_within_limit(&self, text: &str) -> bool {
        self.expansion(text).within_limit_for_ayaml()
    }

    /// Same verdict as [`expansion`] for the scanned `text`. A caller that
    /// reads the text with a parser other than ayaml refuses
    /// [`Expansion::Unparsed`] as well as [`Expansion::OverLimit`].
    #[must_use]
    pub fn expansion(&self, text: &str) -> Expansion {
        if self.has_alias {
            expansion_budget_pass(text, expansion_budget(text.len()))
        } else {
            Expansion::WithinLimit
        }
    }
}

/// Scan `text` once for both guards; see [`GuardScan`].
#[must_use]
pub fn scan_guards(text: &str) -> GuardScan {
    scan_guards_with(text, MAX_YAML_FLOW_DEPTH)
}

fn scan_guards_with(text: &str, max_depth: usize) -> GuardScan {
    let has_flow = text.bytes().any(|b| b == b'[' || b == b'{');
    let has_star = text.as_bytes().contains(&b'*');
    let mut scan = GuardScan {
        flow_within_limit: true,
        has_alias: false,
    };
    if !has_flow && !has_star {
        return scan;
    }
    lex(text, |tok| {
        match tok {
            Tok::FlowOpen(depth) if depth > max_depth => scan.flow_within_limit = false,
            Tok::Alias => scan.has_alias = true,
            Tok::FlowOpen(_) => {}
        }
        // Too deep decides the scan (callers stop on it); otherwise keep going
        // while either question is still open.
        scan.flow_within_limit && (has_flow || !scan.has_alias)
    });
    scan
}

/// `true` when the text holds a genuine YAML alias token (`*name`), as the
/// parser's scanner would see it -- so a `"**/*.rs"` glob or a `*` inside a comment, block
/// scalar or plain scalar doesn't count.
///
/// Safety property: it must NEVER return `false` when a real alias exists (that
/// would skip the expansion budget pass and re-open the alias-bomb `DoS`). It
/// reports every `*` at a token start, even a malformed one the parser rejects:
/// a false positive merely triggers the (still correct, still bounded) counting
/// pass.
fn contains_alias(text: &str) -> bool {
    // An alias token needs a `*` byte.
    if !text.as_bytes().contains(&b'*') {
        return false;
    }
    let mut found = false;
    lex(text, |tok| {
        if matches!(tok, Tok::Alias) {
            found = true;
        }
        !found
    });
    found
}

/// One open collection of [`mapping_subtree_is_literal`]'s walk.
struct LiteralFrame {
    mapping: bool,
    /// How many keys of the path led here, while they match (`None` once a
    /// key does not); the path's length means inside the subtree.
    candidate: Option<usize>,
    /// The collection has an anchor or a tag.
    indirect: bool,
    /// In a mapping, whether a key or a value comes next.
    due: Due,
}

/// What a mapping of [`mapping_subtree_is_literal`]'s walk reads next.
#[derive(Clone, Copy)]
enum Due {
    Key,
    /// A value, with its candidate depth (see [`LiteralFrame::candidate`]).
    Value(Option<usize>),
}

/// Return whether the mapping value at `path` contains only direct YAML nodes.
///
/// Compatibility metadata uses this after the cheap complexity guards to reject
/// anchors, aliases, explicit tags, and merge keys before typed deserialization.
/// The rest of the document is not restricted. An absent path is literal.
///
/// It reads the text as alint decodes it (ayaml), so the verdict cannot depend
/// on another parser's reading, and it walks the events with a stack of its own
/// rather than recursion: a document nested deeper than
/// [`MAX_YAML_FLOW_DEPTH`] is an error, never a deep native stack.
///
/// # Errors
///
/// When the text does not parse, or nests deeper than [`MAX_YAML_FLOW_DEPTH`].
pub fn mapping_subtree_is_literal(text: &str, path: &[&str]) -> Result<bool, String> {
    use ayaml::Event;

    if path.is_empty() {
        return Ok(true);
    }
    let events = crate::yaml::stream_events(text).map_err(|error| error.to_string())?;
    // Anchor names that name a scalar, so far: an alias key reads as its text.
    let mut scalar_anchors = std::collections::HashMap::<&str, &str>::new();
    let mut stack: Vec<LiteralFrame> = Vec::new();
    let mut literal = true;
    for event in &events {
        let candidate = match stack.last() {
            None => Some(0),
            Some(frame) => match frame.due {
                Due::Value(value) => value,
                Due::Key => (frame.candidate == Some(path.len())).then_some(path.len()),
            },
        };
        let inside = candidate == Some(path.len());
        // The node just completed: its text (a scalar's, or the scalar an
        // alias names) and whether it has an anchor, a tag, or is an alias.
        let (value, indirect) = match event {
            Event::Scalar {
                value, anchor, tag, ..
            } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.insert(anchor.as_str(), value.as_ref());
                }
                (Some(value.as_ref()), anchor.is_some() || tag.is_some())
            }
            Event::Alias { anchor, .. } => {
                literal &= !inside;
                (scalar_anchors.get(anchor.as_str()).copied(), true)
            }
            Event::SequenceStart { anchor, tag, .. } | Event::MappingStart { anchor, tag, .. } => {
                if let Some(anchor) = anchor {
                    scalar_anchors.remove(anchor.as_str());
                }
                let indirect = anchor.is_some() || tag.is_some();
                literal &= !(inside && indirect);
                stack.push(LiteralFrame {
                    mapping: matches!(event, Event::MappingStart { .. }),
                    candidate,
                    indirect,
                    due: Due::Key,
                });
                if stack.len() > MAX_YAML_FLOW_DEPTH {
                    return Err(format!(
                        "YAML nesting exceeds the maximum supported depth ({MAX_YAML_FLOW_DEPTH})"
                    ));
                }
                continue;
            }
            Event::SequenceEnd { .. } | Event::MappingEnd { .. } => {
                let Some(frame) = stack.pop() else {
                    return Err("unbalanced YAML event stream".to_string());
                };
                (None, frame.indirect)
            }
            _ => continue,
        };
        if matches!(event, Event::Scalar { .. }) {
            literal &= !(inside && indirect);
        }
        let Some(parent) = stack.last_mut() else {
            // The first document's root is complete.
            return Ok(literal);
        };
        if !parent.mapping {
            continue;
        }
        if matches!(std::mem::replace(&mut parent.due, Due::Key), Due::Value(_)) {
            // That was a value; a key is due.
            continue;
        }
        // That was a key: decide what its value is.
        let parent_inside = parent.candidate == Some(path.len());
        literal &= !(parent_inside && value == Some("<<"));
        parent.due = Due::Value(match parent.candidate {
            Some(depth) if depth == path.len() => Some(depth),
            Some(depth) if depth < path.len() && value == Some(path[depth]) => {
                literal &= !indirect;
                Some(depth + 1)
            }
            _ => None,
        });
    }
    Ok(true)
}

/// A structural event the [`Lexer`] reports to its consumer.
#[derive(Debug, Clone, Copy)]
enum Tok {
    /// A flow collection opened; the payload is the new flow depth.
    FlowOpen(usize),
    /// An alias token (`*name`).
    Alias,
}

/// The position of the (only) simple-key candidate the lexer tracks: the one at
/// flow level 0, which decides the column the scanner rolls the block indent to when
/// its `:` arrives. Flow-level keys never move the indent, so they're not kept.
#[derive(Debug, Clone, Copy)]
struct KeyMark {
    line: usize,
    col: isize,
}

/// Run `emit` over the tokens of `text`, in each way a parser alint uses may
/// read a byte order mark that starts a line, until `emit` returns `false`. A
/// text without a byte order mark has one reading.
fn lex(text: &str, mut emit: impl FnMut(Tok) -> bool) {
    let mut open = true;
    for bom in [Bom::Column, Bom::Stripped, Bom::Ayaml] {
        let needed = match bom {
            Bom::Column => true,
            Bom::Stripped => text.contains('\u{feff}'),
            Bom::Ayaml => text.starts_with('\u{feff}'),
        };
        if open && needed {
            Lexer::new(text, bom).run(|tok| {
                open = emit(tok);
                open
            });
        }
    }
}

/// How a [`Lexer`] reads a byte order mark that starts a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bom {
    /// Every one takes a column, as libyaml reads them. So does ayaml, for one
    /// that starts any line but the first, and for the `---` check after the
    /// one that starts the text: `\u{feff}---` is text to it.
    Column,
    /// Every one has no width: the text saphyr parses, after alint strips the
    /// marks that start it.
    Stripped,
    /// As ayaml reads a text that starts with one: that mark has no width, so
    /// the first line's indentation counts from column 0 and a `...` after it
    /// ends a document (one that more text follows on its line is text to
    /// ayaml, as to libyaml, so there this reading counts more than ayaml);
    /// another mark right after it is content (asamarts/ayaml#140); and one
    /// that starts a later line takes a column.
    Ayaml,
}

/// The token-boundary logic of a YAML scanner, ported from libyaml-rs 0.3.0
/// (`yaml_parser_fetch_next_token` and the `scan_*` routines it calls; the
/// methods below keep those names) and adjusted to read as ayaml does (see the
/// module docs). It tracks exactly the state that decides where tokens begin
/// and end -- flow level, the block indent stack, `simple_key_allowed`, and the
/// level-0 simple key -- and skips scalar / comment CONTENTS the way the parser
/// does, reporting flow opens and aliases. Syntax errors are not modelled: the
/// parser stops at one, so the lexer just keeps going (see the module docs).
/// Columns count characters, as the parser's marks do.
#[derive(Debug)]
struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
    col: isize,
    flow: usize,
    indent: isize,
    indents: Vec<isize>,
    simple_key_allowed: bool,
    key: Option<KeyMark>,
    /// The token just scanned was a JSON-like node (a quoted scalar or a
    /// closed flow collection), after which a flow `:` is a value indicator
    /// even with no space after it.
    after_json_node: bool,
    /// How a byte order mark that starts a line reads (see [`lex`]).
    bom: Bom,
    /// An anchor or alias name has ended where ayaml's scanner and the line
    /// reader it decides indentation with disagree (see `scan_anchor`).
    distrust: bool,
    /// The anchor names so far in this document that end in `:`, which an
    /// alias keeps whole.
    colon_anchors: std::collections::HashSet<&'a [u8]>,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str, bom: Bom) -> Self {
        // No BOM stripping here: `scan_to_next_token` skips a BOM that starts
        // a line, and in the reading that gives it a column (`Bom::Column`) a
        // `---` right after it sits at column 1 and is not a document marker.
        // Stripping the BOM up front left the lexer one column behind the
        // parser on line 1, which hid flow bombs and aliases behind a
        // `\u{feff}---`.
        Self {
            b: text.as_bytes(),
            i: 0,
            line: 0,
            col: 0,
            flow: 0,
            indent: -1,
            indents: Vec::new(),
            simple_key_allowed: true,
            key: None,
            after_json_node: false,
            bom,
            distrust: false,
            colon_anchors: std::collections::HashSet::new(),
        }
    }

    /// Byte at `i + k`, or `0` past the end (a NUL-terminated buffer, as in the
    /// scanner the lexer was ported from).
    fn at(&self, k: usize) -> u8 {
        self.b.get(self.i + k).copied().unwrap_or(0)
    }

    fn eof(&self) -> bool {
        self.i >= self.b.len()
    }

    /// A line break: CR or LF. NEL, LS and PS are content to ayaml.
    fn is_break_at(&self, k: usize) -> bool {
        matches!(self.at(k), b'\r' | b'\n')
    }

    fn is_blank_at(&self, k: usize) -> bool {
        matches!(self.at(k), b' ' | b'\t')
    }

    fn at_nel(&self) -> bool {
        self.b[self.i.min(self.b.len())..].starts_with("\u{85}".as_bytes())
    }

    fn is_breakz_at(&self, k: usize) -> bool {
        self.i + k >= self.b.len() || self.is_break_at(k)
    }

    fn is_blankz_at(&self, k: usize) -> bool {
        self.is_blank_at(k) || self.is_breakz_at(k)
    }

    /// `---` / `...` at column 0 followed by a blank or end of line.
    fn at_document_marker(&self) -> bool {
        self.col == 0
            && ((self.at(0) == b'-' && self.at(1) == b'-' && self.at(2) == b'-')
                || (self.at(0) == b'.' && self.at(1) == b'.' && self.at(2) == b'.'))
            && self.is_blankz_at(3)
    }

    /// `SKIP`: advance one character (its UTF-8 width), one column.
    fn advance(&mut self) {
        if self.eof() {
            return;
        }
        let w = match self.b[self.i] {
            c if c & 0x80 == 0 => 1,
            c if c & 0xE0 == 0xC0 => 2,
            c if c & 0xF0 == 0xE0 => 3,
            c if c & 0xF8 == 0xF0 => 4,
            _ => 1,
        };
        self.i = (self.i + w).min(self.b.len());
        self.col += 1;
    }

    /// `SKIP_LINE`: consume one line break (CRLF as one).
    fn skip_line(&mut self) {
        if self.at(0) == b'\r' && self.at(1) == b'\n' {
            self.i += 2;
        } else if self.is_break_at(0) {
            self.advance();
        } else {
            return;
        }
        self.col = 0;
        self.line += 1;
    }

    fn roll_indent(&mut self, col: isize) {
        if self.flow == 0 && self.indent < col {
            self.indents.push(self.indent);
            self.indent = col;
        }
    }

    fn unroll_indent(&mut self, col: isize) {
        if self.flow != 0 {
            return;
        }
        while self.indent > col {
            self.indent = self.indents.pop().unwrap_or(-1);
        }
    }

    fn save_simple_key(&mut self) {
        if self.flow == 0 && self.simple_key_allowed {
            self.key = Some(KeyMark {
                line: self.line,
                col: self.col,
            });
        }
    }

    fn remove_simple_key(&mut self) {
        if self.flow == 0 {
            self.key = None;
        }
    }

    /// Run the scanner to the end (or until `emit` returns `false`).
    fn run(mut self, mut emit: impl FnMut(Tok) -> bool) {
        loop {
            if self.distrust {
                self.assume_the_worst(emit);
                return;
            }
            self.scan_to_next_token();
            // `yaml_parser_stale_simple_keys`, by ayaml's rule: a key candidate
            // expires at a new line or more than 1024 characters on, as ayaml
            // refuses a key whose `:` is further from it (`implicit_key_fits`).
            if self
                .key
                .is_some_and(|k| k.line < self.line || k.col + 1024 < self.col)
            {
                self.key = None;
            }
            self.unroll_indent(self.col);
            if self.eof() {
                return;
            }
            let c = self.at(0);
            let start = self.i;
            let after_json_node = std::mem::take(&mut self.after_json_node);
            if self.col == 0 && c == b'%' {
                // A directive (`%YAML` / `%TAG`) runs to the end of its line.
                self.unroll_indent(-1);
                self.remove_simple_key();
                self.simple_key_allowed = false;
                while !self.is_breakz_at(0) {
                    self.advance();
                }
            } else if self.at_document_marker() {
                self.unroll_indent(-1);
                self.remove_simple_key();
                self.simple_key_allowed = false;
                // An alias keeps a final `:` only for an anchor of its own
                // document, as ayaml forgets the names at either marker.
                self.colon_anchors.clear();
                for _ in 0..3 {
                    self.advance();
                }
            } else if c == b'[' || c == b'{' {
                self.save_simple_key();
                self.flow += 1;
                self.simple_key_allowed = true;
                self.advance();
                if !emit(Tok::FlowOpen(self.flow)) {
                    return;
                }
            } else if c == b']' || c == b'}' {
                self.remove_simple_key();
                self.flow = self.flow.saturating_sub(1);
                self.simple_key_allowed = false;
                self.advance();
                self.after_json_node = true;
            } else if c == b',' {
                self.remove_simple_key();
                self.simple_key_allowed = true;
                self.advance();
            } else if c == b'-' && self.is_blankz_at(1) {
                self.roll_indent(self.col);
                self.remove_simple_key();
                self.simple_key_allowed = true;
                self.advance();
            } else if c == b'?' && self.is_key_indicator_at() {
                self.fetch_key();
            } else if c == b':'
                && (self.is_value_indicator_at() || (self.flow != 0 && after_json_node))
            {
                self.fetch_value();
            } else if c == b'*' || c == b'&' {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.scan_anchor(c == b'*');
                if c == b'*' && !emit(Tok::Alias) {
                    return;
                }
            } else if c == b'!' {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.scan_tag();
            } else if (c == b'|' || c == b'>') && self.flow == 0 {
                self.remove_simple_key();
                self.simple_key_allowed = true;
                self.scan_block_scalar();
            } else if c == b'\'' || c == b'"' {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.scan_flow_scalar(c == b'\'');
                self.after_json_node = true;
            } else if self.can_start_plain(c) {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.scan_plain_scalar();
            } else {
                // A character that cannot start a token: the parser stops
                // here. Step over it and keep scanning (never hides input).
                self.advance();
            }
            // Every token consumes input. One that did not would be read
            // again forever, so a scanner bug fails tests, and in a release
            // build the scanner steps over the byte rather than hang.
            debug_assert!(
                self.i > start,
                "the scanner made no progress at byte {start}"
            );
            if self.i == start {
                self.advance();
            }
        }
    }

    /// After a name ayaml reads two ways (see `scan_anchor`), what follows
    /// has no reading to follow: count each later `[` or `{` as a level that
    /// never closes, and each `*` as an alias.
    fn assume_the_worst(&self, mut emit: impl FnMut(Tok) -> bool) {
        let mut depth = self.flow;
        for &byte in &self.b[self.i..] {
            let tok = match byte {
                b'[' | b'{' => {
                    depth += 1;
                    Tok::FlowOpen(depth)
                }
                b'*' => Tok::Alias,
                _ => continue,
            };
            if !emit(tok) {
                return;
            }
        }
    }

    /// `yaml_parser_scan_to_next_token`: skip blanks, a comment (a `#` at any
    /// token start), and line breaks; a line break in block context re-allows a
    /// simple key. (Tabs are skipped unconditionally: where the parser refuses
    /// one it errors and stops.)
    fn scan_to_next_token(&mut self) {
        loop {
            // A BOM at column 0 is skipped, or read as content (see `Bom`).
            // Read as a character (the column advances to 1), a second BOM
            // starts a plain scalar. So does a second one in ayaml's reading,
            // which looks for a mark once per line: the scan returns before
            // the one right after the first (asamarts/ayaml#140).
            if self.col == 0 && self.b[self.i.min(self.b.len())..].starts_with(b"\xEF\xBB\xBF") {
                match self.bom {
                    Bom::Stripped => {
                        while self.b[self.i..].starts_with(b"\xEF\xBB\xBF") {
                            self.i += 3;
                        }
                    }
                    Bom::Ayaml if self.i == 0 => self.i += 3,
                    Bom::Column | Bom::Ayaml => self.advance(),
                }
            }
            while self.is_blank_at(0) {
                self.advance();
            }
            if self.at(0) == b'#' {
                while !self.is_breakz_at(0) {
                    self.advance();
                }
            }
            if !self.is_break_at(0) {
                return;
            }
            self.skip_line();
            if self.flow == 0 {
                self.simple_key_allowed = true;
            }
        }
    }

    /// `yaml_parser_fetch_key`: a `?` key indicator. A comment right after a
    /// flow `?` ends at a NEL too, as ayaml reads it, and it reads on after the
    /// NEL on the same line (asamarts/ayaml#141).
    fn fetch_key(&mut self) {
        self.roll_indent(self.col);
        self.remove_simple_key();
        self.simple_key_allowed = self.flow == 0;
        self.advance();
        if self.flow != 0 && self.at(0) == b'#' {
            while !self.is_breakz_at(0) && !self.at_nel() {
                self.advance();
            }
            if self.at_nel() {
                self.advance();
            }
        }
    }

    /// `yaml_parser_fetch_value`: a `:` value indicator. In block context it rolls
    /// the indent to the pending simple key's column (or, with none, to the
    /// indicator's own column, as after a `?` complex key).
    fn fetch_value(&mut self) {
        if self.flow == 0 {
            if let Some(k) = self.key.take() {
                self.roll_indent(k.col);
                self.simple_key_allowed = false;
            } else {
                self.roll_indent(self.col);
                self.simple_key_allowed = true;
            }
        } else {
            self.simple_key_allowed = false;
        }
        self.advance();
    }

    /// The plain-scalar start test at the end of `yaml_parser_fetch_next_token`.
    fn can_start_plain(&self, c: u8) -> bool {
        let indicator = self.is_blankz_at(0)
            || matches!(
                c,
                b'-' | b'?'
                    | b':'
                    | b','
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'#'
                    | b'&'
                    | b'*'
                    | b'!'
                    | b'|'
                    | b'>'
                    | b'\''
                    | b'"'
                    | b'%'
                    | b'@'
                    | b'`'
            );
        !indicator
            || (c == b'-' && !self.is_blank_at(1))
            || (c == b'?' && !self.is_key_indicator_at())
            || (c == b':' && !self.is_value_indicator_at())
    }

    /// Whether the `?` at the cursor is the key indicator rather than the start
    /// of a plain scalar: it is in a flow collection, where ayaml's
    /// compatibility reader keeps libyaml's rule (`[?a]` is the key `a`), and
    /// elsewhere when a blank or a line break follows it. ayaml refuses a flow
    /// `?` right before a flow indicator (`[?[a]]`); the lexer reads the key
    /// indicator there, and the parser stops at it.
    fn is_key_indicator_at(&self) -> bool {
        self.flow != 0 || self.is_blankz_at(1)
    }

    /// Whether the `:` at the cursor is the value indicator rather than the
    /// start of a plain scalar: a blank or a line break follows it, or, in a
    /// flow collection, a flow indicator does. ayaml takes a `,`, `]` or `}`
    /// after it there (YAML 1.2: `[a:b]` is one scalar), and refuses a `[` or
    /// `{` (`[a:[b]]`), unless the `:` follows a quoted scalar or a closed
    /// collection; the lexer reads the value indicator before those too, so it
    /// counts the collection the parser stops at.
    fn is_value_indicator_at(&self) -> bool {
        self.is_blankz_at(1) || (self.flow != 0 && self.is_flow_indicator_at(1))
    }

    /// `scan_anchor` as ayaml's compatibility reader runs it: an anchor or alias
    /// name runs to a blank, a line break or a flow indicator, so `&a"` names
    /// `a"` rather than opening a quoted scalar. Like libyaml, it stops at a `?`
    /// that no flow indicator follows, and, outside a flow collection, at a `:`
    /// after the name's first character that a character other than a blank or a
    /// line break follows. An alias name that ends in `:` gives the `:` back
    /// unless an anchor before it in its document has that whole name.
    ///
    /// The line reader ayaml decides indentation with does not stop at a `:`
    /// that a flow indicator follows, so for `&a:[: x` it reads an unclosed
    /// `[` where the scanner reads the key `:[`, and lets later lines continue
    /// the value at any indentation (asamarts/ayaml#139). The lexer does not
    /// mirror that: it assumes the worst about the rest of the text (see
    /// `run`).
    fn scan_anchor(&mut self, alias: bool) {
        self.advance();
        let start = self.i;
        while !self.is_blankz_at(0) && !self.is_flow_indicator_at(0) {
            let stop = match self.at(0) {
                b'?' => !self.is_flow_indicator_at(1),
                b':' => {
                    self.flow == 0
                        && self.i > start
                        && !self.is_blankz_at(1)
                        && !self.b[self.i + 1..].starts_with("\u{85}".as_bytes())
                        && !self.b[self.i + 1..].starts_with("\u{2028}".as_bytes())
                        && !self.b[self.i + 1..].starts_with("\u{2029}".as_bytes())
                }
                _ => false,
            };
            if stop {
                self.distrust |= self.at(0) == b':' && self.is_flow_indicator_at(1);
                break;
            }
            self.advance();
        }
        let name = &self.b[start..self.i];
        if name.last() == Some(&b':') {
            if !alias {
                self.colon_anchors.insert(name);
            } else if name.len() > 1 && !self.colon_anchors.contains(name) {
                self.i -= 1;
                self.col -= 1;
            }
        }
    }

    /// A flow indicator: `,`, `[`, `]`, `{` or `}`.
    fn is_flow_indicator_at(&self, k: usize) -> bool {
        matches!(self.at(k), b',' | b'[' | b']' | b'{' | b'}')
    }

    /// `yaml_parser_scan_tag`: `!<verbatim-uri>` or `!handle!suffix` / `!suffix`.
    /// The token runs over ayaml's characters for each: a verbatim tag over URI
    /// characters (`is_uri_char`), a shorthand over tag characters
    /// (`is_tag_char`: the URI characters but `!`, `,`, `[` and `]`) and the `!`s
    /// of its handle. A `#` is one of them, so `!a# [b]` is the tag `!a#`, not a
    /// tag and a comment.
    fn scan_tag(&mut self) {
        let verbatim = self.at(1) == b'<';
        self.advance();
        if verbatim {
            self.advance();
        }
        loop {
            let c = self.at(0);
            let uri = c.is_ascii_alphanumeric()
                || b"-#_;/?:@&=+$.%!~*'()".contains(&c)
                || (verbatim && matches!(c, b',' | b'[' | b']'));
            if !uri || self.eof() {
                break;
            }
            self.advance();
        }
        if verbatim && self.at(0) == b'>' {
            self.advance();
        }
    }

    /// `yaml_parser_scan_flow_scalar`: a single- or double-quoted scalar, which
    /// may span lines. `''` escapes a single quote; `\` escapes one character in
    /// double quotes (an escaped line break is a continuation).
    fn scan_flow_scalar(&mut self, single: bool) {
        let quote = if single { b'\'' } else { b'"' };
        self.advance();
        loop {
            if self.eof() {
                return;
            }
            while !self.is_blankz_at(0) {
                if single && self.at(0) == b'\'' && self.at(1) == b'\'' {
                    self.advance();
                    self.advance();
                } else if self.at(0) == quote {
                    self.advance();
                    return;
                } else if !single && self.at(0) == b'\\' && self.is_break_at(1) {
                    self.advance();
                    self.skip_line();
                    break;
                } else if !single && self.at(0) == b'\\' {
                    self.advance();
                    self.advance();
                } else {
                    self.advance();
                }
            }
            while self.is_blank_at(0) || self.is_break_at(0) {
                if self.is_blank_at(0) {
                    self.advance();
                } else {
                    self.skip_line();
                }
            }
        }
    }

    /// `yaml_parser_scan_plain_scalar`: ends before a `:` value indicator, ` #`,
    /// a flow indicator (in flow context), a document marker, or -- in block
    /// context -- a continuation line indented less than the block indent + 1.
    /// Consumes the trailing blanks / breaks, and re-allows a simple key if it
    /// ended on a line break.
    fn scan_plain_scalar(&mut self) {
        let min_col = self.indent + 1;
        let mut leading_blanks = false;
        loop {
            if self.at_document_marker() || self.at(0) == b'#' {
                break;
            }
            while !self.is_blankz_at(0) {
                let c = self.at(0);
                // In a flow collection a `:` before any other character is
                // content, as in `[a:?b]`, the one scalar `a:?b` to ayaml, where
                // libyaml stopped with "found unexpected ':'".
                if (c == b':' && self.is_value_indicator_at())
                    || (self.flow != 0 && self.is_flow_indicator_at(0))
                {
                    break;
                }
                leading_blanks = false;
                self.advance();
            }
            if !(self.is_blank_at(0) || self.is_break_at(0)) {
                break;
            }
            while self.is_blank_at(0) || self.is_break_at(0) {
                if self.is_blank_at(0) {
                    self.advance();
                } else {
                    self.skip_line();
                    leading_blanks = true;
                }
            }
            if self.flow == 0 && self.col < min_col {
                break;
            }
        }
        if leading_blanks {
            self.simple_key_allowed = true;
        }
    }

    /// `yaml_parser_scan_block_scalar`: the `|` / `>` header (chomping and
    /// indentation indicators, an optional comment), then every line indented to
    /// the content indent -- explicit (`block indent + N`) or auto-detected from
    /// the first non-empty line (never less than the block indent + 1).
    fn scan_block_scalar(&mut self) {
        self.advance();
        let mut increment: isize = 0;
        for _ in 0..2 {
            match self.at(0) {
                b'+' | b'-' => self.advance(),
                d @ b'1'..=b'9' if increment == 0 => {
                    increment = isize::from(d - b'0');
                    self.advance();
                }
                _ => break,
            }
        }
        while self.is_blank_at(0) {
            self.advance();
        }
        if self.at(0) == b'#' {
            while !self.is_breakz_at(0) {
                self.advance();
            }
        }
        if !self.is_breakz_at(0) {
            // The parser expects a comment or a line break here; it stops.
            return;
        }
        self.skip_line();
        let mut content_indent: isize = if increment == 0 {
            0
        } else if self.indent >= 0 {
            self.indent + increment
        } else {
            increment
        };
        self.block_scalar_breaks(&mut content_indent);
        while self.col == content_indent && !self.eof() {
            while !self.is_breakz_at(0) {
                self.advance();
            }
            self.block_scalar_breaks(&mut content_indent);
        }
    }

    /// `yaml_parser_scan_block_scalar_breaks`: skip indentation (up to the content
    /// indent once known) and empty lines; on first use, auto-detect the content
    /// indent from the most-indented leading line.
    fn block_scalar_breaks(&mut self, content_indent: &mut isize) {
        let mut max_indent: isize = 0;
        loop {
            while (*content_indent == 0 || self.col < *content_indent) && self.at(0) == b' ' {
                self.advance();
            }
            max_indent = max_indent.max(self.col);
            if !self.is_break_at(0) {
                break;
            }
            self.skip_line();
        }
        if *content_indent == 0 {
            *content_indent = max_indent.max(self.indent + 1).max(1);
        }
    }
}

/// A discard-only `serde` seed that charges every node it visits
/// ([`YAML_NODE_COST`]) and every scalar's text (its length) against a shared
/// byte budget, flagging + erroring the instant it underflows. Used by
/// [`expansion_within_limit`] to bound YAML alias expansion without materializing a
/// value. `Copy` (it holds only shared `Cell` refs), so it seeds child nodes freely.
#[derive(Clone, Copy)]
pub(crate) struct NodeBudget<'a> {
    pub(crate) remaining: &'a std::cell::Cell<usize>,
    pub(crate) exceeded: &'a std::cell::Cell<bool>,
}

impl NodeBudget<'_> {
    fn charge<E: serde::de::Error>(self, cost: usize) -> Result<(), E> {
        let Some(n) = self.remaining.get().checked_sub(cost) else {
            self.exceeded.set(true);
            return Err(E::custom(
                "YAML alias expansion exceeds the maximum supported size",
            ));
        };
        self.remaining.set(n);
        Ok(())
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for NodeBudget<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        self.charge::<D::Error>(YAML_NODE_COST)?;
        d.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for NodeBudget<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a YAML node")
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(self)?.is_some() {}
        Ok(())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_key_seed(self)?.is_some() {
            map.next_value_seed(self)?;
        }
        Ok(())
    }
    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        use serde::de::VariantAccess as _;
        let ((), variant) = data.variant_seed(self)?;
        variant.newtype_variant_seed(self)
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        serde::de::DeserializeSeed::deserialize(self, d)
    }
    fn visit_newtype_struct<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        serde::de::DeserializeSeed::deserialize(self, d)
    }
    // Every node is charged once in `deserialize` above; a string scalar (a value,
    // a key, or a tag name) is additionally charged its length, since each alias
    // replay copies it into the materialized tree (`visit_borrowed_str` /
    // `visit_string` forward here). Fixed-size scalars cost no more.
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<(), E> {
        self.charge(v.len())
    }
    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<(), E> {
        self.charge(v.len())
    }
    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_i128<E>(self, _: i128) -> Result<(), E> {
        Ok(())
    }
    fn visit_u128<E>(self, _: u128) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "yaml_depth_tests.rs"]
mod tests;
