//! Did-you-mean suggestions for `RuleRegistry::build` errors.
//!
//! When a user writes a config with a field name that doesn't match the
//! rule kind's expected schema, serde produces an error like:
//!
//! ```text
//! unknown field `argv`, expected one of `command`, `paths`, `timeout`, `level`
//! ```
//!
//! That tells them what they typed is wrong but not what they likely
//! meant. The P2a launch-prep validation pass surfaced 12 distinct
//! schema-rename pitfalls hit by config authors against the live
//! schema; the canonical examples are catalogued in
//! `docs/development/CONFIG-AUTHORING.md`.
//!
//! This module enriches those error messages with a `did you mean: <foo>?`
//! line, using two strategies in order:
//!
//! 1. **Hand-curated overrides** for the highest-drift renames the
//!    pitfall catalogue surfaced (`argv → command`, `secondary →
//!    partner`, `style → target`, `pattern → prefix|suffix`,
//!    `matches → equals` for the structured-path family). These take
//!    precedence over the Levenshtein fallback because the natural-
//!    English drift is too large for edit-distance to bridge (`argv`
//!    is distance 4 from `command` — Levenshtein wouldn't suggest it).
//!
//! 2. **Levenshtein fallback** for the long tail. If no hand-curated
//!    override applies, compute edit distance from the unknown field
//!    to each expected field; suggest the minimum if it's ≤ 2.
//!
//! Wired into [`crate::registry::RuleRegistry::build`] so every rule's
//! options-deserialise error gets enriched at the registry boundary —
//! no per-rule changes required.

use regex::Regex;
use std::sync::OnceLock;

/// Hand-curated rename map for the highest-drift schema renames the
/// P2a pitfall catalogue surfaced. Keyed by `(kind, wrong)` →
/// `right`. When the natural-English drift is too large for
/// Levenshtein distance to bridge, the override below kicks in.
fn curated_suggestion(kind: &str, wrong: &str) -> Option<&'static str> {
    match (kind, wrong) {
        // `command` rule — argv is the conventional Go/Rust/JS name;
        // serde wants `command`. Pitfall #1.
        ("command", "argv") => Some("command"),

        // `pair` rule — `secondary` is the natural English word;
        // schema wants `partner`. Pitfall #4.
        ("pair", "secondary") => Some("partner"),

        // `line_endings` rule — `style` is the conventional name in
        // most editor configs; schema wants `target`. Pitfall #8.
        ("line_endings", "style") => Some("target"),

        // `file_starts_with` / `file_ends_with` — `pattern` is what
        // the broader content-rule family uses (`file_content_matches`
        // etc.); these literal-anchor rules want `prefix` / `suffix`.
        // Pitfall #9.
        ("file_starts_with", "pattern") => Some("prefix"),
        ("file_ends_with", "pattern") => Some("suffix"),

        // `*_path_matches` against a bool/number/null field is silently
        // wrong at runtime (pitfall #16). The field-rename hint is from
        // `matches:` on a `*_path_equals` rule kind (writers reach for
        // `matches:` because that's what the family's name suggests,
        // but `*_path_equals` only accepts `equals:`). Catches the
        // inverse mistake too.
        ("json_path_equals" | "yaml_path_equals" | "toml_path_equals", "matches") => Some("equals"),
        ("json_path_matches" | "yaml_path_matches" | "toml_path_matches", "equals") => {
            Some("matches")
        }

        _ => None,
    }
}

fn unknown_field_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // serde format: `unknown field \`<wrong>\``
        Regex::new(r"unknown field `([^`]+)`").expect("static regex")
    })
}

