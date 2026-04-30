//! `no_bom` — flag files that start with a byte-order mark.
//!
//! Detects:
//!   - UTF-8  : EF BB BF
//!   - UTF-16 LE : FF FE (and not UTF-32LE)
//!   - UTF-16 BE : FE FF
//!   - UTF-32 LE : FF FE 00 00
//!   - UTF-32 BE : 00 00 FE FF
//!
//! Fixable via `file_strip_bom` — removes the leading BOM bytes.

use alint_core::{Context, Error, FixSpec, Fixer, Level, Result, Rule, RuleSpec, Scope, Violation};

use crate::fixers::FileStripBomFixer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BomKind {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl BomKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Utf32Le => "UTF-32 LE",
            Self::Utf32Be => "UTF-32 BE",
        }
    }

    /// Byte count of this BOM sequence. Named `byte_len` rather
    /// than `len` to dodge clippy's "has `len` but no `is_empty`"
    /// lint — BOMs are never empty.
    pub fn byte_len(self) -> usize {
        match self {
            Self::Utf8 => 3,
            Self::Utf16Le | Self::Utf16Be => 2,
            Self::Utf32Le | Self::Utf32Be => 4,
        }
    }
}

/// Detect a BOM at the start of `bytes`. UTF-32 LE (`FF FE 00 00`)
/// is ambiguous with UTF-16 LE (`FF FE`); we check the 4-byte
/// variants first so a UTF-32 LE BOM isn't misclassified.
pub fn detect_bom(bytes: &[u8]) -> Option<BomKind> {
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Some(BomKind::Utf32Le);
    }
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Some(BomKind::Utf32Be);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(BomKind::Utf8);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(BomKind::Utf16Le);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(BomKind::Utf16Be);
    }
    None
}

#[derive(Debug)]
pub struct NoBomRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileStripBomFixer>,
}

impl Rule for NoBomRule {
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
            if let Some(kind) = detect_bom(&bytes) {
                let msg = self
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("file begins with a {} BOM", kind.name()));
                violations.push(
                    Violation::new(msg)
                        .with_path(entry.path.clone())
                        .with_location(1, 1),
                );
            }
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_bom requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileStripBom { .. }) => Some(FileStripBomFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with no_bom", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(NoBomRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_utf8_bom() {
        assert_eq!(detect_bom(b"\xEF\xBB\xBFhello"), Some(BomKind::Utf8));
    }

    #[test]
    fn detects_utf16_le_and_be() {
        assert_eq!(detect_bom(&[0xFF, 0xFE, b'a']), Some(BomKind::Utf16Le));
        assert_eq!(detect_bom(&[0xFE, 0xFF, b'a']), Some(BomKind::Utf16Be));
    }

    #[test]
    fn utf32_le_is_not_misclassified_as_utf16_le() {
        let bytes = [0xFF, 0xFE, 0x00, 0x00, b'a'];
        assert_eq!(detect_bom(&bytes), Some(BomKind::Utf32Le));
    }

    #[test]
    fn no_bom_on_ascii() {
        assert_eq!(detect_bom(b"hello"), None);
        assert_eq!(detect_bom(b""), None);
    }
}
