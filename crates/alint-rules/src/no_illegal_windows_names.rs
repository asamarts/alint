//! `no_illegal_windows_names` — reject path components that
//! Windows can't represent or restore from a checkout.
//!
//! Categories flagged (case-insensitive for the reserved names):
//!
//! - Reserved device names: `CON`, `PRN`, `AUX`, `NUL`,
//!   `COM1..COM9`, `LPT1..LPT9`. Reserved regardless of extension,
//!   so `con.txt` and `nul.py` also fail.
//! - Trailing dots or spaces (`foo.` / `foo `): both get stripped
//!   silently by Windows and break git checkout round-trips.
//! - Characters Windows disallows in filenames: `<`, `>`, `:`,
//!   `"`, `|`, `?`, `*`. (`/` and `\\` are path separators in
//!   alint's Unix-shaped indexes; we don't flag them.)
//!
//! Check-only. The "correct" rename is a user decision.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, ScopeFilter, Violation};

#[derive(Debug)]
pub struct NoIllegalWindowsNamesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    scope_filter: Option<ScopeFilter>,
}

impl Rule for NoIllegalWindowsNamesRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            if let Some(filter) = &self.scope_filter
                && !filter.matches(&entry.path, ctx.index)
            {
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

    fn scope_filter(&self) -> Option<&ScopeFilter> {
        self.scope_filter.as_ref()
    }
}

/// Classify a single path component. Returns a human-readable
/// reason if it's Windows-illegal, `None` otherwise.
pub fn illegal_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
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
    // `/` and `\` are path separators in our Unix-shaped indexes;
    // they won't appear inside a single path component.
    matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')
}

fn is_reserved_device_name(name: &str) -> bool {
    // The reservation applies to the stem regardless of extension.
    let stem = match name.find('.') {
        Some(idx) => &name[..idx],
        None => name,
    };
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec.paths.as_ref().ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            "no_illegal_windows_names requires a `paths` field (often `\"**\"`)",
        )
    })?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "no_illegal_windows_names has no fix op — renames aren't deterministic",
        ));
    }
    Ok(Box::new(NoIllegalWindowsNamesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        scope_filter: spec.parse_scope_filter()?,
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
        assert!(illegal_reason("COM0").is_none());
        assert!(illegal_reason("COM10").is_none());
        assert!(illegal_reason("LPT0").is_none());
        assert!(illegal_reason("confused").is_none());
        assert!(illegal_reason("conventional").is_none());
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
