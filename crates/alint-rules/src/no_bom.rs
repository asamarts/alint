//! `no_bom` — flag files that start with a byte-order mark.
//!
//! Detects:
//!   - UTF-8  : EF BB BF
//!   - UTF-16 LE : FF FE (and not UTF-32LE)
//!   - UTF-16 BE : FE FF
//!   - UTF-32 LE : FF FE 00 00
//!   - UTF-32 BE : 00 00 FE FF
//!
//! Fixable via `file_strip_bom` — removes a leading UTF-8 BOM (a stacked run of
//! them). A UTF-16 / UTF-32 BOM is reported but not auto-fixed: dropping its
//! bytes without transcoding the file would corrupt it.

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::FileStripBomFixer;
use crate::io::read_prefix_n;

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
    /// lint - BOMs are never empty.
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

/// How many leading bytes `no_bom` inspects: enough for a few stacked UTF-8
/// BOMs plus the UTF-16 / UTF-32 mark that may follow them (see
/// [`strippable_bom_run`]).
const BOM_PREFIX_LEN: usize = 16;

/// Whether `file_strip_bom` can strip the leading BOM(s) of `bytes`
/// byte-for-byte: the file starts with a run of UTF-8 BOMs that is NOT followed
/// by a UTF-16 / UTF-32 mark. A UTF-16 / UTF-32 BOM needs transcoding (which the
/// fixer never does), and stripping only the UTF-8 marks in front of one would
/// expose that mark rather than converge. Returns the run length when
/// strippable.
pub fn strippable_bom_run(bytes: &[u8]) -> Option<usize> {
    let run = utf8_bom_run_len(bytes);
    (run > 0 && detect_bom(&bytes[run..]).is_none()).then_some(run)
}

/// Byte length of the run of consecutive UTF-8 BOMs at the start of `bytes`
/// (`0` when it does not start with one). `file_strip_bom` strips exactly this:
/// a UTF-16 / UTF-32 mark cannot be removed byte-for-byte without corrupting the
/// file, so even after a UTF-8 BOM it is left for `check` to report (unfixable).
pub fn utf8_bom_run_len(bytes: &[u8]) -> usize {
    let mut len = 0;
    while bytes[len..].starts_with(&[0xEF, 0xBB, 0xBF]) {
        len += 3;
    }
    len
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
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // Bounded read: BOMs are 2-4 bytes. Solo runs read
            // just the prefix instead of the whole file.
            let full = ctx.root.join(&entry.path);
            // A genuine read error fails CLOSED, matching `check`'s file-major
            // dispatch (audit 2026-10 finding 7); `NotFound` stays a skip.
            let bytes = match read_prefix_n(&full, BOM_PREFIX_LEN) {
                Ok(b) => b,
                Err(e) => {
                    violations.extend(crate::io::io_error_violation(&entry.path, &e));
                    continue;
                }
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for NoBomRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Some(kind) = detect_bom(bytes) else {
            return Ok(Vec::new());
        };
        // Only a UTF-8 BOM (run) is strippable byte-for-byte: a UTF-16 / UTF-32
        // file needs transcoding, which `file_strip_bom` never does, and a UTF-8
        // BOM in front of a UTF-16 / UTF-32 mark would leave that mark behind.
        // Such a finding is reported but marked not fixable (a flag, so the
        // baseline fingerprint is unchanged).
        let fixable = strippable_bom_run(bytes).is_some();
        let msg = self.message.clone().unwrap_or_else(|| {
            if fixable {
                format!("file begins with a {} BOM", kind.name())
            } else {
                format!(
                    "file begins with a {} BOM; not auto-fixed (removing it without \
                     transcoding the file would corrupt it)",
                    if kind == BomKind::Utf8 {
                        "UTF-8 + UTF-16/UTF-32"
                    } else {
                        kind.name()
                    }
                )
            }
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(1, 1)
                .with_not_fixable_if(!fixable),
        ])
    }

    fn max_bytes_needed(&self) -> Option<usize> {
        Some(BOM_PREFIX_LEN)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "no_bom requires a `paths` field"))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileStripBom { file_strip_bom }) => Some(
            FileStripBomFixer::new().with_applicability(
                file_strip_bom
                    .applicability
                    .unwrap_or(alint_core::Applicability::Safe),
            ),
        ),
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
        scope: Scope::from_spec(spec)?,
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

    #[test]
    fn utf8_bom_run_spans_a_stacked_bom() {
        // Regression (round-3 audit F3): a *stack* of UTF-8 BOMs must be measured
        // as a single run so `file_strip_bom` removes it all in one shot.
        // Stripping one mark leaves a leading BOM the rule re-flags -- `fix`
        // never converges. (A stack arises when a tool prepends a UTF-8 BOM to a
        // file that already had one.)
        let two = b"\xEF\xBB\xBF\xEF\xBB\xBF# h\n";
        assert_eq!(utf8_bom_run_len(two), 6);
        // Stripping the whole run yields content with no leading BOM: a genuine
        // fixed point (the rule's pass condition is `detect_bom == None`).
        assert!(detect_bom(&two[6..]).is_none());
        // Degenerate cases: a lone BOM is a run of one; no BOM is 0.
        assert_eq!(utf8_bom_run_len(b"\xEF\xBB\xBFx"), 3);
        assert_eq!(utf8_bom_run_len(b"plain"), 0);
        // A UTF-16 mark is never part of the strippable run (stripping it needs
        // transcoding): after the UTF-8 mark it is left for the check to report
        // as not auto-fixable, and the fixer's binary guard refuses that file.
        assert_eq!(utf8_bom_run_len(b"\xEF\xBB\xBF\xFE\xFFx"), 3);
        assert_eq!(utf8_bom_run_len(b"\xFF\xFEx"), 0);
    }
}

