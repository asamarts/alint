//! `file_is_ascii` — every byte in the file must be < 0x80, except
//! codepoints listed in `allow:`.
//!
//! Stricter than `file_is_text`: that rule only refuses files that
//! look binary (null bytes, weird ratios). `file_is_ascii`
//! explicitly rejects anything outside the ASCII range — useful for
//! source trees that want to keep identifiers and comments in plain
//! ASCII for portability / grep-ability.
//!
//! `allow:` exempts specific non-ASCII codepoints (curl keeps its
//! source ASCII but allows `ö` in "Björn"; the recurring need across
//! llvm / vscode / elixir). Each entry is a single character
//! (`"ö"`), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive
//! range. With no `allow:` the strict byte-level check (and its fast
//! path) is used; with `allow:` the file is decoded as UTF-8 and
//! checked per character.
//!
//! Check-only: auto-picking a replacement for non-ASCII bytes would
//! silently lose meaning. Users either rewrite manually, add the
//! codepoint to `allow:`, or loosen the rule to `file_is_text`.

use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Permitted non-ASCII codepoints - each a single character
    /// (e.g. "o-umlaut"), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY`
    /// inclusive range.
    #[serde(default)]
    allow: Vec<String>,
}

crate::options_schema_for!(Options);

/// Parse one `allow:` entry into an inclusive codepoint range.
fn parse_allow_entry(s: &str) -> std::result::Result<(u32, u32), String> {
    let s = s.trim();
    // A range is `U+XXXX-U+YYYY` — detected by the `-U+` separator,
    // so a literal `-` (ASCII, never needs allowing) isn't ambiguous.
    if let Some(idx) = s.find("-U+") {
        let lo = parse_codepoint(&s[..idx])?;
        let hi = parse_codepoint(&s[idx + 1..])?;
        if lo > hi {
            return Err(format!(
                "allow range {s:?}: start codepoint is greater than end"
            ));
        }
        return Ok((lo, hi));
    }
    let cp = parse_codepoint(s)?;
    Ok((cp, cp))
}

/// A single character (`"ö"`) or a `U+XXXX` hex codepoint.
fn parse_codepoint(s: &str) -> std::result::Result<u32, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("U+").or_else(|| s.strip_prefix("u+")) {
        return u32::from_str_radix(hex, 16)
            .map_err(|_| format!("allow entry {s:?}: not a valid `U+XXXX` codepoint"));
    }
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c as u32),
        _ => Err(format!(
            "allow entry {s:?} must be a single character or a `U+XXXX` codepoint"
        )),
    }
}

#[derive(Debug)]
pub struct FileIsAsciiRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    /// Inclusive permitted-codepoint ranges. Empty → strict
    /// byte-level check.
    allow: Vec<(u32, u32)>,
}

impl Rule for FileIsAsciiRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileIsAsciiRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // Fast path: no exemptions → the strict byte-level check.
        if self.allow.is_empty() {
            let Some(pos) = first_non_ascii(bytes) else {
                return Ok(Vec::new());
            };
            return Ok(vec![self.violation(
                path,
                &format!("non-ASCII byte 0x{:02X} at offset {pos}", bytes[pos]),
            )]);
        }
        // With exemptions, decode UTF-8 and check each character.
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                for (offset, c) in text.char_indices() {
                    if c.is_ascii() || self.is_allowed(c) {
                        continue;
                    }
                    return Ok(vec![self.violation(
                        path,
                        &format!(
                            "non-ASCII character U+{:04X} {c:?} at offset {offset} \
                             is not in the allow list",
                            c as u32,
                        ),
                    )]);
                }
                Ok(Vec::new())
            }
            Err(e) => {
                // A byte that isn't valid UTF-8 can't be an allowed
                // codepoint, so `allow:` can't exempt it.
                let pos = e.valid_up_to();
                Ok(vec![self.violation(
                    path,
                    &format!(
                        "non-ASCII, non-UTF-8 byte 0x{:02X} at offset {pos} \
                         (cannot be exempted by `allow:`)",
                        bytes.get(pos).copied().unwrap_or(0),
                    ),
                )])
            }
        }
    }
}

