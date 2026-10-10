//! Pre-parse bounds on YAML flow-collection nesting depth and alias use.
//!
//! `serde_yaml_ng`/libyaml is super-linear on deeply-nested FLOW collections
//! (`[[[…]]]` / `{{{…}}}`): a ~40 KB `[`×20000 document already takes seconds,
//! and a slightly deeper one hangs the whole run — an algorithmic-complexity `DoS`
//! reachable both from a `yaml_path_*` rule over crafted repo content and from a
//! deeply-nested config / `extends:`'d ruleset (which the config loader parses
//! with the same library). libyaml has no nesting limit and the slowness is in
//! its tokenizer, so only a cheap pre-parse scan of the raw text can bound it.
//!
//! **The scan is a port of libyaml's own token-boundary logic** (`Lexer`), not a
//! heuristic. Both guards are only as sound as the scanner's agreement with
//! libyaml on where a scalar or comment ENDS: any region the scanner wrongly
//! treats as opaque hides the structure inside it (a `#` comment in a flow
//! collection whose `"` a heuristic read as a string to EOF; a block-scalar line
//! starting with `"`; a plain-scalar continuation line) -- and a hidden `[[[…` or
//! `*alias` run is exactly the `DoS` the guard exists to stop. So the lexer mirrors
//! libyaml's scanner (unsafe-libyaml 0.2.11, `scanner.rs`): token dispatch,
//! comments at any token start, plain-scalar end rules (incl. multi-line
//! continuation bounded by the block indent), quoted-scalar escapes,
//! indentation-delimited block scalars (auto-detected or explicit indent), the
//! indent stack it rolls on `-` / `?` / simple keys, and NEL / LS / PS as line
//! breaks. Where libyaml would raise a scanner error the lexer simply keeps
//! going: libyaml stops there, so nothing after the error point can reach the
//! super-linear parse, and continuing (rather than stopping) never hides input.
//! A differential property test pins the parity against the real parser.

/// Real config/manifest YAML nests a handful of flow levels; anything past this
/// is a bomb. Pinned to `serde_yaml_ng`'s own recursion limit: its deserializer
/// allows 128 nested collections (block or flow) and fails the 129th, so a
/// document whose flow nesting alone exceeds 128 can never deserialize into a
/// value anyway -- rejecting it here introduces no new false reject, only skips
/// the libyaml tokenizing (whose per-token simple-key bookkeeping grows with the
/// flow depth) that would end in that same error. (The one exception is a
/// subtree serde IGNORES -- `IgnoredAny` skips events without a depth check --
/// which no alint path relies on: configs are `deny_unknown_fields`, and every
/// structured query builds the whole tree.)
pub const MAX_YAML_FLOW_DEPTH: usize = 128;

/// `true` when the YAML text's flow-collection nesting stays within
/// [`MAX_YAML_FLOW_DEPTH`]. A cheap single-pass scan with libyaml's own lexical
/// rules (see the module docs), so brackets inside scalars and comments don't
/// count and no construct can hide real nesting from it.
#[must_use]
pub fn flow_depth_within_limit(text: &str) -> bool {
    flow_depth_within_limit_with(text, MAX_YAML_FLOW_DEPTH)
}

/// [`flow_depth_within_limit`] with an explicit ceiling (tests probe parity with
/// a ceiling below the real parser's recursion limit).
fn flow_depth_within_limit_with(text: &str, max: usize) -> bool {
    let mut within = true;
    Lexer::new(text).run(|tok| match tok {
        Tok::FlowOpen(depth) if depth > max => {
            within = false;
            false
        }
        _ => true,
    });
    within
}

/// Maximum number of nodes a YAML document may EXPAND to once aliases are replayed.
/// `serde_yaml_ng` materializes each `*alias` by copying its anchor's whole subtree,
/// and its OWN limits do not bound this: the recursion limit (128) only catches
/// *nested* aliases (classic billion-laughs), and the alias-DEREF counter
/// (`jumpcount > events.len()*100`) is never reached by a SINGLE anchor referenced
/// many times -- N flat `*a` refs make only N derefs but N x (anchor size) node
/// materializations. So a ~150 KB crafted file (one 1000-element anchor, 20-50k
/// refs, or a `<<: *a` merge variant) expands to 20-50M nodes: hundreds of MB and
/// seconds of CPU -- a crafted-small-file `DoS` reachable from any untrusted YAML (a
/// `yaml_path_*` / `json_schema_passes` / `extract` target, or a config / `extends:`
/// body). The heaviest LEGIT alias use measured is ~300K nodes (a big merge-key
/// bundle), so 8M keeps a >25x margin while catching the multi-million-node bombs;
/// it also caps a single alias-bearing file's tree at a few hundred MB.
pub const MAX_YAML_EXPANSION_NODES: usize = 8_000_000;

