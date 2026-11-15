//! Pre-parse bounds on YAML flow-collection nesting depth and alias use.
//!
//! The parser bounds both itself: ayaml stops at 128 levels of nesting and at
//! an alias-expansion budget, in time linear in the input, under the limits
//! [`crate::yaml`] sets from this module's constants. These guards scan the raw
//! text before any parse and refuse a document past either bound with alint's
//! own error. They were written for the parser v0.22 used (libyaml-rs through
//! `yaml_serde`), whose tokenizer was super-linear on deep flow nesting (a
//! 40 KB `[`x20000 document took seconds) and which did not bound one anchor
//! replayed many times. They stay as defense in depth (`docs/design/yaml-parser.md`,
//! "Security and resource invariants"): a bomb is refused before any parser
//! reads it, whichever parser that is. saphyr, which reads YAML for source spans
//! and structured fixes, stops at 256 levels in linear time of its own.
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
//! breaks; in a flow collection a `:` is the value indicator only before a
//! blank or a flow indicator, or right after a quoted scalar or a closed
//! collection (a `?` there is always the key indicator, as in libyaml); an
//! anchor or alias name runs to a blank or a flow indicator; and a byte order
//! mark that starts a line is read both ways (see `lex`). Where
//! the parser would raise a syntax error the lexer keeps going: the parser
//! stops there, so nothing after the error point is ever parsed, and continuing
//! (rather than stopping) never hides input. A differential property test
//! against ayaml pins the parity.

/// Real config/manifest YAML nests a handful of flow levels; anything past this
/// is a bomb. It is also the parser's depth limit ([`crate::yaml`] sets it): the
/// parser allows 128 nested collections (block or flow) and fails the 129th, so
/// a document whose flow nesting alone exceeds 128 can never deserialize into a
/// value anyway -- rejecting it here introduces no new false reject. (The one
/// exception is a subtree serde IGNORES -- `IgnoredAny` skips events without a
/// depth check -- which no alint path relies on: configs are
/// `deny_unknown_fields`, and every structured query builds the whole tree.)
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
/// refs make only N derefs but N x (anchor size) materializations. ayaml charges
/// the same budget as this guard ([`crate::yaml`] sets it from these constants)
/// and stops at it. Measured on the parser v0.22 used (release,
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
/// counted up to the structured-parse byte cap so padding a config body (which
/// has no such cap) cannot buy more: the budget never exceeds ~768 MiB.
#[must_use]
pub fn expansion_budget(len: usize) -> usize {
    let len = len.min(crate::structured_format::MAX_STRUCTURED_BYTES);
    MAX_YAML_EXPANSION_BYTES + len * YAML_EXPANSION_BYTES_PER_INPUT_BYTE
}

/// `true` when the YAML text's ALIAS expansion stays within its
/// [`expansion_budget`]. Only a document that actually uses an alias
/// (`*name`) can amplify, so alias-free text short-circuits to `true` at zero cost
/// (its node count is ~linear in bytes, already bounded by the read cap). For
/// alias-bearing text a cheap DISCARD-ONLY pass drives the parser's
/// deserializer, which replays anchored events through the visitor -- so the
/// expansion is charged ([`YAML_NODE_COST`] per node plus every scalar's bytes,
/// exactly the copies the real parse would make) and the pass bails once the
/// budget is exceeded (measured, release: both reported bombs -- 7.5M map
/// nodes, and 4000 copies of a 1 MB scalar -- are rejected in well under a
/// second without allocating). It builds NO value, so it cannot change the real
/// parse's output; the caller runs the real parse only after this returns
/// `true`. A non-budget deserialize error (malformed YAML, or an unusual node the
/// counter doesn't model) is ignored -- only a genuine budget overflow returns
/// `false`, and everything else falls through to the real parse, which produces
/// the proper error. This is fail-safe: a real bomb is ordinary scalars /
/// sequences / maps (and tagged nodes, via `visit_enum`) and is counted.
#[must_use]
pub fn expansion_within_limit(text: &str) -> bool {
    expansion_within_limit_with(text, expansion_budget(text.len()))
}

/// [`expansion_within_limit`] with an explicit byte budget, so tests can exercise
/// the counting + bail + alias-gate logic with a small budget (fast) instead of
/// materializing millions of nodes at the production ceiling.
fn expansion_within_limit_with(text: &str, max: usize) -> bool {
    !contains_alias(text) || expansion_budget_pass(text, max)
}

