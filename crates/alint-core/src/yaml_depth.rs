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
            b'"' => {
                // Double-quoted scalar: `\` escapes the next byte.
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                prev_significant = b'"';
            }
            b'\'' => {
                // Single-quoted scalar: `''` is an escaped quote.
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                prev_significant = b'\'';
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

/// Lexical scan for an unquoted YAML alias reference (`*name`), skipping quoted
/// scalars (so a `"**/*.rs"` glob doesn't count) and line comments. Conservative:
/// a stray unquoted `*` merely triggers the (still-correct) counting pass; the only
/// property that matters is never MISSING a real alias, which sits at a node
/// position and is caught here.
fn contains_alias(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    // `true` at line start or right after whitespace -- where a `#` opens a comment.
    let mut prev_ws = true;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                prev_ws = false;
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
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
                i += 1;
                prev_ws = false;
            }
            c => {
                prev_ws = c == b' ' || c == b'\t' || c == b'\n' || c == b'\r';
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
