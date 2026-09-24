//! `indent_style` — every non-blank line in each file in scope must
//! indent with the configured style: `tabs` or `spaces`.
//!
//! The check is byte-level and only inspects the *leading* run of
//! whitespace on each line. Mid-line tabs or spaces are not the
//! rule's business (many formatters use a mix for alignment after
//! the indent column).
//!
//! Optional `width`: when `style: spaces`, the number of leading
//! spaces must be an exact multiple of `width`. Ignored for
//! `style: tabs` since a tab is a single character regardless of
//! visual width.
//!
//! Auto-fix (`fix: { indent_style: {} }`) is available for a
//! `style: spaces` + `width: N` rule: the `width` supplies the
//! spaces-per-tab, so a PURE-TAB lead converts unambiguously to
//! `K*N` spaces. The genuinely ambiguous cases are declined (a
//! mixed tab+space lead, a pure-space count that isn't a multiple
//! of `width`), and a `tabs`-style or width-less rule with a `fix`
//! is rejected at load (`spaces -> tabs` has no spaces-per-tab).
//! For those, pair it with your editor's "reindent on save".

use std::path::Path;

use alint_core::{
    Applicability, Context, Error, FixContext, FixEdit, FixOutcome, FixSpec, Fixer, Level,
    PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

/// `baseline_key` markers for the fixable-rule case (`style: spaces` + `width`
/// with a `fix`). The check tags its ONE finding (the first bad line) with
/// whether the `indent_style` reindent fix can resolve it: a PURE-TAB lead is
/// convertible to `K*width` spaces (`FIXABLE`); a mixed tab+space lead or a
/// pure-space width-mismatch is round-ambiguous and left (`UNFIXABLE`). `can_fix`
/// reads this so `check` never advertises an ambiguous line as auto-fixable.
/// Set ONLY when a fixer is attached, so a check-only `indent_style` keeps its
/// offending-line fingerprint (no baseline churn). One finding per file, so
/// (unlike `ordered_block`) no per-line ordinal is needed.
const REINDENT_FIXABLE_KEY: &str = "indent_style\u{0}reindent\u{0}fixable";
const REINDENT_UNFIXABLE_KEY: &str = "indent_style\u{0}reindent\u{0}unfixable";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Required indentation style: `tabs` rejects any leading space; `spaces`
    /// rejects any leading tab.
    style: StyleName,
    /// When `style: spaces`, the leading-space count on every non-blank line
    /// must be a multiple of this. Ignored for `style: tabs`.
    #[serde(default)]
    #[schemars(range(min = 1))]
    width: Option<u32>,
}

crate::options_schema_for!(Options);

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum StyleName {
    Tabs,
    Spaces,
}

#[derive(Debug)]
pub struct IndentStyleRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    style: StyleName,
    width: Option<u32>,
    fixer: Option<IndentStyleReindentFixer>,
}

impl Rule for IndentStyleRule {
    alint_core::rule_common_impl!();

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for IndentStyleRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // The leading-indent scan inspects ASCII whitespace
        // characters and uses `char_indices` to slice the prefix
        // — we keep the UTF-8 validation pass for parity with
        // the rule-major path. Non-UTF-8 files silently skip.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(Vec::new());
        };
        let Some((line_no, reason, fixable)) = first_bad_line(text, self.style, self.width) else {
            return Ok(Vec::new());
        };
        let msg = self.message.clone().unwrap_or_else(|| match reason {
            BadReason::WrongChar => format!(
                "line {line_no} indented with the wrong character (expected {})",
                self.style_name()
            ),
            BadReason::WidthMismatch => format!(
                "line {line_no} has leading spaces that are not a multiple of {}",
                self.width.unwrap_or(0),
            ),
        });
        let mut violation = Violation::new(msg)
            .with_path(std::sync::Arc::<Path>::from(path))
            .with_location(line_no, 1);
        // When a `reindent` fix is attached, tag the finding with whether that fix
        // can resolve THIS bad line, so `can_fix` (and the engine's `is_fixable`
        // tag) stay honest for the ambiguous cases it declines. Key-less otherwise
        // -> a check-only rule's baseline fingerprint is unchanged.
        if self.fixer.is_some() {
            violation = violation.with_baseline_key(if fixable {
                REINDENT_FIXABLE_KEY
            } else {
                REINDENT_UNFIXABLE_KEY
            });
        }
        Ok(vec![violation])
    }
}

