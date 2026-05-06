//! Domain-specific hints for `JSONPath` parse errors.
//!
//! `serde_json_path` is RFC 9535-compliant. The authoring mistake
//! the P2a launch-prep validation pass surfaced and confirmed against
//! `serde_json_path` 0.7.x is:
//!
//! - **§ 10** — dashed keys after a `.` segment (e.g.
//!   `$.package-name` or `$.foo[?@.dashed-key == 'x']`) are rejected;
//!   bracket notation (`$['package-name']`, `$.foo[?@['dashed-key']`)
//!   is required by the spec. This applies in both top-level path
//!   segments and inside filter expressions.
//!
//! (A previously-suspected pitfall — outer parentheses on filter
//! predicates `$.foo[?(@.bar == 'baz')]` — was investigated during
//! Phase 4 and confirmed valid against `serde_json_path` 0.7.2; the
//! original report mis-attributed a dashed-key error to the parens.
//! See the v0.9.15 Phase 4 commit for the test that proved it.)
//!
//! The raw `serde_json_path` error message says *what* failed
//! (lexical / syntax) but not *why*. This helper inspects the source
//! path for the dashed-key pattern and appends a hint pointing at the
//! canonical-correct form.

use std::sync::OnceLock;

use regex::Regex;

fn dashed_after_dot_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // `.` followed by an identifier-start that contains a dash
        // before the next segment boundary. Catches `$.foo-bar`,
        // `$.a.foo-bar.b`, `$['x'].foo-bar`, etc.
        Regex::new(r"\.([A-Za-z_][A-Za-z0-9_]*-[A-Za-z0-9_-]+)").expect("static regex")
    })
}

/// Inspect a `JSONPath` source string for known-bad patterns and
/// return a hint string when one applies.
///
/// Returns `None` for paths that pass the inspection — callers should
/// surface the raw `serde_json_path` error unchanged in that case.
pub fn diagnose_path(path: &str) -> Option<String> {
    // Pitfall #10: dashed key after `.` segment (in any position —
    // top-level or inside a filter). RFC 9535 dot-notation requires
    // identifier-shape keys; dashed keys must use bracket notation.
    if let Some(cap) = dashed_after_dot_re().captures(path)
        && let Some(key_match) = cap.get(1)
    {
        let key = key_match.as_str();
        return Some(format!(
            "JSONPath dot-notation requires identifier-shape keys (RFC 9535). For dashed keys, use \
             bracket notation: `$['{key}']` instead of `$.{key}` (or `@['{key}']` instead of \
             `@.{key}` inside a filter). See `docs/development/CONFIG-AUTHORING.md` § 10.",
        ));
    }

    None
}

/// Build a complete error-message string for a `JSONPath` parse
/// failure: `invalid JSONPath "<path>": <serde_json_path err>` plus
/// any domain-specific hint from [`diagnose_path`].
pub fn format_parse_error(path: &str, err: impl std::fmt::Display) -> String {
    let base = format!("invalid JSONPath {path:?}: {err}");
    match diagnose_path(path) {
        Some(hint) => format!("{base}\n  hint: {hint}"),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashed_key_inside_filter_gets_hint() {
        // The arrow case study originally attributed the failure to
        // outer parens; in fact `serde_json_path` 0.7.x accepts
        // outer-parens filters and the real issue was the dashed
        // `package-ecosystem` key inside. The hint should still fire
        // and point at bracket notation.
        let path = "$.updates[?(@.package-ecosystem == 'github-actions')]";
        let hint = diagnose_path(path).expect("should diagnose");
        assert!(hint.contains("$['package-ecosystem']"), "hint: {hint}");
        assert!(
            hint.contains("@['package-ecosystem']"),
            "should mention filter form: {hint}",
        );
    }

    #[test]
    fn dashed_key_after_dot_gets_hint() {
        let path = "$.package-name";
        let hint = diagnose_path(path).expect("should diagnose");
        assert!(hint.contains("$['package-name']"), "hint: {hint}");
        assert!(hint.contains("§ 10"), "hint: {hint}");
    }

    #[test]
    fn dashed_key_in_middle_path_gets_hint() {
        let path = "$.foo.dashed-key.bar";
        let hint = diagnose_path(path).expect("should diagnose");
        assert!(hint.contains("$['dashed-key']"), "hint: {hint}");
    }

    #[test]
    fn already_correct_bracket_notation_no_hint() {
        let path = "$['package-name']";
        assert!(diagnose_path(path).is_none());
    }

    #[test]
    fn correct_filter_no_hint() {
        let path = "$.updates[?@.bar == 'baz']";
        assert!(diagnose_path(path).is_none());
    }

    #[test]
    fn outer_parens_alone_no_hint() {
        // `serde_json_path` 0.7.x accepts outer-parens filter
        // predicates — there's nothing to diagnose.
        let path = "$.updates[?(@.bar == 'baz')]";
        assert!(diagnose_path(path).is_none());
    }

    #[test]
    fn plain_dot_path_no_hint() {
        let path = "$.package.edition";
        assert!(diagnose_path(path).is_none());
    }

    #[test]
    fn format_parse_error_includes_hint_when_diagnosed() {
        let out = format_parse_error("$.foo-bar", "syntax error at column 7");
        assert!(out.contains("invalid JSONPath"), "out: {out}");
        assert!(out.contains("syntax error"), "out: {out}");
        assert!(out.contains("hint:"), "out: {out}");
        assert!(out.contains("$['foo-bar']"), "out: {out}");
    }

    #[test]
    fn format_parse_error_no_hint_when_undiagnosed() {
        let out = format_parse_error("$.foo[", "unterminated bracket");
        assert!(out.contains("invalid JSONPath"), "out: {out}");
        assert!(out.contains("unterminated bracket"), "out: {out}");
        assert!(!out.contains("hint:"), "out: {out}");
    }
}
