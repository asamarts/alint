//! Pre-parse bound on YAML flow-collection nesting depth.
//!
//! `serde_yaml_ng`/libyaml is super-linear on deeply-nested FLOW collections
//! (`[[[…]]]` / `{{{…}}}`): a ~40 KB `[`×20000 document already takes seconds,
//! and a slightly deeper one hangs the whole run — an algorithmic-complexity `DoS`
//! reachable both from a `yaml_path_*` rule over crafted repo content and from a
//! deeply-nested config / `extends:`'d ruleset (which the config loader parses
//! with the same library). libyaml has no nesting limit and the slowness is in
//! its tokenizer, so only a cheap pre-parse scan of the raw text can bound it.
//!
//! This tracks FLOW nesting specifically (block/indentation nesting is bounded
//! by the file's byte size and isn't the pathological case), distinguishing a
//! genuine flow open from a `[`/`{` that merely sits inside a plain scalar
//! (`key: a[b` is a valid scalar, not a nested sequence) so it never
//! false-rejects ordinary YAML.

/// Real config/manifest YAML nests a handful of flow levels; anything past this
/// is a bomb. Chosen far above any legitimate document yet far below the depth
/// where libyaml starts to slow (~tens of thousands), so the margin is huge in
/// both directions.
pub const MAX_YAML_FLOW_DEPTH: usize = 1024;

/// `true` when the YAML text's flow-collection nesting stays within
/// [`MAX_YAML_FLOW_DEPTH`]. A cheap single-pass scan; skips quoted scalars and
/// end-of-line comments so their contents don't count.
#[must_use]
pub fn flow_depth_within_limit(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut depth = 0usize;
    // The last significant non-space byte seen in BLOCK context, used to decide
    // whether a `[`/`{` opens a flow node (it follows `:`/`-`/`,`/`?`/`[`/`{` or
    // starts a line) or is just a char inside a plain scalar (follows a scalar
    // byte). `0` = line start.
    let mut prev_significant: u8 = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' => {
                prev_significant = 0;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'"' | b'\'' if opens_yaml_node(prev_significant) || depth > 0 => {
                // A genuine quoted scalar (opens at a node position, or anywhere
                // inside a flow collection): skip its content so brackets inside it
                // don't count. A quote in mid-plain-scalar (`size: 12" wide`) is NOT
                // a node position and falls to the `_` arm as an ordinary byte -- its
                // brackets already can't count (they'd follow a scalar byte), and
                // skipping from there could hide a real flow bomb on a later line.
                // An unterminated quoted scalar is malformed; the real parse errors
                // on it, so treating the rest as skipped here is safe.
                let _closed = skip_quoted_scalar(bytes, &mut i, c);
                prev_significant = c;
            }
            b'&' | b'!' if opens_yaml_node(prev_significant) => {
                // An anchor/tag decorates the node that FOLLOWS it, so the quoted
                // scalar or flow open after `&a`/`!!str` is still at a node position
                // (`b':'` keeps that status) -- otherwise brackets inside an anchored
                // quoted scalar would be miscounted.
                skip_anchor_or_tag(bytes, &mut i);
                prev_significant = b':';
            }
            b'#' if depth == 0 && (prev_significant == 0 || prev_significant == b' ') => {
                // A `#` is a comment only at line start or after whitespace
                // (which this branch's guard approximates via `prev_significant`,
                // reset to a space by the whitespace arm... conservatively we
                // only treat line-start `#` as a comment to avoid ever skipping a
                // bomb). Skip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'[' | b'{' => {
                // In flow context every open nests. In block context, only an
                // open at a node position (line start, or after `:`/`-`/`,`/`?`)
                // starts a flow collection; one after a scalar byte is part of a
                // plain scalar and must NOT count.
                let opens_flow = depth > 0
                    || matches!(
                        prev_significant,
                        0 | b':' | b'-' | b',' | b'?' | b'[' | b'{'
                    );
                if opens_flow {
                    depth += 1;
                    if depth > MAX_YAML_FLOW_DEPTH {
                        return false;
                    }
                }
                prev_significant = c;
                i += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                prev_significant = c;
                i += 1;
            }
            _ => {
                prev_significant = c;
                i += 1;
            }
        }
    }
    true
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

/// `true` when a `[`/`{`/quote sitting at this preceding-significant-byte context
/// opens a genuine YAML node (the value of a mapping, a sequence item, or a flow
/// element) rather than being ordinary plain-scalar content. `0` = line start.
///
/// This is the crux of not being fooled by a quote *inside* a plain scalar: in
/// `desc: it's fine` the `'` follows a scalar byte (`t`), so it is NOT a node
/// position and must not be treated as a quoted-scalar delimiter; in `desc: 'x'`
/// the `'` follows `:` (a node position) and genuinely opens a quoted scalar.
fn opens_yaml_node(prev_significant: u8) -> bool {
    matches!(
        prev_significant,
        0 | b':' | b'-' | b',' | b'?' | b'[' | b'{'
    )
}