impl IndentStyleRule {
    fn style_name(&self) -> &'static str {
        match self.style {
            StyleName::Tabs => "tabs",
            StyleName::Spaces => "spaces",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BadReason {
    WrongChar,
    WidthMismatch,
}

/// Return the 1-based line number of the first line whose leading whitespace
/// violates the configured style, its [`BadReason`], and whether the `reindent`
/// fix can RESOLVE that line (`style: spaces` + `width` + a PURE-TAB lead ->
/// `K*width` spaces). Blank lines (empty or whitespace-only) are skipped so
/// trailing indentation on an otherwise-blank line doesn't cause spurious
/// failures. The fixability flag is meaningful only when a fixer is attached
/// (spaces + width); it is `false` for every `tabs`-style / width-less case.
fn first_bad_line(
    text: &str,
    style: StyleName,
    width: Option<u32>,
) -> Option<(usize, BadReason, bool)> {
    for (idx, line) in text.split('\n').enumerate() {
        let body = line.strip_suffix('\r').unwrap_or(line);
        let lead: &str = body
            .char_indices()
            .find(|(_, c)| *c != ' ' && *c != '\t')
            .map_or(body, |(i, _)| &body[..i]);
        // Blank / whitespace-only line: no indent to judge.
        if lead.len() == body.len() {
            continue;
        }
        let line_no = idx + 1;
        match style {
            StyleName::Tabs => {
                if lead.bytes().any(|b| b == b' ') {
                    // spaces -> tabs needs a spaces-per-tab the rule does not carry.
                    return Some((line_no, BadReason::WrongChar, false));
                }
            }
            StyleName::Spaces => {
                if lead.bytes().any(|b| b == b'\t') {
                    // WrongChar. Reindent-fixable IFF the lead is PURE tab (no
                    // interspersed space -> `K*width` spaces is unambiguous) and a
                    // positive `width` gives the spaces-per-tab. A mixed tab+space
                    // lead is left (rounding the trailing spaces is ambiguous).
                    let fixable = width.is_some_and(|w| w > 0) && lead.bytes().all(|b| b == b'\t');
                    return Some((line_no, BadReason::WrongChar, fixable));
                }
                if let Some(w) = width
                    && w > 0
                    && lead.len() % (w as usize) != 0
                {
                    // Round up or down? Ambiguous -> not reindent-fixable.
                    return Some((line_no, BadReason::WidthMismatch, false));
                }
            }
        }
    }
    None
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "indent_style requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    // The `indent_style` reindent fix needs a spaces-per-tab, which only a
    // `style: spaces` + positive `width:` rule carries. A `tabs`-style rule
    // (spaces -> tabs) or a width-less `spaces` rule has no unambiguous
    // conversion, so the fix is rejected at load rather than silently declined.
    let fixer = match &spec.fix {
        Some(FixSpec::IndentStyle { indent_style }) => {
            let width = match (opts.style, opts.width) {
                (StyleName::Spaces, Some(w)) if w > 0 => w as usize,
                _ => {
                    return Err(Error::rule_config(
                        &spec.id,
                        "indent_style fix requires `style: spaces` and a positive `width:` \
                         (the spaces-per-tab for conversion); a `tabs`-style or width-less rule \
                         has no unambiguous tab/space conversion",
                    ));
                }
            };
            Some(IndentStyleReindentFixer {
                width,
                applicability: indent_style.applicability.unwrap_or(Applicability::Safe),
            })
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with indent_style (only `indent_style` is)",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(IndentStyleRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        style: opts.style,
        width: opts.width,
        fixer,
    }))
}

/// The whole-file `reindent` transform: rewrite each line whose leading
/// whitespace is PURE TABS (K tabs, K>0, no interspersed space) to `K*width`
/// spaces, leaving every other line -- and every line's terminator + the
/// trailing-newline state -- untouched. Returns `None` when nothing changed.
/// Blank / whitespace-only lines are left (the check does not judge them).
fn reindent_pure_tabs(text: &str, width: usize) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for chunk in text.split_inclusive('\n') {
        // Split the chunk into its body (no terminator) and its exact ending.
        let (body, ending): (&str, &str) = if let Some(b) = chunk.strip_suffix('\n') {
            b.strip_suffix('\r')
                .map_or((b, "\n"), |without_cr| (without_cr, "\r\n"))
        } else {
            (chunk, "") // final line, no terminator
        };
        let lead_end = body
            .char_indices()
            .find(|(_, c)| *c != ' ' && *c != '\t')
            .map_or(body.len(), |(i, _)| i);
        let lead = &body[..lead_end];
        // A pure-tab lead on a NON-blank line (there is content after it) converts
        // to `K*width` spaces. Blank / whitespace-only lines (`lead_end` covers the
        // whole body) and any lead containing a space are left verbatim.
        if lead_end < body.len() && !lead.is_empty() && lead.bytes().all(|b| b == b'\t') {
            for _ in 0..(lead.len() * width) {
                out.push(' ');
            }
            out.push_str(&body[lead_end..]);
            out.push_str(ending);
            changed = true;
        } else {
            out.push_str(body);
            out.push_str(ending);
        }
    }
    changed.then_some(out)
}