/// Upper bound on [`MAX_YAML_EXPANSION_NODES`], enforced at compile time: set it
/// absurdly high (or `usize::MAX`) and the guard would never fire, silently
/// re-opening the alias-bomb `DoS` while the small-budget mechanism test still passes.
const _: () = assert!(
    MAX_YAML_EXPANSION_NODES <= 64_000_000,
    "MAX_YAML_EXPANSION_NODES is too high to meaningfully bound alias expansion"
);

/// `true` when the YAML text's ALIAS expansion stays within
/// [`MAX_YAML_EXPANSION_NODES`]. Only a document that actually uses an alias
/// (`*name`) can amplify, so alias-free text short-circuits to `true` at zero cost
/// (its node count is ~linear in bytes, already bounded by the read cap). For
/// alias-bearing text a cheap DISCARD-ONLY pass drives `serde_yaml_ng`'s
/// deserializer, which replays anchored events through the visitor -- so the
/// expansion is counted and the pass bails once the budget is exceeded (measured: a
/// 30M-node flat bomb aborts in ~0.3s, a tagged variant in ~1.3s, both bounded). It
/// builds NO value, so
/// it cannot change the real parse's output; the caller runs the real parse only
/// after this returns `true`. A non-budget deserialize error (malformed YAML, or an
/// unusual node the counter doesn't model) is ignored -- only a genuine budget
/// overflow returns `false`, and everything else falls through to the real parse,
/// which produces the proper error. This is fail-safe: a real bomb is ordinary
/// scalars / sequences / maps (and tagged nodes, via `visit_enum`) and is counted.
#[must_use]
pub fn expansion_within_limit(text: &str) -> bool {
    expansion_within_limit_with(text, MAX_YAML_EXPANSION_NODES)
}

/// [`expansion_within_limit`] with an explicit node budget, so tests can exercise
/// the counting + bail + alias-gate logic with a small budget (fast) instead of
/// materializing millions of nodes at the production ceiling.
fn expansion_within_limit_with(text: &str, max: usize) -> bool {
    use serde::de::DeserializeSeed as _;
    if !contains_alias(text) {
        return true;
    }
    let remaining = std::cell::Cell::new(max);
    let exceeded = std::cell::Cell::new(false);
    let seed = NodeBudget {
        remaining: &remaining,
        exceeded: &exceeded,
    };
    let _ = seed.deserialize(serde_yaml_ng::Deserializer::from_str(text));
    !exceeded.get()
}