/// Skip a YAML anchor (`&name`) or tag (`!tag`) token starting at `bytes[*i]`. The
/// node the token decorates follows it, so callers set the preceding-significant
/// context back to a node position afterwards. The token ends at whitespace or a
/// flow indicator / quote / comment; it never crosses a newline, so it cannot
/// swallow a real `*alias` or flow open on a following line.
fn skip_anchor_or_tag(bytes: &[u8], i: &mut usize) {
    *i += 1; // past the `&` or `!`
    while *i < bytes.len() {
        match bytes[*i] {
            b' ' | b'\t' | b'\r' | b'\n' | b'[' | b']' | b'{' | b'}' | b',' | b'"' | b'\''
            | b'#' => break,
            _ => *i += 1,
        }
    }
}

/// Skip a quoted YAML scalar starting at `bytes[*i]` (the opening `quote`). On
/// return `*i` is just past the closing quote (returning `true`) or at
/// `bytes.len()` if the quote was never closed (returning `false`). Handles `\`
/// escapes in double quotes and `''` escapes in single quotes -- the same rules
/// libyaml uses, so the scanner and the real parser agree on where the scalar ends.
fn skip_quoted_scalar(bytes: &[u8], i: &mut usize, quote: u8) -> bool {
    *i += 1; // past the opening quote
    if quote == b'"' {
        while *i < bytes.len() {
            match bytes[*i] {
                b'\\' => *i += 2,
                b'"' => {
                    *i += 1;
                    return true;
                }
                _ => *i += 1,
            }
        }
    } else {
        while *i < bytes.len() {
            if bytes[*i] == b'\'' {
                if bytes.get(*i + 1) == Some(&b'\'') {
                    *i += 2;
                } else {
                    *i += 1;
                    return true;
                }
            } else {
                *i += 1;
            }
        }
    }
    false
}

/// Lexical scan for an unquoted YAML alias reference (`*name`), skipping genuine
/// quoted scalars (so a `"**/*.rs"` glob doesn't count) and line comments.
///
/// Safety property: it must NEVER return `false` when a real alias exists (that
/// would skip the expansion budget pass and re-open the alias-bomb `DoS`). A stray
/// unquoted `*` is a harmless false positive -- it merely triggers the (still
/// correct, still bounded) counting pass. The subtlety is that a quote only opens a
/// scalar-to-skip at a node position: a quote in mid-plain-scalar (`desc: it's fine`,
/// `size: 12" wide`) is ordinary content, and skipping from there to the next quote
/// could swallow a real `*alias` on a later line. So quotes are skipped only via
/// [`opens_yaml_node`], and an unterminated quoted scalar forces the counting pass.
fn contains_alias(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    // Last significant (non-space) byte, `0` at line start: gates both the quote
    // skip (node position only) and the `#` comment (line start / after space).
    let mut prev_significant: u8 = 0;
    // `true` at line start or right after whitespace -- where a `#` opens a comment.
    let mut prev_ws = true;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' | b'\r' => {
                prev_significant = 0;
                prev_ws = true;
                i += 1;
            }
            b' ' | b'\t' => {
                prev_ws = true;
                i += 1;
            }
            b'"' | b'\'' if opens_yaml_node(prev_significant) => {
                if !skip_quoted_scalar(bytes, &mut i, c) {
                    // Unterminated quoted scalar (malformed): don't risk having
                    // skipped a real alias -- fall through to the counting pass.
                    return true;
                }
                prev_significant = c;
                prev_ws = false;
            }
            b'&' | b'!' if opens_yaml_node(prev_significant) => {
                // Skip the anchor/tag token; the node it decorates follows and is
                // still a node position (so a genuine quoted scalar after `&a`/`!!str`
                // is skipped rather than triggering a spurious counting pass).
                skip_anchor_or_tag(bytes, &mut i);
                prev_significant = b':';
                prev_ws = false;
            }
            b'#' if prev_ws => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'*' => {
                // An alias is `*` followed immediately by a non-space (the anchor
                // name); `a * b` (multiplication, space after `*`) is not an alias.
                if bytes.get(i + 1).is_some_and(|c| !c.is_ascii_whitespace()) {
                    return true;
                }
                prev_significant = b'*';
                prev_ws = false;
                i += 1;
            }
            _ => {
                prev_significant = c;
                prev_ws = false;
                i += 1;
            }
        }
    }
    false
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