/// The discard-only counting pass of [`expansion_within_limit`], for text that
/// holds an alias.
fn expansion_budget_pass(text: &str, max: usize) -> bool {
    let remaining = std::cell::Cell::new(max);
    let exceeded = std::cell::Cell::new(false);
    let seed = NodeBudget {
        remaining: &remaining,
        exceeded: &exceeded,
    };
    let result = crate::yaml::from_str_seed(text, seed);
    // D28: ayaml enforces its own alias budget before the visitor sees every
    // node. An ayaml limit error therefore means the document is over budget;
    // treat it exactly like a NodeBudget overflow instead of letting the
    // early error fall through as "within limit".
    if result.is_err_and(|error| matches!(error.kind(), ayaml::ErrorKind::Limit)) {
        exceeded.set(true);
    }
    !exceeded.get()
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

    /// Same verdict as [`expansion_within_limit`] for the scanned `text`.
    #[must_use]
    pub fn expansion_within_limit(&self, text: &str) -> bool {
        !self.has_alias || expansion_budget_pass(text, expansion_budget(text.len()))
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

#[derive(Clone, Copy)]
struct LiteralScalarNode<'a> {
    value: Option<&'a str>,
    indirect: bool,
}

fn walk_literal_subtree<'event>(
    events: &'event [(saphyr_parser::Event<'_>, saphyr_parser::Span)],
    scalar_anchors: &std::collections::HashMap<usize, &'event str>,
    index: &mut usize,
    candidate_depth: Option<usize>,
    target: &[&str],
    literal: &mut bool,
) -> Result<LiteralScalarNode<'event>, String> {
    use saphyr_parser::Event;

    let Some((event, _)) = events.get(*index) else {
        return Err("unexpected end of YAML event stream".to_string());
    };
    let inside = candidate_depth == Some(target.len());
    match event {
        Event::Scalar(value, _, anchor, tag) => {
            let indirect = *anchor != 0 || tag.is_some();
            if inside && indirect {
                *literal = false;
            }
            *index += 1;
            Ok(LiteralScalarNode {
                value: Some(value.as_ref()),
                indirect,
            })
        }
        Event::Alias(anchor) => {
            if inside {
                *literal = false;
            }
            *index += 1;
            Ok(LiteralScalarNode {
                value: scalar_anchors.get(anchor).copied(),
                indirect: true,
            })
        }
        Event::SequenceStart(anchor, tag) => {
            let indirect = *anchor != 0 || tag.is_some();
            if inside && indirect {
                *literal = false;
            }
            *index += 1;
            while !matches!(events.get(*index), Some((Event::SequenceEnd, _))) {
                walk_literal_subtree(
                    events,
                    scalar_anchors,
                    index,
                    inside.then_some(target.len()),
                    target,
                    literal,
                )?;
            }
            *index += 1;
            Ok(LiteralScalarNode {
                value: None,
                indirect,
            })
        }
        Event::MappingStart(anchor, tag) => {
            let indirect = *anchor != 0 || tag.is_some();
            if inside && indirect {
                *literal = false;
            }
            *index += 1;
            while !matches!(events.get(*index), Some((Event::MappingEnd, _))) {
                let key = walk_literal_subtree(
                    events,
                    scalar_anchors,
                    index,
                    inside.then_some(target.len()),
                    target,
                    literal,
                )?;
                if inside && key.value == Some("<<") {
                    *literal = false;
                }
                let child_depth = match candidate_depth {
                    Some(depth) if depth == target.len() => Some(depth),
                    Some(depth) if depth < target.len() && key.value == Some(target[depth]) => {
                        if key.indirect {
                            *literal = false;
                        }
                        Some(depth + 1)
                    }
                    _ => None,
                };
                walk_literal_subtree(events, scalar_anchors, index, child_depth, target, literal)?;
            }
            *index += 1;
            Ok(LiteralScalarNode {
                value: None,
                indirect,
            })
        }
        other => Err(format!("unexpected YAML event {other:?} inside a value")),
    }
}