/// `true` when the text holds a genuine YAML alias token (`*name`), as libyaml's
/// scanner would see it -- so a `"**/*.rs"` glob or a `*` inside a comment, block
/// scalar or plain scalar doesn't count.
///
/// Safety property: it must NEVER return `false` when a real alias exists (that
/// would skip the expansion budget pass and re-open the alias-bomb `DoS`). It
/// reports every `*` at a token start, even a malformed one libyaml would reject:
/// a false positive merely triggers the (still correct, still bounded) counting
/// pass.
fn contains_alias(text: &str) -> bool {
    let mut found = false;
    Lexer::new(text).run(|tok| {
        if matches!(tok, Tok::Alias) {
            found = true;
        }
        !found
    });
    found
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
/// flow level 0, which decides the column libyaml rolls the block indent to when
/// its `:` arrives. Flow-level keys never move the indent, so they're not kept.
#[derive(Debug, Clone, Copy)]
struct KeyMark {
    line: usize,
    index: usize,
    col: isize,
}

/// A port of the token-boundary logic of libyaml's scanner (unsafe-libyaml
/// 0.2.11, `yaml_parser_fetch_next_token` and the `scan_*` routines it calls). It
/// tracks exactly the state that decides where tokens begin and end -- flow
/// level, the block indent stack, `simple_key_allowed`, and the level-0 simple
/// key -- and skips scalar / comment CONTENTS the way libyaml does, reporting
/// flow opens and aliases. Scanner errors are not modelled: libyaml stops at an
/// error, so the lexer just keeps going (see the module docs). Columns count
/// characters, as libyaml's marks do.
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
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        // No BOM special-casing here: `serde_yaml_ng` sets the parser's encoding
        // to UTF-8 explicitly, so libyaml's reader never runs its BOM detection
        // and hands a leading BOM to the scanner. The scanner skips it in
        // `scan_to_next_token` (at column 0, like any line start) with an
        // ordinary `SKIP`, which ADVANCES the column: a `---` right after a BOM
        // sits at column 1 and is not a document marker. Stripping the BOM up
        // front left the lexer one column behind libyaml on line 1, which hid
        // flow bombs and aliases behind a `\u{feff}---`.
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
        }
    }

    /// Byte at `i + k`, or `0` past the end (libyaml's NUL-terminated buffer).
    fn at(&self, k: usize) -> u8 {
        self.b.get(self.i + k).copied().unwrap_or(0)
    }

    fn eof(&self) -> bool {
        self.i >= self.b.len()
    }

    /// libyaml `IS_BREAK_AT`: CR, LF, NEL (U+0085), LS (U+2028), PS (U+2029).
    fn is_break_at(&self, k: usize) -> bool {
        match self.at(k) {
            b'\r' | b'\n' => true,
            0xC2 => self.at(k + 1) == 0x85,
            0xE2 => self.at(k + 1) == 0x80 && matches!(self.at(k + 2), 0xA8 | 0xA9),
            _ => false,
        }
    }

    fn is_blank_at(&self, k: usize) -> bool {
        matches!(self.at(k), b' ' | b'\t')
    }

    fn is_breakz_at(&self, k: usize) -> bool {
        self.i + k >= self.b.len() || self.is_break_at(k)
    }

    fn is_blankz_at(&self, k: usize) -> bool {
        self.is_blank_at(k) || self.is_breakz_at(k)
    }

    /// libyaml `IS_ALPHA`: the anchor / tag-handle character class.
    fn is_alpha_at(&self, k: usize) -> bool {
        let c = self.at(k);
        c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
    }

    /// `---` / `...` at column 0 followed by a blank or end of line.
    fn at_document_marker(&self) -> bool {
        self.col == 0
            && ((self.at(0) == b'-' && self.at(1) == b'-' && self.at(2) == b'-')
                || (self.at(0) == b'.' && self.at(1) == b'.' && self.at(2) == b'.'))
            && self.is_blankz_at(3)
    }

    /// libyaml `SKIP`: advance one character (its UTF-8 width), one column.
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

    /// libyaml `SKIP_LINE`: consume one line break (CRLF as one).
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
            } else if c == b',' {
                self.remove_simple_key();
                self.simple_key_allowed = true;
                self.advance();
            } else if c == b'-' && self.is_blankz_at(1) {
                self.roll_indent(self.col);
                self.remove_simple_key();
                self.simple_key_allowed = true;
                self.advance();
            } else if c == b'?' && (self.flow != 0 || self.is_blankz_at(1)) {
                self.roll_indent(self.col);
                self.remove_simple_key();
                self.simple_key_allowed = self.flow == 0;
                self.advance();
            } else if c == b':' && (self.flow != 0 || self.is_blankz_at(1)) {
                self.fetch_value();
            } else if c == b'*' || c == b'&' {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.advance();
                while self.is_alpha_at(0) {
                    self.advance();
                }
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
            } else if self.can_start_plain(c) {
                self.save_simple_key();
                self.simple_key_allowed = false;
                self.scan_plain_scalar();
            } else {
                // "found character that cannot start any token": libyaml stops
                // here. Step over it and keep scanning (never hides input).
                self.advance();
            }
        }
    }

    /// `yaml_parser_scan_to_next_token`: skip blanks, a comment (a `#` at any
    /// token start), and line breaks; a line break in block context re-allows a
    /// simple key. (Tabs are skipped unconditionally: where libyaml would refuse
    /// one it errors and stops.)
    fn scan_to_next_token(&mut self) {
        loop {
            // libyaml skips ONE BOM at column 0 as an ordinary character (the
            // column advances to 1), so a second BOM starts a plain scalar.
            if self.col == 0 && self.b[self.i.min(self.b.len())..].starts_with(b"\xEF\xBB\xBF") {
                self.advance();
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
            || (self.flow == 0 && (c == b'?' || c == b':') && !self.is_blankz_at(1))
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

    /// `yaml_parser_scan_plain_scalar`: ends before `: ` / ` #`, a flow indicator
    /// (in flow context), a document marker, or -- in block context -- a
    /// continuation line indented less than the block indent + 1. Consumes the
    /// trailing blanks / breaks, and re-allows a simple key if it ended on a
    /// line break.
    fn scan_plain_scalar(&mut self) {
        let min_col = self.indent + 1;
        let mut leading_blanks = false;
        loop {
            if self.at_document_marker() || self.at(0) == b'#' {
                break;
            }
            while !self.is_blankz_at(0) {
                let c = self.at(0);
                if self.flow != 0
                    && c == b':'
                    && matches!(self.at(1), b',' | b'?' | b'[' | b']' | b'{' | b'}')
                {
                    // libyaml: "found unexpected ':'" -- it stops here.
                    return;
                }
                if (c == b':' && self.is_blankz_at(1))
                    || (self.flow != 0 && matches!(c, b',' | b'[' | b']' | b'{' | b'}'))
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
            // libyaml: "did not find expected comment or line break" -- it stops.
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

/// A discard-only `serde` seed that counts every node it visits, decrementing a
/// shared budget and flagging + erroring the instant it underflows. Used by
/// [`expansion_within_limit`] to bound YAML alias expansion without materializing a
/// value. `Copy` (it holds only shared `Cell` refs), so it seeds child nodes freely.
#[derive(Clone, Copy)]
struct NodeBudget<'a> {
    remaining: &'a std::cell::Cell<usize>,
    exceeded: &'a std::cell::Cell<bool>,
}

impl<'de> serde::de::DeserializeSeed<'de> for NodeBudget<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        let Some(n) = self.remaining.get().checked_sub(1) else {
            self.exceeded.set(true);
            return Err(serde::de::Error::custom(
                "YAML alias expansion exceeds the maximum supported node count",
            ));
        };
        self.remaining.set(n);
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
    // Scalars are already counted (once) in `deserialize` above; just accept them.
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
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_bytes<E>(self, _: &[u8]) -> Result<(), E> {
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
    fn flow_depth_limit_matches_serde_yaml_ngs_recursion_limit() {
        // The ceiling sits exactly at serde_yaml_ng's recursion limit: the
        // deepest flow document it can deserialize passes the guard and parses
        // to the same value as without the guard, and one level deeper is a
        // document serde_yaml_ng could never deserialize (so the guard adds no
        // new false reject). A regression to a looser ceiling (1024 before) fails
        // the second half.
        let at = MAX_YAML_FLOW_DEPTH;
        let ok = bomb(at);
        assert!(flow_depth_within_limit(&ok));
        let parsed = serde_yaml_ng::from_str::<serde_json::Value>(&ok).unwrap();
        assert_eq!(crate::Format::Yaml.parse(&ok).unwrap(), parsed);
        let over = bomb(at + 1);
        assert!(serde_yaml_ng::from_str::<serde_json::Value>(&over).is_err());
        assert!(serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&over).is_err());
        assert!(!flow_depth_within_limit(&over));
        // Curly flow maps count the same way.
        let maps = |n: usize| format!("{}1{}", "{a: ".repeat(n), "}".repeat(n));
        assert!(serde_yaml_ng::from_str::<serde_json::Value>(&maps(at)).is_ok());
        assert!(flow_depth_within_limit(&maps(at)));
        assert!(serde_yaml_ng::from_str::<serde_json::Value>(&maps(at + 1)).is_err());
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
            let parsed: Result<serde_yaml_ng::Value, _> = serde_yaml_ng::from_str(case);
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
            !expansion_within_limit_with(&hidden, 10_000),
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

    /// `true` when the REAL parser (libyaml via `serde_yaml_ng`) sees nesting past
    /// its 128-level recursion limit -- i.e. the fixture's bracket run is genuine
    /// structure, not text hidden inside a scalar or comment.
    fn libyaml_sees_deep_nesting(doc: &str) -> bool {
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(doc)
            .err()
            .is_some_and(|e| e.to_string().contains("recursion limit"))
    }

    /// `true` when the bracket run itself is what pushes the real parser past its
    /// recursion limit: `deep` trips it and the same document with a one-level
    /// `shallow` bomb does not. A self-referencing alias (`&a` anchoring a node
    /// that contains `*a`) trips the limit with no nesting at all, so the bare
    /// [`libyaml_sees_deep_nesting`] verdict is not proof of a flow bomb.
    fn libyaml_bomb_causes_deep_nesting(deep: &str, shallow: &str) -> bool {
        libyaml_sees_deep_nesting(deep) && !libyaml_sees_deep_nesting(shallow)
    }

    #[test]
    fn self_referencing_alias_is_not_mistaken_for_a_flow_bomb() {
        // Proptest counterexample (windows CI): `&a` anchors a mapping holding
        // `*a`, so libyaml reports "recursion limit" at any bracket depth. The
        // brackets sit inside a double-quoted scalar, which the scanner rightly
        // skips; the oracle must not blame it for the alias recursion.
        let doc = |b: &str| format!("&a \n*a : \"\n{b}\nc: \"\"\n");
        let (deep, shallow) = (doc(&bomb(300)), doc(&bomb(1)));
        assert!(libyaml_sees_deep_nesting(&deep));
        assert!(libyaml_sees_deep_nesting(&shallow));
        assert!(!libyaml_bomb_causes_deep_nesting(&deep, &shallow));
        assert!(flow_depth_within_limit_with(&deep, 200));
    }

    /// `true` when the REAL parser sees a genuine alias token (an undefined
    /// `*undefined_zz` reference is reported as an unknown anchor).
    fn libyaml_sees_alias(doc: &str) -> bool {
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(doc)
            .err()
            .is_some_and(|e| e.to_string().contains("unknown anchor"))
    }

    fn bomb(n: usize) -> String {
        format!("{}1{}", "[".repeat(n), "]".repeat(n))
    }

    #[test]
    fn flow_bomb_hidden_by_a_lexer_desync_is_still_rejected() {
        // GATE for the scanner's lexer parity with libyaml. Each fixture hides a
        // deep flow collection behind a construct a naive scanner mis-lexes into
        // a quoted scalar running past the bomb. Each is probed against the real
        // parser so the bomb is proven to be structure libyaml actually nests.
        let n = MAX_YAML_FLOW_DEPTH + 76;
        let b = bomb(n);
        let fixtures = [
            // `#` comment inside a flow collection (a `"` in it is comment text).
            ("flow-comment", format!("b: [ #\"\n{b}\n ]\n")),
            // `#` right at a flow token start is a comment too in libyaml.
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
            // NEL / LS are line breaks to libyaml, ending a comment.
            ("nel-ends-comment", format!("# c \"\u{85}b: {b}\nc: \"\"\n")),
            (
                "ls-ends-comment",
                format!("# c \"\u{2028}b: {b}\nc: \"\"\n"),
            ),
            // A `#` WITHOUT preceding space is plain-scalar content (not a comment).
            ("hash-in-plain", format!("a: b#c\nb: {b}\n")),
            // Single-quoted `''` escape does not end the scalar early.
            ("single-quote-escape", format!("a: 'it''s'\nb: {b}\n")),
        ];
        for (name, doc) in &fixtures {
            assert!(
                libyaml_sees_deep_nesting(doc),
                "{name}: fixture must nest for real in libyaml"
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
        ];
        for doc in &ok {
            assert!(
                serde_yaml_ng::from_str::<serde_yaml_ng::Value>(doc).is_ok(),
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
            "# c \"\u{85}b: *undefined_zz\nc: \"\"\n",
            "- k: >\n    \"\n  b: *undefined_zz\n  c: \"\"\n",
        ];
        for doc in fixtures {
            assert!(
                libyaml_sees_alias(doc),
                "fixture must hold a real alias: {doc:?}"
            );
            assert!(contains_alias(doc), "alias must be detected: {doc:?}");
        }
        // End to end: the reported billion-laughs shape is rejected.
        let anchor = "1,".repeat(1000);
        let refs = "*a,".repeat(50);
        let doc = format!("a: &a [{anchor}]\ns: |\n  \"\nb: [{refs}]\nc: \"\"\n");
        assert!(!expansion_within_limit_with(&doc, 10_000));
    }

    #[test]
    fn leading_bom_does_not_desync_the_line_one_column() {
        // Regression: the lexer stripped a leading BOM without advancing the
        // column, but libyaml (UTF-8 encoding set explicitly by serde_yaml_ng)
        // skips it as a column-0 character, so `\u{feff}---` is NOT a document
        // marker to libyaml. The off-by-one hid everything after it inside a
        // phantom single-quoted scalar: a 60 000-deep flow bomb passed the guard
        // (7.7 s in libyaml), and an alias bomb skipped the expansion budget
        // (3.5 GB). Reachable through config / `extends:` / `suggest` bodies,
        // which are guarded on the raw text (`Format::parse` strips BOMs first).
        let b = bomb(MAX_YAML_FLOW_DEPTH + 76);
        for boms in ["\u{feff}", "\u{feff}\u{feff}"] {
            let flow = format!("{boms}--- 'x: {b}\n'\n");
            assert!(libyaml_sees_deep_nesting(&flow), "{boms:?}: real nesting");
            assert!(
                !flow_depth_within_limit(&flow),
                "{boms:?}: hidden flow bomb"
            );
            let alias = format!("{boms}--- 'x: *undefined_zz\n'\n");
            assert!(libyaml_sees_alias(&alias), "{boms:?}: real alias");
            assert!(contains_alias(&alias), "{boms:?}: hidden alias");
        }
        // The reported alias-bomb shape, end to end.
        let anchor = "1,".repeat(1000);
        let refs = "*a,".repeat(50);
        let doc = format!(
            "\u{feff}--- 'k: {{a: &a [{anchor}], b: [{refs}], c: \"{{{{env.HOME}}}}\"}}\n'\n"
        );
        assert!(!expansion_within_limit_with(&doc, 10_000));
        // A BOM before an ordinary document still scans like the document.
        assert!(flow_depth_within_limit("\u{feff}---\na: [1, [2]]\n"));
        assert!(!contains_alias("\u{feff}---\na: '*x'\n"));
    }

    /// The body of the differential parity property (see
    /// [`scanner_never_misses_what_libyaml_sees`]), shared by its BOM variant.
    fn check_parity(prefix: &str, sep: &str) -> Result<(), proptest::test_runner::TestCaseError> {
        let deep = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(300));
        let shallow = format!("{prefix}{sep}{}\nc: \"\"\n", bomb(1));
        if libyaml_bomb_causes_deep_nesting(&deep, &shallow) {
            proptest::prop_assert!(
                !flow_depth_within_limit_with(&deep, 200),
                "scanner missed a real flow bomb: {deep:?}"
            );
        }
        let alias = format!("{prefix}{sep}*undefined_zz\nc: \"\"\n");
        if libyaml_sees_alias(&alias) {
            proptest::prop_assert!(contains_alias(&alias), "missed alias: {alias:?}");
        }
        Ok(())
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
        "\\",
        "\t",
        "\u{85}",
        "\r\n",
        "---\n",
        "a#b",
        "k: |\n  ",
        "- k: |\n    ",
    ];
    const PARITY_SEPS: &[&str] = &["\n", " ", "\n  ", ", ", "\nb: "];

    proptest::proptest! {
        /// Differential parity against the real parser: whatever adversarial
        /// prefix precedes a deep flow bomb, if libyaml nests it the scanner must
        /// see it; and whatever precedes an alias, if libyaml resolves it as an
        /// alias token the short-circuit must report one.
        #[test]
        fn scanner_never_misses_what_libyaml_sees(
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
        fn scanner_never_misses_what_libyaml_sees_with_bom(
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
            !expansion_within_limit_with(&bomb, 10_000),
            "single-level alias bomb must be rejected"
        );
        // Merge-key (`<<: *a`) is the same materialization path -> also rejected.
        let merge: String = (0..500).map(|_| "  - <<: *a\n".to_string()).collect();
        let merge_bomb = format!(
            "anchor: &a {{a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0, h: 0, i: 0, j: 0}}\nrefs:\n{merge}"
        );
        assert!(
            !expansion_within_limit_with(&merge_bomb, 1_000),
            "merge-key alias bomb must be rejected"
        );
        // Legit DRY alias use stays well under budget.
        let mut dry = String::from("d: &d {a: 1, b: 2, c: 3}\nitems:\n");
        for _ in 0..50 {
            dry.push_str("  - <<: *d\n    n: 1\n");
        }
        assert!(
            expansion_within_limit_with(&dry, 10_000),
            "legit DRY alias bundle must pass"
        );
        // Alias-free text short-circuits (never even parses here) -> always within.
        assert!(expansion_within_limit_with(&"k: v\n".repeat(100_000), 10));
        // A quoted glob is not an alias -> short-circuits, passes.
        assert!(expansion_within_limit_with("paths:\n  - \"**/*.rs\"\n", 5));
    }
}