/// The `indent_style` reindent fix (Phase 4): a whole-file rewrite that converts
/// PURE-TAB leading indentation to `width` spaces per tab, for a `style: spaces`
/// rule that carries a `width`. `Safe` by default (a pure-tab reindent is
/// behavior-preserving for the common case); content-injecting in the W2 partition
/// (a remote could aim it at an indent-significant file). Declines the ambiguous
/// bad lines (a mixed tab+space lead, a pure-space width mismatch) via `can_fix`.
#[derive(Debug, Clone)]
struct IndentStyleReindentFixer {
    /// Spaces per tab (the rule's `width`, guaranteed > 0 at build).
    width: usize,
    applicability: Applicability,
}

impl Fixer for IndentStyleReindentFixer {
    fn describe(&self) -> String {
        format!(
            "reindent tab-indented lines to {} spaces per tab",
            self.width
        )
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, violation: &Violation) -> bool {
        // Only the finding the check tagged FIXABLE (a pure-tab lead) is
        // reindentable; a mixed lead or a pure-space width-mismatch is declined.
        violation.baseline_key.as_deref() == Some(REINDENT_FIXABLE_KEY)
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let Ok(text) = std::str::from_utf8(&existing) else {
            // The detector also skips non-UTF-8 (no violation); defensive.
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot reindent",
                path.display()
            )));
        };
        let Some(out) = reindent_pure_tabs(text, self.width) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} has no pure-tab indentation to convert",
                path.display()
            )));
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would reindent tab-indented lines in {} to {} spaces per tab",
                path.display(),
                self.width
            )));
        }
        ctx.commit_write(&abs, out.as_bytes())
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "reindented tab-indented lines in {} to {} spaces per tab",
            path.display(),
            self.width
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        if !self.can_fix(violation) {
            return None;
        }
        let path = violation.path.as_deref()?;
        let text = std::str::from_utf8(bytes).ok()?;
        let out = reindent_pure_tabs(text, self.width)?;
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out.into_bytes(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_style_accepts_pure_tab_indent() {
        assert_eq!(
            first_bad_line("fn x() {\n\tlet a = 1;\n}\n", StyleName::Tabs, None),
            None
        );
    }

    #[test]
    fn tabs_style_flags_space_indent() {
        let (line, reason, fixable) =
            first_bad_line("fn x() {\n    let a = 1;\n}\n", StyleName::Tabs, None).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WrongChar);
        assert!(
            !fixable,
            "spaces -> tabs has no spaces-per-tab: not fixable"
        );
    }

    #[test]
    fn spaces_style_accepts_pure_space_indent() {
        assert_eq!(
            first_bad_line("x:\n  a: 1\n  b: 2\n", StyleName::Spaces, Some(2)),
            None
        );
    }

    #[test]
    fn spaces_style_flags_tab_indent() {
        let (line, reason, fixable) =
            first_bad_line("x:\n\ta: 1\n", StyleName::Spaces, Some(2)).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WrongChar);
        assert!(
            fixable,
            "a pure-tab lead under spaces+width is reindent-fixable"
        );
    }

    #[test]
    fn spaces_style_flags_width_mismatch() {
        let (line, reason, fixable) =
            first_bad_line("x:\n   a: 1\n", StyleName::Spaces, Some(2)).unwrap();
        assert_eq!(line, 2);
        assert_eq!(reason, BadReason::WidthMismatch);
        assert!(!fixable, "a width mismatch is round-ambiguous: not fixable");
    }

    #[test]
    fn blank_lines_are_not_judged() {
        assert_eq!(first_bad_line("\n   \na\n", StyleName::Tabs, None), None);
    }

    #[test]
    fn crlf_is_handled() {
        assert_eq!(
            first_bad_line("a\r\n  b\r\n", StyleName::Spaces, Some(2)),
            None
        );
    }

    // ----- the `indent_style` reindent fix ------------------------------

    fn spec_for(yaml: &str) -> RuleSpec {
        crate::test_support::spec_yaml(yaml)
    }

    #[test]
    fn reindent_converts_pure_tab_leads_scaled_by_width() {
        // 1 tab -> width spaces, 2 tabs -> 2*width, per level.
        let out = reindent_pure_tabs("fn x() {\n\tone();\n\t\ttwo();\n}\n", 4).unwrap();
        assert_eq!(out, "fn x() {\n    one();\n        two();\n}\n");
    }

    #[test]
    fn reindent_preserves_crlf_and_missing_final_newline() {
        assert_eq!(
            reindent_pure_tabs("\tcrlf();\r\n", 2).unwrap(),
            "  crlf();\r\n"
        );
        // Last line, no terminator.
        assert_eq!(reindent_pure_tabs("\tnonl();", 2).unwrap(), "  nonl();");
    }

    #[test]
    fn reindent_leaves_mixed_and_pure_space_and_blank_lines() {
        // A mixed tab+space lead is NOT converted (ambiguous).
        assert_eq!(reindent_pure_tabs("\t  mixed();\n", 4), None);
        // A pure-space lead is already spaces -> unchanged.
        assert_eq!(reindent_pure_tabs("    spaces();\n", 4), None);
        // A blank / whitespace-only tab line is not judged -> left verbatim.
        assert_eq!(
            reindent_pure_tabs("\t\n\tx();\n", 4).unwrap(),
            "\t\n    x();\n"
        );
    }

    #[test]
    fn reindent_returns_none_when_unchanged() {
        assert_eq!(
            reindent_pure_tabs("no_indent();\n    already();\n", 4),
            None
        );
    }

    #[test]
    fn build_wires_the_fixer_only_for_spaces_plus_width() {
        let ok = build(&spec_for(
            "id: t\nkind: indent_style\npaths: [\"x\"]\nstyle: spaces\nwidth: 4\nlevel: error\nfix: { indent_style: {} }\n",
        ))
        .unwrap();
        assert!(ok.fixer().is_some(), "spaces + width wires a fixer");
        assert_eq!(ok.fixer().unwrap().applicability(), Applicability::Safe);

        let no_width = build(&spec_for(
            "id: t\nkind: indent_style\npaths: [\"x\"]\nstyle: spaces\nlevel: error\nfix: { indent_style: {} }\n",
        ))
        .unwrap_err();
        assert!(
            no_width.to_string().contains("positive `width:`"),
            "{no_width}"
        );

        let tabs = build(&spec_for(
            "id: t\nkind: indent_style\npaths: [\"x\"]\nstyle: tabs\nlevel: error\nfix: { indent_style: {} }\n",
        ))
        .unwrap_err();
        assert!(tabs.to_string().contains("style: spaces"), "{tabs}");

        let bad_op = build(&spec_for(
            "id: t\nkind: indent_style\npaths: [\"x\"]\nstyle: spaces\nwidth: 4\nlevel: error\nfix: { file_remove: {} }\n",
        ))
        .unwrap_err();
        assert!(bad_op.to_string().contains("not compatible"), "{bad_op}");
    }

    #[test]
    fn build_honors_an_explicit_tier_override() {
        let r = build(&spec_for(
            "id: t\nkind: indent_style\npaths: [\"x\"]\nstyle: spaces\nwidth: 4\nlevel: error\nfix:\n  indent_style:\n    applicability: unsafe\n",
        ))
        .unwrap();
        assert_eq!(r.fixer().unwrap().applicability(), Applicability::Unsafe);
    }

    fn eval_keys(yaml: &str, text: &str) -> Vec<Violation> {
        let rule = build(&spec_for(yaml)).unwrap();
        let ctx = Context {
            root: Path::new("/"),
            index: &alint_core::FileIndex::from_entries(Vec::new()),
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        rule.as_per_file()
            .unwrap()
            .evaluate_file(&ctx, Path::new("f.txt"), text.as_bytes())
            .unwrap()
    }

    const FIX_YAML: &str = "id: t\nkind: indent_style\npaths: [\"**/*\"]\nstyle: spaces\nwidth: 4\nlevel: error\nfix: { indent_style: {} }\n";

    #[test]
    fn check_keys_a_pure_tab_finding_fixable() {
        let v = eval_keys(FIX_YAML, "x:\n\ta();\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].baseline_key.as_deref(), Some(REINDENT_FIXABLE_KEY));
    }

    #[test]
    fn check_keys_a_mixed_lead_finding_unfixable() {
        let v = eval_keys(FIX_YAML, "x:\n\t  a();\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].baseline_key.as_deref(), Some(REINDENT_UNFIXABLE_KEY));
    }

    #[test]
    fn check_keys_a_width_mismatch_finding_unfixable() {
        let v = eval_keys(FIX_YAML, "x:\n   a();\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].baseline_key.as_deref(), Some(REINDENT_UNFIXABLE_KEY));
    }

    #[test]
    fn check_only_rule_leaves_the_finding_key_less() {
        // No fix declared -> no key -> the baseline fingerprint is unchanged.
        let v = eval_keys(
            "id: t\nkind: indent_style\npaths: [\"**/*\"]\nstyle: spaces\nwidth: 4\nlevel: error\n",
            "x:\n\ta();\n",
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].baseline_key.is_none());
    }

    fn reindent_fixer(applicability: Applicability) -> IndentStyleReindentFixer {
        IndentStyleReindentFixer {
            width: 4,
            applicability,
        }
    }

    #[test]
    fn can_fix_only_the_fixable_finding() {
        let f = reindent_fixer(Applicability::Safe);
        let fixable = Violation::new("x").with_baseline_key(REINDENT_FIXABLE_KEY);
        let unfixable = Violation::new("x").with_baseline_key(REINDENT_UNFIXABLE_KEY);
        assert!(f.can_fix(&fixable));
        assert!(!f.can_fix(&unfixable));
    }

    #[test]
    fn apply_reindents_on_disk() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.rs"), "fn x() {\n\tlet a = 1;\n}\n").unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = reindent_fixer(Applicability::Safe)
            .apply(
                &Violation::new("x")
                    .with_path(Path::new("f.rs"))
                    .with_baseline_key(REINDENT_FIXABLE_KEY),
                &ctx,
            )
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("f.rs")).unwrap(),
            "fn x() {\n    let a = 1;\n}\n"
        );
    }

    #[test]
    fn apply_skips_a_file_with_no_pure_tab_indent() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        // Only a width-mismatch (pure-space) line: nothing pure-tab to convert.
        std::fs::write(tmp.path().join("f.rs"), "x:\n   a();\n").unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = reindent_fixer(Applicability::Safe)
            .apply(
                &Violation::new("x")
                    .with_path(Path::new("f.rs"))
                    .with_baseline_key(REINDENT_UNFIXABLE_KEY),
                &ctx,
            )
            .unwrap();
        assert!(
            matches!(&outcome, FixOutcome::Skipped(s) if s.contains("no pure-tab")),
            "{outcome:?}"
        );
    }

    #[test]
    fn fix_converges_for_a_pure_tab_file() {
        // The correlation: check flags a pure-tab file, the fix converts every
        // pure-tab line, and re-check is clean.
        let rule = build(&spec_for(FIX_YAML)).unwrap();
        let text = "fn x() {\n\tone();\n\t\ttwo();\n}\n";
        let flag = eval_keys(FIX_YAML, text);
        assert_eq!(flag.len(), 1, "{flag:?}");
        let out = reindent_pure_tabs(text, 4).unwrap();
        let ctx = Context {
            root: Path::new("/"),
            index: &alint_core::FileIndex::from_entries(Vec::new()),
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        let after = rule
            .as_per_file()
            .unwrap()
            .evaluate_file(&ctx, Path::new("f.txt"), out.as_bytes())
            .unwrap();
        assert!(after.is_empty(), "converged: {after:?}");
    }
}