/// Return whether the mapping value at `path` contains only direct YAML nodes.
///
/// Compatibility metadata uses this after the cheap complexity guards to reject
/// anchors, aliases, explicit tags, and merge keys before typed deserialization.
/// The rest of the document is not restricted. An absent path is literal.
pub fn mapping_subtree_is_literal(text: &str, path: &[&str]) -> Result<bool, String> {
    use saphyr_parser::{Event, Parser};

    if path.is_empty() {
        return Ok(true);
    }
    let events = Parser::new_from_str(text)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let scalar_anchors = events
        .iter()
        .filter_map(|(event, _)| match event {
            Event::Scalar(value, _, anchor, _) if *anchor != 0 => Some((*anchor, value.as_ref())),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let Some(mut index) = events.iter().position(|(event, _)| {
        matches!(
            event,
            Event::Scalar(..)
                | Event::Alias(..)
                | Event::SequenceStart(..)
                | Event::MappingStart(..)
        )
    }) else {
        return Ok(true);
    };
    let mut literal = true;
    walk_literal_subtree(
        &events,
        &scalar_anchors,
        &mut index,
        Some(0),
        path,
        &mut literal,
    )?;
    Ok(literal)
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
    index: usize,
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
    /// the first line's indentation, and a `...` after it, count from column 0;
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
    /// The anchor names so far that end in `:`, which an alias keeps whole.
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
                index: self.i,
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
            // `yaml_parser_stale_simple_keys`: a key candidate expires at a new
            // line or 1024 bytes on.
            if self
                .key
                .is_some_and(|k| k.line < self.line || k.index + 1024 < self.i)
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
            // starts a plain scalar.
            if self.col == 0 && self.b[self.i.min(self.b.len())..].starts_with(b"\xEF\xBB\xBF") {
                match self.bom {
                    Bom::Stripped => {
                        while self.b[self.i..].starts_with(b"\xEF\xBB\xBF") {
                            self.i += 3;
                        }
                    }
                    Bom::Ayaml if self.i == 0 => self.i += 3,
                    Bom::Ayaml if self.line == 0 => {}
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
    /// of a plain scalar: it always is in a flow collection, where ayaml's
    /// compatibility reader keeps libyaml's rule (`[?a]` is the key `a`), and
    /// elsewhere when a blank or a line break follows it.
    fn is_key_indicator_at(&self) -> bool {
        self.flow != 0 || self.is_blankz_at(1)
    }

    /// Whether the `:` at the cursor is the value indicator rather than the
    /// start of a plain scalar: a blank or a line break follows it, or, in a
    /// flow collection, a flow indicator does (YAML 1.2, as ayaml reads it:
    /// `[a:b]` is one scalar).
    fn is_value_indicator_at(&self) -> bool {
        self.is_blankz_at(1) || (self.flow != 0 && self.is_flow_indicator_at(1))
    }

    /// `scan_anchor` as ayaml's compatibility reader runs it: an anchor or alias
    /// name runs to a blank, a line break or a flow indicator, so `&a"` names
    /// `a"` rather than opening a quoted scalar. Like libyaml, it stops at a `?`
    /// that a character other than a flow indicator follows, and, outside a flow
    /// collection, at a `:` after the name's first character that a character
    /// other than a blank or a line break follows. An alias name that ends in
    /// `:` gives the `:` back unless an anchor before it has that whole name.
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
                b'?' => self.i + 1 < self.b.len() && !self.is_flow_indicator_at(1),
                b':' => {
                    self.flow == 0
                        && self.i > start
                        && self.i + 1 < self.b.len()
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

    /// `yaml_parser_scan_tag`: `!<verbatim-uri>` or `!handle!suffix` / `!suffix`;
    /// the token is a run of URI characters (`,` `[` `]` only inside `!<…>`).
    fn scan_tag(&mut self) {
        let verbatim = self.at(1) == b'<';
        self.advance();
        if verbatim {
            self.advance();
        }
        loop {
            let c = self.at(0);
            let uri = c.is_ascii_alphanumeric()
                || b"-_;/?:@&=+$.%!~*'()".contains(&c)
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
struct NodeBudget<'a> {
    remaining: &'a std::cell::Cell<usize>,
    exceeded: &'a std::cell::Cell<bool>,
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
mod tests {
    use super::*;

    /// The byte prefilters must never change a verdict: compare them with the
    /// lexer walks they short-circuit, over every string up to length 4 on an
    /// alphabet of YAML's structural characters.
    #[test]
    fn prefilters_agree_with_the_lexer() {
        fn flow_reference(text: &str, max: usize) -> bool {
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
        fn alias_reference(text: &str) -> bool {
            let mut found = false;
            lex(text, |tok| {
                if matches!(tok, Tok::Alias) {
                    found = true;
                }
                !found
            });
            found
        }
        const ALPHABET: [char; 14] = [
            'a', ' ', '\n', ':', '-', '[', ']', '{', '}', '*', '&', '#', '\'', '"',
        ];
        let mut idx = [0usize; 4];
        let mut checked = 0usize;
        for len in 0..=4 {
            idx.fill(0);
            loop {
                let text: String = idx[..len].iter().map(|&i| ALPHABET[i]).collect();
                for max in [0, 1] {
                    assert_eq!(
                        flow_depth_within_limit_with(&text, max),
                        flow_reference(&text, max),
                        "{text:?} max={max}"
                    );
                }
                assert_eq!(contains_alias(&text), alias_reference(&text), "{text:?}");
                let scan = scan_guards_with(&text, 1);
                assert_eq!(
                    scan.flow_within_limit(),
                    flow_reference(&text, 1),
                    "{text:?}"
                );
                if scan.flow_within_limit() {
                    assert_eq!(scan.has_alias, alias_reference(&text), "{text:?}");
                }
                checked += 1;
                let mut pos = 0;
                while pos < len {
                    idx[pos] += 1;
                    if idx[pos] < ALPHABET.len() {
                        break;
                    }
                    idx[pos] = 0;
                    pos += 1;
                }
                if pos == len {
                    break;
                }
            }
        }
        assert!(checked > 40_000, "{checked}");
    }

    #[test]
    fn shallow_and_realistic_yaml_passes() {
        assert!(flow_depth_within_limit(
            "a: [1, 2, [3, {b: 4}]]\nc:\n  - x\n  - y\n"
        ));
        // Plain scalars containing brackets must NOT be counted as flow.
        assert!(flow_depth_within_limit(&"k: value[0][1]\n".repeat(5000)));
        // Brackets inside quoted scalars don't count.
        assert!(flow_depth_within_limit(&"k: \"[[[[[[[[[[\"\n".repeat(5000)));
    }

    #[test]
    fn deep_flow_nesting_is_rejected() {
        let bomb = format!("x: {}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(!flow_depth_within_limit(&bomb));
        // With content between the opens (`[1,[1,[1,…`) it still nests.
        let bomb2 = format!("x: {}1{}", "[1,".repeat(2000), "]".repeat(2000));
        assert!(!flow_depth_within_limit(&bomb2));
        // Curly flow maps too.
        let bomb3 = format!("x: {}1{}", "{a: ".repeat(2000), "}".repeat(2000));
        assert!(!flow_depth_within_limit(&bomb3));
    }

    #[test]
    fn flow_depth_limit_matches_the_parsers_depth_limit() {
        // The ceiling sits exactly at the parser's depth limit: the deepest
        // flow document it can deserialize passes the guard and parses to the
        // same value as without the guard, and one level deeper is a document
        // the parser could never deserialize (so the guard adds no new false
        // reject). A regression to a looser ceiling (1024 before) fails the
        // second half.
        let at = MAX_YAML_FLOW_DEPTH;
        let ok = bomb(at);
        assert!(flow_depth_within_limit(&ok));
        let parsed = crate::yaml::from_str::<serde_json::Value>(&ok).unwrap();
        assert_eq!(crate::Format::Yaml.parse(&ok).unwrap(), parsed);
        let over = bomb(at + 1);
        assert!(crate::yaml::from_str::<serde_json::Value>(&over).is_err());
        assert!(crate::yaml::from_str::<ayaml::Value>(&over).is_err());
        assert!(!flow_depth_within_limit(&over));
        // Curly flow maps count the same way.
        let maps = |n: usize| format!("{}1{}", "{a: ".repeat(n), "}".repeat(n));
        assert!(crate::yaml::from_str::<serde_json::Value>(&maps(at)).is_ok());
        assert!(flow_depth_within_limit(&maps(at)));
        assert!(crate::yaml::from_str::<serde_json::Value>(&maps(at + 1)).is_err());
        assert!(!flow_depth_within_limit(&maps(at + 1)));
    }

    #[test]
    fn contains_alias_detects_aliases_not_globs() {
        assert!(contains_alias("x: *anchor\n"));
        assert!(contains_alias("  - <<: *base\n"));
        assert!(contains_alias("k:\n  - *a\n  - *a\n"));
        // A quoted glob is not an alias.
        assert!(!contains_alias("paths:\n  - \"**/*.rs\"\n"));
        assert!(!contains_alias("p: '*.rs'\n"));
        // Multiplication (space after `*`) and comments are not aliases.
        assert!(!contains_alias("expr: 2 * 3\n"));
        assert!(!contains_alias("# a comment mentioning *stars\n"));
        assert!(!contains_alias("plain: value\nother: 42\n"));
    }

    #[test]
    fn alias_hidden_behind_plain_scalar_quote_is_still_detected() {
        // A plain scalar containing a quote is valid, extremely common YAML. The
        // alias detector must not treat that quote as a string delimiter and skip
        // past a real `*alias` after it -- doing so would bypass the budget pass and
        // re-open the alias-bomb DoS. Both quote flavors, both "unclosed to EOF" and
        // "a later quote pairs across the alias" shapes.
        let cases = [
            "desc: it's fine\nanchor: &a [1, 2, 3]\nuse: *a\n",
            "size: 12\" wide\nanchor: &a [1, 2, 3]\nuse: *a\n",
            "a: it's\nb: &x [1]\nc: *x\nd: 'closed'\n",
            "a: 12\" x\nb: &x [1]\nc: *x\ntail: \"y\"\n",
        ];
        for case in cases {
            // Sanity: each case is genuinely parseable YAML (so the bomb is real).
            let parsed: Result<ayaml::Value, _> = crate::yaml::from_str(case);
            assert!(parsed.is_ok(), "case must be valid YAML: {case:?}");
            assert!(
                contains_alias(case),
                "alias must not be hidden behind a plain-scalar quote: {case:?}"
            );
        }
        // A GENUINE quoted scalar containing `*` is correctly skipped (no alias).
        assert!(!contains_alias("pattern: \"*.rs\"\nother: '*.md'\n"));
        assert!(!contains_alias("- \"a*b\"\n- '*'\n"));
        // A genuine quoted scalar AFTER an anchor/tag is still recognized (skipped).
        assert!(!contains_alias("k: &a \"*.rs\"\n"));
        assert!(!contains_alias("k: !!str '*.md'\n"));
        // But a real alias that references an anchored node is still detected.
        assert!(contains_alias("base: &b [1]\nuse: *b\n"));
        // End-to-end: a real bomb hidden behind an innocuous apostrophe line must be
        // rejected (previously slipped through the short-circuit).
        let anchor: String = (0..200).map(|_| "0,".to_string()).collect();
        let refs: String = (0..500).map(|_| "  - *a\n".to_string()).collect();
        let hidden = format!("desc: it's fine\nanchor: &a [{anchor}]\nrefs:\n{refs}");
        assert!(
            !expansion_within_limit_with(&hidden, 10_000 * YAML_NODE_COST),
            "alias bomb hidden behind a plain-scalar quote must still be rejected"
        );
    }

    #[test]
    fn flow_bomb_hidden_behind_plain_scalar_quote_is_still_rejected() {
        // A mid-plain-scalar quote must not let a deep flow collection on a later
        // line escape the depth count. `lead: it's` opens a bogus quote region in a
        // naive scanner that would swallow the `deep: [[[[...` bomb below it.
        let bomb = format!(
            "lead: it's\ndeep: {}1{}\n",
            "[".repeat(4000),
            "]".repeat(4000)
        );
        assert!(
            !flow_depth_within_limit(&bomb),
            "flow bomb after a mid-scalar quote must still be rejected"
        );
        let bomb2 = format!(
            "lead: 12\" x\ndeep: {}1{}\n",
            "[".repeat(4000),
            "]".repeat(4000)
        );
        assert!(!flow_depth_within_limit(&bomb2));
        // A genuine quoted scalar full of brackets must still NOT false-reject.
        assert!(flow_depth_within_limit(
            &"re: \"[[[[[[[[[[[[[[[[[[[[\"\n".repeat(3000)
        ));
        assert!(flow_depth_within_limit(&"re: '[[[[[[[[[[' \n".repeat(3000)));
        // Anchored / tagged quoted scalars full of brackets must NOT false-reject
        // (the quote still opens at a node position after `&a` / `!!str`).
        assert!(flow_depth_within_limit(
            &"k: &a \"[[[[[[[[[[\"\n".repeat(2000)
        ));
        assert!(flow_depth_within_limit(
            &"k: !!str \"[[[[[[[[[[\"\n".repeat(2000)
        ));
        // But an anchored FLOW bomb is still caught.
        let anchored = format!("k: &a {}1{}\n", "[".repeat(4000), "]".repeat(4000));
        assert!(!flow_depth_within_limit(&anchored));
    }

    /// `true` when the REAL parser sees nesting past its 128-level depth limit
    /// -- i.e. the fixture's bracket run is genuine structure, not text hidden
    /// inside a scalar or comment.
    fn parser_sees_deep_nesting(doc: &str) -> bool {
        crate::yaml::from_str::<ayaml::Value>(doc)
            .err()
            .is_some_and(|e| e.to_string().contains("recursion limit"))
    }

    #[test]
    fn self_referencing_alias_is_not_mistaken_for_a_flow_bomb() {
        // Proptest counterexample (windows CI, with the parser v0.22 used): `&a`
        // anchored a mapping holding `*a`, which that parser reported as
        // "recursion limit" at any bracket depth, though the brackets sat in a
        // double-quoted scalar the scanner rightly skips. ayaml names the cycle
        // instead, so the depth oracle cannot blame the brackets for it.
        let cycle = crate::yaml::from_str::<ayaml::Value>("&a [*a]\n")
            .unwrap_err()
            .to_string();
        assert!(
            cycle.contains("an alias cannot refer to a collection that contains it"),
            "{cycle}"
        );
        assert!(!parser_sees_deep_nesting("&a [*a]\n"));
        // The reported shape: ayaml stops at the quoted scalar's continuation
        // line, whatever the bracket depth after it.
        let doc = |b: &str| format!("&a \n*a : \"\n{b}\nc: \"\"\n");
        let error = |b: &str| {
            crate::yaml::from_str::<ayaml::Value>(&doc(b))
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            error(&bomb(300)),
            "quoted scalar continuation must be indented more than its parent at line 3 column 1"
        );
        assert_eq!(error(&bomb(300)), error(&bomb(1)));
        assert!(flow_depth_within_limit_with(&doc(&bomb(300)), 200));
    }

    /// `true` when the REAL parser sees a genuine alias token (an undefined
    /// `*undefined_zz` reference is reported as an unknown anchor).
    fn parser_sees_alias(doc: &str) -> bool {
        crate::yaml::from_str::<ayaml::Value>(doc)
            .err()
            .is_some_and(|e| {
                let e = e.into_parser();
                let message = e.to_string();
                message.contains("unknown anchor")
                    || (matches!(e.kind(), ayaml::ErrorKind::Limit) && message.contains("alias"))
            })
    }

    fn bomb(n: usize) -> String {
        format!("{}1{}", "[".repeat(n), "]".repeat(n))
    }

    #[test]
    fn flow_bomb_hidden_by_a_lexer_desync_is_still_rejected() {
        // GATE for the scanner's lexer parity with the parser. Each fixture hides
        // a deep flow collection behind a construct a naive scanner mis-lexes
        // into a quoted scalar or comment running past the bomb. Each is probed
        // against the real parser so the bomb is proven to be structure it nests.
        let n = MAX_YAML_FLOW_DEPTH + 76;
        let b = bomb(n);
        let fixtures = [
            // `#` comment inside a flow collection (a `"` in it is comment text).
            ("flow-comment", format!("b: [ #\"\n{b}\n ]\n")),
            // `#` right at a flow token start is a comment too.
            ("flow-comment-no-space", format!("b: [#\"\n{b}\n ]\n")),
            // A block scalar line starting with `"` is content, not a string.
            ("block-scalar", format!("s: |\n  \"\nb: {b}\nc: \"\"\n")),
            ("folded-chomp", format!("s: >-\n  \"\nb: {b}\nc: \"\"\n")),
            ("explicit-indent", format!("s: |2\n  \"\nb: {b}\nc: \"\"\n")),
            (
                "block-in-seq-map",
                format!("- k: |\n    \"\n  b: {b}\n  c: \"\"\n"),
            ),
            // A plain scalar CONTINUATION line starting with `"` is content.
            (
                "plain-continuation",
                format!("k: some text\n  \"more\nb: {b}\nz: \"x\"\n"),
            ),
            // A `"` mid-plain-scalar inside a flow collection is content.
            ("flow-mid-plain", format!("a: [x\"y, {b}, z\"w]\n")),
            // `-"` / `?"` / `:"` with no space are plain scalars, not indicators.
            ("dash-quote", format!("a: -\"\nb: {b}\nc: \"\"\n")),
            ("colon-quote", format!("a: x:\"\nb: {b}\nc: \"\"\n")),
            // In a flow collection too, a `:` before a quote is content
            // (YAML 1.2), so the quote opens no scalar.
            ("flow-colon-quote", format!("a: [x:\"y,\n{b}\n]\nc: \"\"\n")),
            // A flow `?` is the key indicator whatever follows, so a `#` right
            // after it starts a comment that hides the `]`.
            ("flow-key-comment", format!("[?#]\\\n{b}\nc: \"\"\n")),
            // ...and ayaml ends that comment at a NEL, where every other
            // comment runs on to the line feed.
            ("flow-key-comment-nel", format!("[?#\u{85} {b}]\n")),
            // NEL is content (YAML 1.2), not a line break before a quote.
            (
                "nel-is-content",
                format!("a: [x,\u{85}\",\n{b}\n]\nc: \"\"\n"),
            ),
            // An anchor name holds any character but a blank and the flow
            // indicators, so a quote, `#`, `|` or `.` in it opens nothing.
            ("anchor-name-quote", format!("k: &a\" {b}\nc: \"\"\n")),
            ("anchor-name-single-quote", format!("k: &a' {b}\nc: ''\n")),
            ("anchor-name-hash", format!("k: &a# {b}\n")),
            ("anchor-name-block", format!("k: &a|\n  {b}\n")),
            ("anchor-name-dot", format!("k: &a. {b}\n")),
            // ayaml's scanner ends the name `a` at a `:` before a flow
            // indicator, where its line reader reads on: the value then
            // continues past a column-0 quote.
            (
                "anchor-colon-flow-indicator",
                format!("&a:[: x\n\"\nb: {b}\nc: \"\"\n"),
            ),
            ("anchor-colon-comma", format!("&a:,: x\n'\nb: {b}\nc: ''\n")),
            // ayaml gives the byte order mark that starts the text no width and
            // reads a second one as content, the key here, at column 0, so the
            // `|2` scalar's content indent is 2.
            (
                "second-byte-order-mark",
                format!("\u{feff}\u{feff}  : |2\n  \"\nb: {b}\nc: \"\"\n"),
            ),
            // A `#` WITHOUT preceding space is plain-scalar content (not a comment).
            ("hash-in-plain", format!("a: b#c\nb: {b}\n")),
            // Single-quoted `''` escape does not end the scalar early.
            ("single-quote-escape", format!("a: 'it''s'\nb: {b}\n")),
        ];
        for (name, doc) in &fixtures {
            assert!(
                parser_sees_deep_nesting(doc),
                "{name}: fixture must nest for real in the parser"
            );
            assert!(
                !flow_depth_within_limit(doc),
                "{name}: the scanner must see the hidden flow bomb"
            );
        }
    }

    #[test]
    fn scalar_and_comment_contents_never_false_reject() {
        // The other half of parity: brackets / quotes inside genuine scalars and
        // comments are not structure, so ordinary YAML must still pass.
        let deep = "[".repeat(MAX_YAML_FLOW_DEPTH + 50);
        let ok = [
            format!("s: |\n  {deep}\n  \"\nt: 1\n"),
            format!("s: >+\n  {deep}\n\nt: 1\n"),
            format!("- k: |\n    {deep}\n  j: 2\n"),
            format!("a: '{deep}'\nb: 'it''s {deep}'\n"),
            format!("a: \"\\\"{deep}\"\n"),
            format!("# {deep}\nk: v # {deep}\n"),
            format!("k: !!str \"{deep}\"\nj: &a \"{deep}\"\n"),
            format!("k: value{deep}\n"),
            format!("k: [a, \"{deep}\", '{deep}']\n"),
            // Names that end at a `:` before other text, or hold one, are read
            // one way only.
            format!("a: &x: '{deep}'\nb: *x:\nc: &y '{deep}'\nd: *y\n"),
            format!("\u{feff}a:\n\u{feff}  b: '{deep}'\n"),
        ];
        for doc in &ok {
            assert!(
                crate::yaml::from_str::<ayaml::Value>(doc).is_ok(),
                "fixture must be valid YAML: {doc:?}"
            );
            assert!(flow_depth_within_limit(doc), "false reject: {doc:?}");
        }
    }

    #[test]
    fn alias_hidden_by_a_lexer_desync_is_still_detected() {
        // The alias short-circuit must never miss a real `*alias`: a missed one
        // skips the expansion budget and re-opens the alias bomb (a block-scalar
        // line starting with `"` hid 50 000 `*a` refs -> 1.6 GB, 29 s).
        let fixtures = [
            "s: |\n  \"\nb: [*undefined_zz]\nc: \"\"\n",
            "b: [ #\"\n*undefined_zz ]\nc: \"\"\n",
            "k: some text\n  \"more\nb: *undefined_zz\nz: \"x\"\n",
            "a: [x\"y, *undefined_zz, z\"w]\n",
            "a: -\"\nb: *undefined_zz\nc: \"\"\n",
            "- k: >\n    \"\n  b: *undefined_zz\n  c: \"\"\n",
            "k: &x\" v\nb: *undefined_zz\nc: \"\"\n",
            "k: &x|\n  [*undefined_zz]\n",
        ];
        for doc in fixtures {
            assert!(
                parser_sees_alias(doc),
                "fixture must hold a real alias: {doc:?}"
            );
            assert!(contains_alias(doc), "alias must be detected: {doc:?}");
        }
        // End to end: the reported billion-laughs shape is rejected.
        let anchor = "1,".repeat(1000);
        let refs = "*a,".repeat(50);
        let doc = format!("a: &a [{anchor}]\ns: |\n  \"\nb: [{refs}]\nc: \"\"\n");
        assert!(!expansion_within_limit_with(&doc, 10_000 * YAML_NODE_COST));
    }

    #[test]
    fn leading_bom_does_not_desync_the_line_one_column() {
        // Regression (with the parser v0.22 used): the lexer stripped a leading
        // BOM without advancing the column, but libyaml-rs skipped it as a
        // column-0 character, so `\u{feff}---` was NOT a document marker to it.
        // The off-by-one hid everything after it inside a phantom single-quoted
        // scalar: a 60 000-deep flow bomb passed the guard (7.7 s to parse),
        // and an alias bomb skipped the expansion budget (3.5 GB). ayaml takes
        // the same column, and saphyr does not, so the lexer reads it both ways.
        // Reachable through config / `extends:` / `suggest` bodies, which are
        // guarded on the raw text (`Format::parse` strips BOMs first).
        let b = bomb(MAX_YAML_FLOW_DEPTH + 76);
        for boms in ["\u{feff}", "\u{feff}\u{feff}"] {
            let flow = format!("{boms}--- 'x: {b}\n'\n");
            assert!(
                crate::yaml::from_str::<ayaml::Value>(&flow).is_err(),
                "{boms:?}: malformed BOM document is rejected"
            );
            assert!(
                !flow_depth_within_limit(&flow),
                "{boms:?}: hidden flow bomb"
            );
            let alias = format!("{boms}--- 'x: *undefined_zz\n'\n");
            assert!(
                crate::yaml::from_str::<ayaml::Value>(&alias).is_err(),
                "{boms:?}: malformed BOM document is rejected"
            );
            assert!(contains_alias(&alias), "{boms:?}: hidden alias");
        }
        // The reported alias-bomb shape, end to end.
        let anchor = "1,".repeat(1000);
        let refs = "*a,".repeat(50);
        let doc = format!(
            "\u{feff}--- 'k: {{a: &a [{anchor}], b: [{refs}], c: \"{{{{env.HOME}}}}\"}}\n'\n"
        );
        // ayaml checks the whole stream's syntax before it replays an alias,
        // so the quote that `---` leaves open on line 2 stops it first.
        assert_eq!(
            crate::yaml::from_str::<ayaml::Value>(&doc)
                .unwrap_err()
                .to_string(),
            "unterminated single-quoted scalar at line 2 column 1"
        );
        assert!(contains_alias(&doc));
        // A BOM before an ordinary document still scans like the document.
        assert!(flow_depth_within_limit("\u{feff}---\na: [1, [2]]\n"));
        assert!(!contains_alias("\u{feff}---\na: '*x'\n"));
    }

    /// The body of the differential parity property (see
    /// [`scanner_never_misses_what_the_parser_sees`]), shared by its BOM variant.
    fn check_parity(prefix: &str, sep: &str) -> Result<(), proptest::test_runner::TestCaseError> {
        let deep = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(300));
        let shallow = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(1));
        if parser_sees_deep_nesting(&deep) && !parser_sees_deep_nesting(&shallow) {
            proptest::prop_assert!(
                !flow_depth_within_limit_with(&deep, 200),
                "scanner missed a real flow bomb: {deep:?}"
            );
        }
        let alias = format!("{prefix}{sep}*undefined_zz\nc: \"\"\n");
        if parser_sees_alias(&alias) {
            proptest::prop_assert!(contains_alias(&alias), "missed alias: {alias:?}");
        }
        Ok(())
    }

    /// After a name that ayaml's scanner and line reader end in different
    /// places, the scanner counts every later bracket as structure and every
    /// `*` as an alias, quoted or not, and stops scanning.
    #[test]
    fn a_name_ayaml_reads_two_ways_makes_the_scanner_assume_the_worst() {
        // The `[` of the key `:[` and the three quoted ones.
        let quoted = format!("&a:[: x\nb: '{}'\n", "[".repeat(3));
        assert!(!flow_depth_within_limit_with(&quoted, 3));
        assert!(flow_depth_within_limit_with(&quoted, 4));
        assert!(contains_alias("&a:{: x\nb: '*'\n"));
        // A `:` before other text is no such name.
        assert!(flow_depth_within_limit_with(
            &format!("&a:b: x\nb: '{}'\n", "[".repeat(3)),
            0
        ));
        assert!(!contains_alias("&a:b: x\nb: '*'\n"));
    }

    /// A flow `:` before `?` is content to ayaml, so the scanner reads a plain
    /// scalar there and moves on. It once started a plain scalar at that `:`
    /// that ended before it, again and again: these inputs hung the parity
    /// properties, two of them as an alias whose name gave back its `:`.
    #[test]
    fn the_scanner_moves_past_a_flow_colon_before_a_question_mark() {
        for (prefix, sep) in [
            ("[:?", "\n"),
            ("{a:?b", ", "),
            ("[*a:?", " "),
            ("{\\*a:?x!t \"\n", "\nb: "),
            (" {- \t\n\u{2028}|2\\*a:? &a .", "\n"),
        ] {
            check_parity(prefix, sep).unwrap();
        }
        for doc in ["[:?]\n", "[a:?b]\n", "{:?: c}\n", "{*a:?x}\n"] {
            assert!(flow_depth_within_limit_with(doc, 1), "{doc:?}");
            assert!(
                !flow_depth_within_limit_with(&format!("{doc}{}\n", bomb(3)), 2),
                "{doc:?}"
            );
        }
        assert!(!flow_depth_within_limit_with(
            &format!("[a:?b, {}]\n", bomb(2)),
            2
        ));
    }

    const PARITY_ALPHABET: &[&str] = &[
        "\"",
        "'",
        "''",
        "#",
        " #",
        " ",
        "\n",
        "  ",
        "|",
        ">",
        "|2",
        ">-",
        "-",
        "- ",
        "?",
        "? ",
        ":",
        ": ",
        "k: ",
        "x",
        "[",
        "]",
        "{",
        "}",
        ",",
        "&a ",
        "!t ",
        "*a ",
        "&a",
        "*a",
        "&a:",
        "*a:",
        ".",
        "\\",
        "\t",
        "\u{85}",
        "\u{2028}",
        "\u{2029}",
        "\r\n",
        "---\n",
        "a#b",
        "k: |\n  ",
        "- k: |\n    ",
    ];
    const PARITY_SEPS: &[&str] = &["\n", " ", "\n  ", ", ", "\nb: "];

    proptest::proptest! {
        /// Differential parity against the real parser: whatever adversarial
        /// prefix precedes a deep flow bomb, if the parser nests it the scanner
        /// must see it; and whatever precedes an alias, if the parser resolves it
        /// as an alias token the short-circuit must report one.
        #[test]
        fn scanner_never_misses_what_the_parser_sees(
            prefix in proptest::collection::vec(
                proptest::sample::select(PARITY_ALPHABET), 0..14,
            ),
            sep in proptest::sample::select(PARITY_SEPS),
        ) {
            check_parity(&prefix.concat(), sep)?;
        }

        /// The same parity behind one or two leading BOMs, with the document
        /// markers and quotes that a line-1 column desync turns into hiding
        /// places (a BOM can also follow a line break, so the alphabet has it).
        #[test]
        fn scanner_never_misses_what_the_parser_sees_with_bom(
            boms in 1usize..3,
            prefix in proptest::collection::vec(
                proptest::sample::select(
                    [PARITY_ALPHABET, &["---", "--- ", "--- '", "...", "\u{feff}", "%"]].concat()
                ),
                0..14,
            ),
            sep in proptest::sample::select(PARITY_SEPS),
        ) {
            check_parity(&format!("{}{}", "\u{feff}".repeat(boms), prefix.concat()), sep)?;
        }
    }

    #[test]
    fn alias_expansion_is_bounded() {
        // Uses a SMALL budget so the counting/bail logic is exercised fast (the
        // production ceiling is validated by the compile-time upper-bound assert).
        // Single-level bomb: one 200-element anchor referenced 500 times -> 100k
        // nodes, well over a 10k budget -> rejected.
        let anchor: String = (0..200).map(|_| "0,".to_string()).collect();
        let refs: String = (0..500).map(|_| "  - *a\n".to_string()).collect();
        let bomb = format!("anchor: &a [{anchor}]\nrefs:\n{refs}");
        assert!(
            !expansion_within_limit_with(&bomb, 10_000 * YAML_NODE_COST),
            "single-level alias bomb must be rejected"
        );
        // Merge-key (`<<: *a`) is the same materialization path -> also rejected.
        let merge: String = (0..500).map(|_| "  - <<: *a\n".to_string()).collect();
        let merge_bomb = format!(
            "anchor: &a {{a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0, h: 0, i: 0, j: 0}}\nrefs:\n{merge}"
        );
        assert!(
            !expansion_within_limit_with(&merge_bomb, 1_000 * YAML_NODE_COST),
            "merge-key alias bomb must be rejected"
        );
        // Legit DRY alias use stays well under budget.
        let mut dry = String::from("d: &d {a: 1, b: 2, c: 3}\nitems:\n");
        for _ in 0..50 {
            dry.push_str("  - <<: *d\n    n: 1\n");
        }
        assert!(
            expansion_within_limit_with(&dry, 10_000 * YAML_NODE_COST),
            "legit DRY alias bundle must pass"
        );
        // Alias-free text short-circuits (never even parses here) -> always within.
        assert!(expansion_within_limit_with(&"k: v\n".repeat(100_000), 10));
        // A quoted glob is not an alias -> short-circuits, passes.
        assert!(expansion_within_limit_with("paths:\n  - \"**/*.rs\"\n", 5));
    }

    #[test]
    fn alias_expansion_budget_is_charged_in_bytes() {
        // Regression: the budget counted NODES, so one big anchored scalar
        // replayed N times cost N, not N x its size. A 1 MB scalar x 4000 refs
        // (4000 nodes) passed and the real parse copied 4 GB (3.8 GB peak RSS) on
        // both the Value config path and the structured-query path. Production
        // budget, the reported shape (scaled to 3000 refs, still 3 GB of copies).
        let big = format!(
            "a: &a \"{}\"\nb: [{}]\n",
            "x".repeat(1_000_000),
            "*a,".repeat(3000)
        );
        assert!(!expansion_within_limit(&big));
        assert!(!scan_guards(&big).expansion_within_limit(&big));
        // The same scalar replayed a handful of times is fine.
        let few = format!("a: &a \"{}\"\nb: [*a, *a, *a]\n", "x".repeat(1_000_000));
        assert!(expansion_within_limit(&few));
        assert!(scan_guards(&few).expansion_within_limit(&few));
        // Mechanism, small budget: keys and tag names are charged too.
        let s = "y".repeat(1000);
        let key = format!("a: &a {{\"{s}\": 1}}\nb: [{}]\n", "*a,".repeat(200));
        assert!(!expansion_within_limit_with(&key, 300_000));
        let tag = format!("a: &a !{s} 1\nb: [{}]\n", "*a,".repeat(200));
        assert!(!expansion_within_limit_with(&tag, 300_000));
    }

    #[test]
    fn map_heavy_alias_bomb_is_rejected_at_the_production_budget() {
        // Regression: 8M nodes was too loose for map-heavy content (~235 B/node):
        // an 18.5 KB `[{k: v} x 500]` anchor x 5000 refs passed and parsed for
        // 4.9 s / 1.76 GB. The byte budget cuts it off at ~1M nodes.
        let doc = format!(
            "a: &a [{}]\nb: [{}]\n",
            "{k: v},".repeat(500),
            "*a,".repeat(5000)
        );
        assert!(!expansion_within_limit(&doc));
        let scan = scan_guards(&doc);
        assert!(scan.flow_within_limit());
        assert!(!scan.expansion_within_limit(&doc));
    }

    #[test]
    fn realistic_anchor_heavy_yaml_passes_with_a_wide_margin() {
        // The other half: real alias use must keep a big margin under the budget.
        // Shaped after the heaviest real file measured (GitLab's
        // `rules.gitlab-ci.yml`: ~1100 aliases into rule lists, ~4 MB by the
        // budget's estimate) plus docker-compose `x-` anchors with `<<:` merges:
        // 65K expanded nodes + 1.3 MB of scalar copies (~18 MB estimated, 12 MB
        // real peak RSS), ~4.5x that file -- and it must pass a budget 10x
        // TIGHTER than production.
        let mut doc = String::from("x-common: &common\n  restart: unless-stopped\n");
        doc.push_str("  environment: &env\n");
        doc.extend(
            (0..40).map(|i| format!("    VAR_{i}: \"value-{i}-with-a-realistic-length\"\n")),
        );
        doc.push_str("  logging: {driver: json-file, options: {max-size: 10m}}\n");
        doc.push_str(".patterns:\n  code: &code\n");
        doc.extend((0..5).map(|p| format!("    - \"{{app,lib,ee/app,ee/lib}}/**/*-{p}.rb\"\n")));
        doc.push_str(".rules:\n");
        for r in 0..300 {
            doc.push_str("  rules-");
            doc.push_str(&r.to_string());
            doc.push_str(": &rules-");
            doc.push_str(&r.to_string());
            doc.push('\n');
            doc.extend((0..3).map(|k| {
                format!(
                    "    - if: '$CI_MERGE_REQUEST_LABELS =~ /pipeline:run-{r}-{k}/ && \
                     $CI_PIPELINE_SOURCE == \"merge_request_event\"'\n      \
                     changes: *code\n      when: on_success\n"
                )
            }));
        }
        doc.push_str("services:\n");
        doc.extend((0..60).map(|s| {
            format!(
                "  svc-{s}:\n    <<: *common\n    image: \"registry.example.com/svc-{s}:1.2.3\"\n    \
                 environment:\n      <<: *env\n      SVC: \"{s}\"\n"
            )
        }));
        doc.push_str("jobs:\n");
        doc.extend((0..1000).map(|j| {
            format!(
                "  job-{j}:\n    script: [\"make test-{j}\"]\n    rules: *rules-{}\n",
                j % 300
            )
        }));
        assert!(contains_alias(&doc));
        assert!(crate::yaml::from_str::<ayaml::Value>(&doc).is_ok());
        assert!(expansion_within_limit_with(
            &doc,
            MAX_YAML_EXPANSION_BYTES / 10
        ));
        assert!(expansion_within_limit(&doc));
    }
}