#[cfg(test)]
mod utf16_tests {
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
    use alint_core::Fixer;

    /// UTF-16 LE text of `中文` behind its BOM: NO NUL byte, so the old
    /// NUL-only binary guard let byte-level fixers edit it.
    const UTF16_NO_NUL: &[u8] = &[0xFF, 0xFE, 0x2D, 0x4E, 0x87, 0x65];

    #[test]
    fn utf16_bom_is_flagged_but_never_stripped() {
        // Stripping a UTF-16 BOM without transcoding corrupts the file (the
        // code units lose their byte-order signature), and `check` must not
        // promise a fix the fixer refuses: the finding is reported, not fixable.
        let rule = super::build(&spec_yaml(
            "id: t\nkind: no_bom\npaths: \"**/*\"\nlevel: warning\n\
             fix:\n  file_strip_bom: {}\n",
        ))
        .unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.txt", UTF16_NO_NUL)]);
        let vs = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(vs.len(), 1, "a UTF-16 BOM is still reported");
        let fixer = rule.fixer().unwrap();
        assert!(vs[0].not_fixable, "but not tagged fixable");
        assert_eq!(
            vs[0].baseline_key, None,
            "fixability must not change the baseline fingerprint"
        );
        assert!(
            fixer.fix_edit(&vs[0], UTF16_NO_NUL, tmp.path()).is_none(),
            "the editor path must not strip it"
        );
        let fctx = alint_core::FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = fixer.apply(&vs[0], &fctx).unwrap();
        assert!(
            matches!(outcome, alint_core::FixOutcome::Skipped(_)),
            "{outcome:?}"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("a.txt")).unwrap(),
            UTF16_NO_NUL
        );
        // A UTF-8 BOM stays reported AND fixable.
        let (tmp, idx) = tempdir_with_files(&[("b.txt", b"\xEF\xBB\xBFhi\n")]);
        let vs = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(!vs[0].not_fixable);
    }

    #[test]
    fn byte_level_hygiene_fixers_never_edit_utf16_text() {
        // `\n` appended to UTF-16 is half a code unit; a stripped `0x20` byte
        // may be half of one. Every byte-level fixer refuses BOM-marked
        // UTF-16 / UTF-32 content, even without a NUL byte.
        let v = alint_core::Violation::new("x").with_path(std::path::Path::new("a.txt"));
        let root = std::path::Path::new("/r");
        let fixers: [&dyn Fixer; 3] = [
            &crate::fixers::FileAppendFinalNewlineFixer::new(),
            &crate::fixers::FileTrimTrailingWhitespaceFixer::new(),
            &crate::fixers::FileStripZeroWidthFixer,
        ];
        let trailing_space_unit: &[u8] = &[0xFF, 0xFE, 0x20, 0x4E, 0x20, 0x4E];
        for f in fixers {
            for body in [UTF16_NO_NUL, trailing_space_unit] {
                assert!(f.fix_edit(&v, body, root).is_none(), "{}", f.describe());
            }
        }
        assert!(crate::io::looks_binary(UTF16_NO_NUL));
        assert!(crate::io::looks_binary(&[0xFE, 0xFF, 0x4E, 0x2D]));
        assert!(!crate::io::looks_binary(b"\xEF\xBB\xBFplain utf-8\n"));
    }

    #[test]
    fn utf8_bom_in_front_of_a_utf16_mark_is_not_fixable() {
        // Stripping the UTF-8 BOM would only expose the UTF-16 mark behind it
        // (never converging), so the finding is reported but not fixable and the
        // fixer leaves the file alone.
        let rule = super::build(&spec_yaml(
            "id: t\nkind: no_bom\npaths: \"**/*\"\nlevel: warning\n\
             fix:\n  file_strip_bom: {}\n",
        ))
        .unwrap();
        let body: &[u8] = b"\xEF\xBB\xBF\xEF\xBB\xBF\xFE\xFFxx";
        let (tmp, idx) = tempdir_with_files(&[("a.txt", body)]);
        let vs = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(vs.len(), 1);
        assert!(vs[0].not_fixable);
        assert!(
            rule.fixer()
                .unwrap()
                .fix_edit(&vs[0], body, tmp.path())
                .is_none()
        );
    }
}
