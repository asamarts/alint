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
        (
            "json_path_equals"
            | "yaml_path_equals"
            | "toml_path_equals"
            | "xml_path_equals"
            | "dotenv_path_equals"
            | "properties_path_equals"
            | "ini_path_equals"
            | "hcl_path_equals",
            "matches",
        ) => Some("equals"),
        (
            "json_path_matches"
            | "yaml_path_matches"
            | "toml_path_matches"
            | "xml_path_matches"
            | "dotenv_path_matches"
            | "properties_path_matches"
            | "ini_path_matches"
            | "hcl_path_matches",
            "equals",
        ) => Some("matches"),

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
/// AFTER the "unknown field" match: these are the expected fields. A prefix
/// before it (the `constraint` kind's "invalid options" prefix) quotes words
/// of its own, so counting tokens from the start would take the unknown
/// field itself for an expected one, at distance 0.
fn extract_expected_fields(msg: &str) -> Vec<&str> {
    let Some(unknown) = unknown_field_re().find(msg) else {
        return Vec::new();
    };
    backticked_token_re()
        .captures_iter(&msg[unknown.end()..])
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str())
        .collect()
}

/// Standard Levenshtein edit distance, which the tests check the bounded one
/// against.
#[cfg(test)]
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            CELLS.with(|cells| cells.set(cells.get() + 1));
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The Levenshtein distance between `a` and `b` when it is at most `limit`,
/// computing only the cells within `limit` of the table's diagonal (a path
/// past them costs more than `limit`) and ending at the first row whose cells
/// all exceed it. The unknown field is the key a config wrote, and the
/// expected fields are read off the message, where a key holding backticks
/// adds tokens of its own: a full table over a long key and a token as long
/// cost their product (a 64 KB key, 8 GB), where this costs their length.
fn levenshtein_within(a: &[char], b: &[char], limit: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let over = limit + 1;
    let mut prev: Vec<usize> = (0..=b.len()).map(|j| j.min(over)).collect();
    let mut cur = vec![over; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        let row = i + 1;
        let low = row.saturating_sub(limit).max(1);
        let high = (row + limit).min(b.len());
        cur[0] = row.min(over);
        if low > 1 {
            cur[low - 1] = over;
        }
        let mut least = cur[low - 1];
        for j in low..=high {
            #[cfg(test)]
            CELLS.with(|cells| cells.set(cells.get() + 1));
            let cost = usize::from(*ca != b[j - 1]);
            let cell = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + cost)
                .min(over);
            cur[j] = cell;
            least = least.min(cell);
        }
        if least > limit {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev[b.len()]).filter(|distance| *distance <= limit)
}

#[cfg(test)]
thread_local! {
    /// The cells [`levenshtein`] and [`levenshtein_within`] computed on this
    /// thread.
    static CELLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Largest edit distance we ever surface as a "did you mean" suggestion.
const MAX_SUGGEST_DISTANCE: usize = 2;

/// Find the closest expected field to `wrong` by Levenshtein distance, but only
/// suggest if distance is ≤ [`MAX_SUGGEST_DISTANCE`] (otherwise the
/// "suggestion" is more confusing than nothing).
fn levenshtein_suggestion<'a>(wrong: &str, expected: &[&'a str]) -> Option<&'a str> {
    // Edit distance is >= the difference in length, and we only suggest within
    // `MAX_SUGGEST_DISTANCE`. So skip any candidate whose length differs by more
    // than that before reading it further (L9): every real field name is short,
    // so a huge `wrong` matches none of them. The rest cost their length each.
    let wrong: Vec<char> = wrong.chars().collect();
    expected
        .iter()
        .filter(|e| wrong.len().abs_diff(e.chars().count()) <= MAX_SUGGEST_DISTANCE)
        .filter_map(|&e| {
            let candidate: Vec<char> = e.chars().collect();
            Some((
                e,
                levenshtein_within(&wrong, &candidate, MAX_SUGGEST_DISTANCE)?,
            ))
        })
        .min_by_key(|&(_, d)| d)
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

    #[test]
    fn equals_matches_hint_covers_every_format() {
        // Parity: the equals<->matches confusion hint must cover every format's
        // `<fmt>_path_{equals,matches}`, so xml (and future formats) aren't left
        // with a generic error. Iterates the SSOT `Format::ALL`.
        use crate::structured_format::Format;
        for f in Format::ALL {
            let fmt = f.label().to_lowercase();
            assert_eq!(
                super::curated_suggestion(&format!("{fmt}_path_equals"), "matches"),
                Some("equals"),
                "{fmt}_path_equals should suggest `equals`"
            );
            assert_eq!(
                super::curated_suggestion(&format!("{fmt}_path_matches"), "equals"),
                Some("matches"),
                "{fmt}_path_matches should suggest `matches`"
            );
        }
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

    /// A prefix that quotes words of its own, as the `constraint` kind's
    /// does, adds none of them, and never the unknown field.
    #[test]
    fn extract_expected_reads_only_past_the_unknown_field() {
        let msg = "invalid `constraint` options: unknown field `determine`, expected `key` or \
                   `determines`";
        assert_eq!(extract_expected_fields(msg), vec!["key", "determines"]);
        assert!(
            enrich("constraint", msg).ends_with("did you mean: `determines`?"),
            "{}",
            enrich("constraint", msg)
        );
        assert_eq!(extract_expected_fields("expected `a` or `b`"), [] as [&str; 0]);
    }

    #[test]
    fn extract_expected_handles_singular_form() {
        // serde uses singular `expected \`X\`` when there's only one.
        let msg = "unknown field `foo`, expected `bar`";
        let expected = extract_expected_fields(msg);
        assert_eq!(expected, vec!["bar"]);
    }

    /// The bounded distance is the distance wherever that is within the
    /// limit: every pair of strings of up to five characters over a
    /// three-letter alphabet, each limit up to three.
    #[test]
    fn the_bounded_distance_is_the_distance_within_its_limit() {
        let mut words = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..5 {
            last = last
                .iter()
                .flat_map(|word| ['a', 'b', 'é'].map(|c| format!("{word}{c}")))
                .collect();
            words.extend(last.iter().cloned());
        }
        for a in &words {
            let a_chars: Vec<char> = a.chars().collect();
            for b in &words {
                let b_chars: Vec<char> = b.chars().collect();
                let distance = levenshtein(a, b);
                for limit in 0..=3 {
                    assert_eq!(
                        levenshtein_within(&a_chars, &b_chars, limit),
                        (distance <= limit).then_some(distance),
                        "{a:?} {b:?} {limit}"
                    );
                }
            }
        }
    }

    /// A key holding backticks adds tokens to the expected fields serde's
    /// message lists, one as long as the key: the suggestion weighs it in
    /// work linear in its length, where a full table cost the product (a
    /// 64 KB key took 8 GB).
    #[test]
    fn a_long_key_and_a_token_it_adds_cost_their_length() {
        let key = "a".repeat(3_000);
        let near = format!("{}b", "a".repeat(2_999));
        let message = format!("unknown field `{key}` `{near}`, expected `pattern`");
        CELLS.with(|cells| cells.set(0));
        let out = enrich("file_content_forbidden", &message);
        assert!(out.ends_with(&format!("did you mean: `{near}`?")), "{out}");
        let cells = CELLS.with(std::cell::Cell::get);
        assert!(cells <= 5 * key.len(), "{cells} cells");
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
