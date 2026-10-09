//! `no_illegal_windows_names` — reject path components that
//! Windows can't represent or restore from a checkout.
//!
//! Categories flagged (case-insensitive for the reserved names):
//!
//! - Reserved device names: `CON`, `PRN`, `AUX`, `NUL`,
//!   `COM0..COM9`, `LPT0..LPT9`, and the superscript-digit ports
//!   `COM¹ COM² COM³` / `LPT¹ LPT² LPT³`. Reserved regardless of
//!   extension (`con.txt`, `nul.tar.gz`) and of spaces before it
//!   (`CON .txt`).
//! - Trailing dots or spaces (`foo.` / `foo `): both get stripped
//!   silently by Windows and break git checkout round-trips.
//! - Characters Windows disallows in filenames: `<`, `>`, `:`,
//!   `"`, `|`, `?`, `*`, a `\\` inside a component (a separator on
//!   Windows), and the control characters U+0000..U+001F (tab
//!   included).
//!
//! Check-only. The "correct" rename is a user decision.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct NoIllegalWindowsNamesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for NoIllegalWindowsNamesRule {
    /// Expose the per-file scope so the engine resolves this rule's
    /// `scope_filter` (manifest sets, `changed_since:`) before dispatch and
    /// can `--changed`-skip it (see `Rule::path_scope`).
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            for component in entry.path.components() {
                let Some(name) = component.as_os_str().to_str() else {
                    continue;
                };
                if let Some(reason) = illegal_reason(name) {
                    let msg = self
                        .message
                        .clone()
                        .unwrap_or_else(|| format!("{reason}: {name:?}"));
                    violations.push(Violation::new(msg).with_path(entry.path.clone()));
                    break;
                }
            }
        }
        Ok(violations)
    }
}

/// Classify a single path component. Returns a human-readable
/// reason if it's Windows-illegal, `None` otherwise. Follows Microsoft's
/// "Naming Files, Paths, and Namespaces" rules.
pub fn illegal_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    if name.chars().any(|c| c < '\u{20}') {
        return Some("contains a control character Windows forbids in filenames");
    }
    if name.ends_with('.') {
        return Some("Windows strips trailing dots on checkout");
    }
    if name.ends_with(' ') {
        return Some("Windows strips trailing spaces on checkout");
    }
    if name.chars().any(is_reserved_char) {
        return Some("contains a character Windows forbids in filenames");
    }
    if is_reserved_device_name(name) {
        return Some("clashes with a Windows reserved device name");
    }
    None
}

fn is_reserved_char(c: char) -> bool {
    // `/` never appears inside a component. A `\` can, on a Unix index (it is
    // an ordinary filename byte there), but Windows reads it as a separator --
    // so it is flagged; a Windows index never yields one inside a component.
    matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\')
}

fn is_reserved_device_name(name: &str) -> bool {
    // The reservation applies to the stem regardless of extension (`NUL.txt`,
    // `NUL.tar.gz`), and Windows ignores spaces between the device name and the
    // extension, so `CON .txt` opens the CON device too.
    let stem = match name.find('.') {
        Some(idx) => &name[..idx],
        None => name,
    };
    let upper = stem.trim_end_matches(' ').to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    // COM0..COM9 / LPT0..LPT9, plus the superscript-digit ports Windows also
    // reserves (`COM¹`, `COM²`, `COM³`, and the LPT equivalents).
    let Some(port) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = port.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{B9}' | '\u{B2}' | '\u{B3}'), None)
    )
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "no_illegal_windows_names requires a `paths` field (often `\"**\"`)",
        )
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_illegal_windows_names has no fix op - renames aren't deterministic",
        ));
    }
    Ok(Box::new(NoIllegalWindowsNamesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_con_stem() {
        assert!(illegal_reason("CON").is_some());
        assert!(illegal_reason("con").is_some());
        assert!(illegal_reason("con.txt").is_some());
        assert!(illegal_reason("Con.py").is_some());
    }

    #[test]
    fn flags_all_com_and_lpt_families() {
        for i in 1..=9 {
            assert!(illegal_reason(&format!("COM{i}")).is_some());
            assert!(illegal_reason(&format!("LPT{i}")).is_some());
        }
    }

    #[test]
    fn does_not_flag_nearby_non_reserved() {
        assert!(illegal_reason("COM10").is_none());
        assert!(illegal_reason("LPT10").is_none());
        assert!(illegal_reason("COM\u{2074}").is_none()); // superscript 4 is not reserved
        assert!(illegal_reason("confused").is_none());
        assert!(illegal_reason("conventional").is_none());
        assert!(illegal_reason("CONSOLE.txt").is_none());
        assert!(illegal_reason("a.con").is_none());
    }

    #[test]
    fn follows_microsofts_full_naming_rules() {
        // FN regressions against "Naming Files, Paths, and Namespaces":
        // COM0 / LPT0 and the superscript-digit ports are reserved too.
        for name in [
            "COM0",
            "lpt0",
            "COM\u{b9}",
            "COM\u{b2}",
            "com\u{b3}.txt",
            "LPT\u{b9}",
        ] {
            assert!(illegal_reason(name).is_some(), "{name:?}");
        }
        // A reserved stem followed by spaces before the extension still names
        // the device (`CON .txt` opens CON), as does a multi-dot extension.
        for name in ["CON .txt", "nul  .tar.gz", "AUX .", "PRN.tar.gz"] {
            assert!(illegal_reason(name).is_some(), "{name:?}");
        }
        // Control characters (U+0001..U+001F, incl. tab) and NUL are forbidden.
        for name in ["a\tb", "a\u{1}b", "a\u{1f}b", "a\u{0}b"] {
            assert!(illegal_reason(name).is_some(), "{name:?}");
        }
        // A backslash inside a (Unix) path component is a separator on Windows.
        assert!(illegal_reason("a\\b").is_some());
        // DEL (U+007F) and ordinary Unicode are allowed.
        assert!(illegal_reason("a\u{7f}b").is_none());
        assert!(illegal_reason("caf\u{e9}.md").is_none());
    }

    #[test]
    fn flags_trailing_dot_and_space() {
        assert!(illegal_reason("foo.").is_some());
        assert!(illegal_reason("foo ").is_some());
    }

    #[test]
    fn flags_reserved_chars() {
        for c in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(illegal_reason(&format!("bad{c}name")).is_some(), "{c}");
        }
    }

    #[test]
    fn normal_names_pass() {
        assert!(illegal_reason("README.md").is_none());
        assert!(illegal_reason("my-config.yaml").is_none());
        assert!(illegal_reason("src").is_none());
    }

    #[test]
    fn scope_filter_narrows() {
        use crate::test_support::{ctx, index, spec_yaml};
        use std::path::Path;
        // Two illegal-named files; only the one inside a
        // directory with `marker.lock` as ancestor should fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: no_illegal_windows_names\n\
             paths: \"**\"\n\
             scope_filter:\n  \
               has_ancestor: marker.lock\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["pkg/marker.lock", "pkg/CON.txt", "other/CON.txt"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 1, "only in-scope file should fire: {v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("pkg/CON.txt")));
    }
}