fn backticked_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`([^`]+)`").expect("static regex"))
}

/// Extract every backtick-quoted token from the serde error message
/// AFTER the "unknown field" capture. These are the expected fields.
fn extract_expected_fields(msg: &str) -> Vec<&str> {
    backticked_token_re()
        .captures_iter(msg)
        .skip(1)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str())
        .collect()
}

/// Standard Levenshtein edit distance. Linear in `a.len() * b.len()`;
/// fine for field-name comparisons (≤ ~20 chars) and we only run this
/// on the error path, not the hot path.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut dp: Vec<Vec<usize>> = vec![vec![0; m + 1]; n + 1];
    for (i, row) in dp.iter_mut().enumerate().take(n + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(m + 1) {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[n][m]
}

/// Find the closest expected field to `wrong` by Levenshtein
/// distance, but only suggest if distance is ≤ 2 (otherwise the
/// "suggestion" is more confusing than nothing).
/// Largest edit distance we ever surface as a "did you mean" suggestion.
const MAX_SUGGEST_DISTANCE: usize = 2;

fn levenshtein_suggestion<'a>(wrong: &str, expected: &[&'a str]) -> Option<&'a str> {
    // Edit distance is >= the difference in length, and we only suggest within
    // `MAX_SUGGEST_DISTANCE`. So skip any candidate whose length differs by more
    // than that *before* building the O(n*m) matrix (L9): this bounds the work
    // on a hostile multi-kilobyte unknown-field name — every real field name is
    // short, so a huge `wrong` matches nothing and no matrix is ever built.
    let wrong_len = wrong.chars().count();
    expected
        .iter()
        .filter(|e| wrong_len.abs_diff(e.chars().count()) <= MAX_SUGGEST_DISTANCE)
        .map(|&e| (e, levenshtein(wrong, e)))
        .min_by_key(|&(_, d)| d)
        .filter(|&(_, d)| d <= MAX_SUGGEST_DISTANCE)
        .map(|(e, _)| e)
}

/// Take a rule-build error message that may contain a serde
/// "unknown field" message + the rule kind, and return the message
/// enriched with a `did you mean: …?` hint when applicable.
///
/// Strategy:
/// 1. Look for an `unknown field` token in the message.
/// 2. Hand-curated overrides win (kind-specific renames where edit
///    distance to the right field is too large).
/// 3. Else fall back to Levenshtein over the expected fields serde
///    listed (distance ≤ 2 only).
/// 4. If no suggestion, return the original message unchanged.
pub fn enrich(kind: &str, message: &str) -> String {
    let Some(wrong) = unknown_field_re()
        .captures(message)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
    else {
        return message.to_string();
    };

    if let Some(suggested) = curated_suggestion(kind, &wrong) {
        return format!("{message}\n  did you mean: `{suggested}`?");
    }

    let expected = extract_expected_fields(message);
    if let Some(suggested) = levenshtein_suggestion(&wrong, &expected) {
        return format!("{message}\n  did you mean: `{suggested}`?");
    }

    message.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Curated suggestions: the high-drift renames -------------------
    //
    // These are the headline pitfalls from
    // `docs/development/CONFIG-AUTHORING.md`. Each must produce a
    // canonical-correct suggestion regardless of what serde puts in
    // the "expected one of" list.

    #[test]
    fn argv_on_command_rule_suggests_command() {
        let msg = "unknown field `argv`, expected one of `command`, `paths`, `timeout`, `level`";
        let out = enrich("command", msg);
        assert!(out.contains("did you mean: `command`"), "out: {out}");
    }

    #[test]
    fn secondary_on_pair_rule_suggests_partner() {
        let msg = "unknown field `secondary`, expected one of `primary`, `partner`, `level`";
        let out = enrich("pair", msg);
        assert!(out.contains("did you mean: `partner`"), "out: {out}");
    }

    #[test]
    fn style_on_line_endings_suggests_target() {
        let msg = "unknown field `style`, expected one of `paths`, `target`, `level`";
        let out = enrich("line_endings", msg);
        assert!(out.contains("did you mean: `target`"), "out: {out}");
    }

    #[test]
    fn pattern_on_file_starts_with_suggests_prefix() {
        let msg = "unknown field `pattern`, expected one of `paths`, `prefix`, `level`";
        let out = enrich("file_starts_with", msg);
        assert!(out.contains("did you mean: `prefix`"), "out: {out}");
    }

    #[test]
    fn pattern_on_file_ends_with_suggests_suffix() {
        let msg = "unknown field `pattern`, expected one of `paths`, `suffix`, `level`";
        let out = enrich("file_ends_with", msg);
        assert!(out.contains("did you mean: `suffix`"), "out: {out}");
    }

    #[test]
    fn matches_on_path_equals_suggests_equals() {
        let msg = "unknown field `matches`, expected one of `path`, `equals`, `level`";
        let out = enrich("json_path_equals", msg);
        assert!(out.contains("did you mean: `equals`"), "out: {out}");
    }

    #[test]
    fn equals_on_path_matches_suggests_matches() {
        let msg = "unknown field `equals`, expected one of `path`, `matches`, `level`";
        let out = enrich("toml_path_matches", msg);
        assert!(out.contains("did you mean: `matches`"), "out: {out}");
    }

    // --- Levenshtein fallback: the long tail ---------------------------

    #[test]
    fn typo_close_to_expected_field_suggests_via_levenshtein() {
        // "patths" → "paths" is edit distance 1.
        let msg = "unknown field `patths`, expected one of `paths`, `level`";
        let out = enrich("file_exists", msg);
        assert!(out.contains("did you mean: `paths`"), "out: {out}");
    }

    #[test]
    fn typo_with_distance_two_still_suggests() {
        // "lvel" → "level" is edit distance 1.
        let msg = "unknown field `lvel`, expected one of `paths`, `level`";
        let out = enrich("file_exists", msg);
        assert!(out.contains("did you mean: `level`"), "out: {out}");
    }

    #[test]
    fn typo_too_far_does_not_suggest() {
        // "completely_random" has no field within distance ≤ 2.
        let msg = "unknown field `completely_random`, expected one of `paths`, `level`";
        let out = enrich("file_exists", msg);
        assert!(!out.contains("did you mean"), "out: {out}");
    }

    #[test]
    fn huge_unknown_field_is_bounded_and_suggests_nothing() {
        // L9: a hostile multi-kilobyte unknown field must not build a giant
        // edit-distance matrix; it is length-mismatched from every real field
        // by far more than the threshold, so it suggests nothing (and fast).
        let huge = "x".repeat(50_000);
        let msg = format!("unknown field `{huge}`, expected one of `paths`, `level`");
        let out = enrich("file_exists", &msg);
        assert!(
            !out.contains("did you mean"),
            "no suggestion for a huge field"
        );
    }

    #[test]
    fn curated_override_beats_levenshtein() {
        // `argv` → `command` is edit distance 6 (Levenshtein wouldn't
        // suggest it), but the curated override should still fire.
        // Even if `paths` is in the expected list (distance 4 — also
        // would not be suggested by Levenshtein), the curated answer
        // is the right one.
        let msg = "unknown field `argv`, expected one of `command`, `paths`, `timeout`, `level`";
        let out = enrich("command", msg);
        assert!(out.contains("did you mean: `command`"), "out: {out}");
        assert!(!out.contains("did you mean: `paths`"), "out: {out}");
    }

    // --- Pass-through: no enrichment when not applicable ---------------

    #[test]
    fn missing_field_passes_through_unchanged() {
        // "missing field" errors are pitfall #6 territory (level: on
        // the inner rule of for_each_dir); not a did-you-mean case.
        let msg = "missing field `level`";
        let out = enrich("for_each_dir", msg);
        assert_eq!(out, msg);
    }

    #[test]
    fn unrelated_error_passes_through_unchanged() {
        let msg = "invalid type: integer `30`, expected a string";
        let out = enrich("command", msg);
        assert_eq!(out, msg);
    }

    #[test]
    fn empty_message_passes_through() {
        let out = enrich("command", "");
        assert_eq!(out, "");
    }

    #[test]
    fn unknown_field_with_no_close_match_passes_through_unchanged() {
        // `xyz` is distance 4 from `paths`, distance 4 from `level`
        // — both > 2, so no Levenshtein suggestion. Curated has no
        // `xyz` entry. Should pass through.
        let msg = "unknown field `xyz`, expected one of `paths`, `level`";
        let out = enrich("file_exists", msg);
        assert_eq!(out, msg);
    }

    // --- Internal helpers ----------------------------------------------

    #[test]
    fn extract_expected_skips_first_backtick_group() {
        // The first backtick group is the WRONG field; the rest are
        // the EXPECTED fields.
        let msg = "unknown field `argv`, expected one of `command`, `paths`, `level`";
        let expected = extract_expected_fields(msg);
        assert_eq!(expected, vec!["command", "paths", "level"]);
    }

    #[test]
    fn extract_expected_handles_singular_form() {
        // serde uses singular `expected \`X\`` when there's only one.
        let msg = "unknown field `foo`, expected `bar`";
        let expected = extract_expected_fields(msg);
        assert_eq!(expected, vec!["bar"]);
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("paths", "patths"), 1);
        assert_eq!(levenshtein("level", "lvel"), 1);
    }
}