impl FileIsAsciiRule {
    fn is_allowed(&self, c: char) -> bool {
        let cp = c as u32;
        self.allow.iter().any(|&(lo, hi)| lo <= cp && cp <= hi)
    }

    fn violation(&self, path: &Path, default: &str) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| default.to_string());
        Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path))
    }
}

fn first_non_ascii(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b >= 0x80)
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    spec.paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_is_ascii requires a `paths` field"))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_is_ascii has no fix op — replacement for non-ASCII bytes is ambiguous",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let mut allow = Vec::with_capacity(opts.allow.len());
    for entry in &opts.allow {
        allow.push(parse_allow_entry(entry).map_err(|e| Error::rule_config(&spec.id, e))?);
    }
    Ok(Box::new(FileIsAsciiRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        allow,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(allow: &[&str]) -> FileIsAsciiRule {
        let allow = allow
            .iter()
            .map(|e| parse_allow_entry(e).expect("valid allow entry"))
            .collect();
        FileIsAsciiRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            allow,
        }
    }

    fn eval(r: &FileIsAsciiRule, bytes: &[u8]) -> Vec<Violation> {
        let ctx = Context {
            root: Path::new("/"),
            index: &alint_core::FileIndex::from_entries(Vec::new()),
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate_file(&ctx, Path::new("f.txt"), bytes).unwrap()
    }

    #[test]
    fn pure_ascii_passes() {
        assert_eq!(first_non_ascii(b"hello world\n"), None);
    }

    #[test]
    fn utf8_snowman_flagged() {
        // ☃ is 0xE2 0x98 0x83 — first high byte at offset 0.
        assert_eq!(first_non_ascii("☃".as_bytes()), Some(0));
    }

    #[test]
    fn tab_and_newline_are_ascii() {
        assert_eq!(first_non_ascii(b"a\tb\nc"), None);
    }

    #[test]
    fn no_allow_uses_strict_byte_path() {
        // The curl "Björn" case fires without an allow list.
        let v = eval(&rule(&[]), "Bj\u{00F6}rn".as_bytes());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("non-ASCII byte"), "{}", v[0].message);
    }

    #[test]
    fn allow_exempts_a_single_char() {
        // `ö` allowed as a literal char and as U+00F6.
        assert!(eval(&rule(&["ö"]), "Bj\u{00F6}rn".as_bytes()).is_empty());
        assert!(eval(&rule(&["U+00F6"]), "Bj\u{00F6}rn".as_bytes()).is_empty());
    }

    #[test]
    fn allow_does_not_exempt_other_codepoints() {
        // `ö` allowed, but `é` (U+00E9) still fires.
        let v = eval(&rule(&["U+00F6"]), "caf\u{00E9}".as_bytes());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("U+00E9"), "{}", v[0].message);
        assert!(v[0].message.contains("allow list"), "{}", v[0].message);
    }

    #[test]
    fn allow_range_covers_a_block() {
        // Allow the whole Latin-1 supplement; both ö and é pass.
        let r = rule(&["U+00A0-U+00FF"]);
        assert!(eval(&r, "Bj\u{00F6}rn caf\u{00E9}".as_bytes()).is_empty());
        // …but a snowman (U+2603) outside the range fires.
        assert_eq!(eval(&r, "\u{2603}".as_bytes()).len(), 1);
    }

    #[test]
    fn invalid_utf8_cannot_be_allowed() {
        // A lone 0xFF is not valid UTF-8, so no codepoint allow can
        // exempt it.
        let v = eval(&rule(&["U+00F6"]), &[b'a', 0xFF, b'b']);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("non-UTF-8"), "{}", v[0].message);
    }

    #[test]
    fn parse_allow_entry_forms() {
        assert_eq!(parse_allow_entry("ö").unwrap(), (0x00F6, 0x00F6));
        assert_eq!(parse_allow_entry("U+00F6").unwrap(), (0x00F6, 0x00F6));
        assert_eq!(
            parse_allow_entry("U+00A0-U+00FF").unwrap(),
            (0x00A0, 0x00FF)
        );
        assert!(parse_allow_entry("U+00FF-U+00A0").is_err()); // reversed range
        assert!(parse_allow_entry("nope").is_err()); // multi-char, not U+
    }
}
