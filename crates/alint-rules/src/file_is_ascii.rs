//! `file_is_ascii` — every byte in the file must be < 0x80.
//!
//! Stricter than `file_is_text`: that rule only refuses files
//! that look binary (null bytes, weird ratios). `file_is_ascii`
//! explicitly rejects anything outside the ASCII range — useful
//! for source trees that want to keep identifiers and comments
//! in plain ASCII for portability / grep-ability.
//!
//! Check-only: auto-picking a replacement for non-ASCII bytes
//! would silently lose meaning. Users either rewrite manually
//! or loosen the rule to `file_is_text`.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};

#[derive(Debug)]
pub struct FileIsAsciiRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for FileIsAsciiRule {
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
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            if let Some(pos) = first_non_ascii(&bytes) {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!("non-ASCII byte 0x{:02X} at offset {pos}", bytes[pos])
                });
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

fn first_non_ascii(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b >= 0x80)
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_is_ascii requires a `paths` field"))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_is_ascii has no fix op — replacement for non-ASCII bytes is ambiguous",
        ));
    }
    Ok(Box::new(FileIsAsciiRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn empty_file_passes() {
        assert_eq!(first_non_ascii(b""), None);
    }
}
