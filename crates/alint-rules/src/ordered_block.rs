//! `ordered_block` — the lines between a `start` / `end` marker
//! pair must stay sorted (optionally unique) under a configurable
//! comparator. Both markers are **optional**: omit `end` to sort
//! from `start` to EOF, omit both to sort the whole file (the
//! markerless "this file is one sorted list" form — dictionaries,
//! `CODEOWNERS`, allow-lists). The generic form of the per-project
//! `keep-sorted` / `keep_sorted` scripts (protobuf `failure_lists`
//! is the highest-stakes source). Per-file rule (the `PerFileRule`
//! fast path), not cross-file. Design + open-question resolutions:
//! `docs/design/v0.10/ordered_block.md`.
//!
//! ```yaml
//! - id: keep-sorted
//!   kind: ordered_block
//!   paths: ["**/.gitignore", "CODEOWNERS"]
//!   start: "# keep-sorted start"   # matched on the trimmed line
//!   end: "# keep-sorted end"
//!   comparator: lexical            # lexical (default) | lexical-ci | numeric
//!   unique: false                  # also forbid duplicate entries
//!   level: warning
//! ```

use std::cmp::Ordering;
use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Comparator {
    /// Rust `str` `Ord` - byte-wise over the UTF-8.
    #[default]
    Lexical,
    /// ASCII-case-insensitive lexical.
    LexicalCi,
    /// Leading-integer order; entries without a leading integer
    /// fall back to `lexical` so a mixed block degrades
    /// predictably rather than panicking.
    Numeric,
}

impl Comparator {
    fn order(self, a: &str, b: &str) -> Ordering {
        match self {
            Self::Lexical => a.cmp(b),
            Self::LexicalCi => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
            Self::Numeric => match (leading_int(a), leading_int(b)) {
                (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
                _ => a.cmp(b),
            },
        }
    }
}

/// The leading (optionally negative) integer of `s`, or `None`
/// when it doesn't start with one.
fn leading_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let b = s.as_bytes();
    let neg = b.first() == Some(&b'-');
    let digits_start = usize::from(neg);
    let digits_end = b[digits_start..]
        .iter()
        .position(|c| !c.is_ascii_digit())
        .map_or(b.len(), |p| digits_start + p);
    if digits_end == digits_start {
        return None;
    }
    s[..digits_end].parse::<i64>().ok()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Marker line opening a block (matched on the trimmed line). Optional -
    /// omit to anchor the block at the start of the file.
    #[serde(default)]
    start: Option<String>,
    /// Marker line closing a block. Optional - omit to run the block to EOF.
    #[serde(default)]
    end: Option<String>,
    /// Comparator used to order entries: lexical (default), lexical-ci, or
    /// numeric.
    #[serde(default)]
    #[schemars(extend("default" = "lexical"))]
    comparator: Comparator,
    /// When true, also forbid duplicate (equal) entries within a block.
    #[serde(default)]
    unique: bool,
    /// Regex; when set, only lines inside a block matching it are sortable
    /// entries (others, such as comments or group headers, pass through). The
    /// sectioned / keep-sorted-subset shape.
    #[serde(default)]
    select: Option<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct OrderedBlockRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    start: Option<String>,
    end: Option<String>,
    comparator: Comparator,
    unique: bool,
    select: Option<Regex>,
}

/// In-flight block state while scanning a file.
struct Block {
    start_line: usize,
    prev: Option<String>,
    /// One violation per block: once set, further entries are
    /// skipped until the `end` marker (keeps output actionable).
    reported: bool,
}

impl Rule for OrderedBlockRule {
    alint_core::rule_common_impl!();

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for OrderedBlockRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            // Non-UTF-8 is degenerate for a line-sorted region.
            return Ok(Vec::new());
        };
        let mut violations = Vec::new();
        // With no `start` marker the block is open from line 1 (the
        // markerless whole-file / sort-to-EOF form); otherwise it
        // opens when the `start` line is seen.
        let mut block: Option<Block> = self.start.is_none().then_some(Block {
            start_line: 1,
            prev: None,
            reported: false,
        });

        for (i, raw) in text.lines().enumerate() {
            let line_no = i + 1;
            let trimmed = raw.trim();

            // A `start` line always (re)opens a fresh block, closing any
            // active one — uniform across delimited / start-only /
            // markerless, so a repeated `start` (e.g. start-only mode)
            // delimits sections rather than being treated as an entry.
            if Some(trimmed) == self.start.as_deref() {
                // In fully-delimited mode (both markers set) a fresh
                // `start` before the previous block's `end` means that
                // block was never closed — flag it before reopening.
                // (In start-only / markerless mode a repeated `start` is
                // the intended section delimiter, not an error.)
                if let (Some(b), Some(end)) = (&block, &self.end) {
                    violations.push(self.violation(
                        path,
                        b.start_line,
                        b.start_line,
                        &format!("unclosed ordered_block - no {end:?} line after the start"),
                    ));
                }
                block = Some(Block {
                    start_line: line_no,
                    prev: None,
                    reported: false,
                });
                continue;
            }

            let Some(b) = block.as_mut() else {
                continue; // no active block, and not a `start` line
            };

            if self.end.as_deref() == Some(trimmed) {
                block = None;
                continue;
            }
            // Blank lines inside a block are not entries.
            if trimmed.is_empty() || b.reported {
                continue;
            }
            // With `select:`, only matching lines are sortable
            // entries; non-matching lines (comments, group headers)
            // pass through untouched.
            if self.select.as_ref().is_some_and(|re| !re.is_match(raw)) {
                continue;
            }

            let entry = trimmed.to_string();
            if let Some(prev) = &b.prev {
                let ord = self.comparator.order(&entry, prev);
                if ord == Ordering::Less {
                    violations.push(self.violation(
                        path,
                        line_no,
                        b.start_line,
                        &format!("{entry:?} is out of order (it comes after {prev:?})"),
                    ));
                    b.reported = true;
                } else if self.unique && ord == Ordering::Equal {
                    violations.push(self.violation(
                        path,
                        line_no,
                        b.start_line,
                        &format!("{entry:?} is a duplicate entry"),
                    ));
                    b.reported = true;
                }
            }
            b.prev = Some(entry);
        }

        // A fully-delimited block (both markers set) that opened but
        // never saw its `end` is unclosed. A block with an absent
        // `end` (or no `start` at all) intentionally runs to EOF and
        // is not a violation.
        if let Some(b) = block
            && let (Some(_), Some(end)) = (&self.start, &self.end)
        {
            violations.push(self.violation(
                path,
                b.start_line,
                b.start_line,
                &format!("unclosed ordered_block - no {end:?} line after the start"),
            ));
        }
        Ok(violations)
    }
}

impl OrderedBlockRule {
    fn violation(&self, path: &Path, line: usize, start_line: usize, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("ordered_block (start at line {start_line}): {desc}"));
        Violation::new(msg)
            .with_path(std::sync::Arc::<Path>::from(path))
            .with_location(line, 1)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block requires a `paths` field (the files whose marked blocks to check)",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    // Markers are optional: omit `end` to sort from `start` to EOF,
    // omit both to sort the whole file. When given, a marker must be
    // non-empty, and a configured start/end pair must differ.
    let start = opts.start.map(|s| s.trim().to_string());
    let end = opts.end.map(|s| s.trim().to_string());
    if start.as_deref() == Some("") || end.as_deref() == Some("") {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block `start` / `end` marker, when given, must not be empty",
        ));
    }
    if let (Some(s), Some(e)) = (&start, &end)
        && s == e
    {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block `start` and `end` markers must differ",
        ));
    }
    let select = opts
        .select
        .as_deref()
        .map(|p| {
            Regex::new(p).map_err(|e| {
                Error::rule_config(&spec.id, format!("invalid `select:` regex `{p}`: {e}"))
            })
        })
        .transpose()?;
    Ok(Box::new(OrderedBlockRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        start,
        end,
        comparator: opts.comparator,
        unique: opts.unique,
        select,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(comparator: Comparator, unique: bool) -> OrderedBlockRule {
        OrderedBlockRule {
            id: "t".into(),
            level: Level::Warning,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            start: Some("# keep-sorted start".into()),
            end: Some("# keep-sorted end".into()),
            comparator,
            unique,
            select: None,
        }
    }

    fn markerless_rule(
        start: Option<&str>,
        end: Option<&str>,
        comparator: Comparator,
    ) -> OrderedBlockRule {
        OrderedBlockRule {
            id: "t".into(),
            level: Level::Warning,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            start: start.map(Into::into),
            end: end.map(Into::into),
            comparator,
            unique: false,
            select: None,
        }
    }

    fn eval(r: &OrderedBlockRule, text: &str) -> Vec<Violation> {
        let ctx = Context {
            root: Path::new("/"),
            index: &alint_core::FileIndex::from_entries(Vec::new()),
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate_file(&ctx, Path::new("f.txt"), text.as_bytes())
            .unwrap()
    }

    #[test]
    fn sorted_block_passes() {
        let t = "x\n# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\ny\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }

    #[test]
    fn unsorted_block_fails_once_at_the_offending_line() {
        let t = "# keep-sorted start\nalpha\ncharlie\nbravo\ndelta\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "one violation per block: {v:?}");
        // `bravo` (line 4) is out of order after `charlie`.
        assert_eq!(v[0].line, Some(4));
        assert!(v[0].message.contains("bravo"));
    }

    #[test]
    fn absent_markers_in_delimited_mode_is_silent() {
        // A delimited rule (both markers set) over a file that contains
        // NEITHER marker forms no block -> silent. (Markerless mode is
        // covered by `markerless_sorts_the_whole_file`.)
        let t = "just\nsome\nunsorted\nlines\nz\na\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }

    #[test]
    fn unique_flags_duplicate_only_when_set() {
        let t = "# keep-sorted start\nalpha\nalpha\nbravo\n# keep-sorted end\n";
        // Non-decreasing: a duplicate is fine without `unique`.
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
        let v = eval(&rule(Comparator::Lexical, true), t);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("duplicate"));
    }

    #[test]
    fn lexical_ci_and_numeric_comparators() {
        // Bravo < alpha lexically (uppercase), but ci-sorted.
        let ci = "# keep-sorted start\nalpha\nBravo\ncharlie\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::LexicalCi, false), ci).is_empty());
        // Numeric: "9" before "10" (lexical would flip them).
        let num = "# keep-sorted start\n2\n9\n10\n100\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::Numeric, false), num).is_empty());
        let bad = "# keep-sorted start\n10\n9\n# keep-sorted end\n";
        assert_eq!(eval(&rule(Comparator::Numeric, false), bad).len(), 1);
    }

    #[test]
    fn multiple_blocks_checked_independently() {
        let t = "# keep-sorted start\na\nb\n# keep-sorted end\nmid\n\
                 # keep-sorted start\nz\nq\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "only the 2nd block (z, q) is unsorted: {v:?}");
    }

    #[test]
    fn markerless_sorts_the_whole_file() {
        // No start/end: every line is a sortable entry.
        let sorted = "alpha\nbravo\ncharlie\n";
        assert!(eval(&markerless_rule(None, None, Comparator::Lexical), sorted).is_empty());
        let unsorted = "banana\napple\ncherry\n";
        let v = eval(&markerless_rule(None, None, Comparator::Lexical), unsorted);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].line, Some(2), "apple (line 2) is out of order");
        assert!(v[0].message.contains("apple"));
    }

    #[test]
    fn start_only_sorts_to_eof_and_is_never_unclosed() {
        // `start` but no `end`: sort from the marker to EOF; an open
        // block at EOF is intentional, not an "unclosed" violation.
        let r = markerless_rule(Some("# sorted below"), None, Comparator::Lexical);
        let ok = "preamble\n# sorted below\nalpha\nbravo\n";
        assert!(
            eval(&r, ok).is_empty(),
            "no unclosed at EOF: {:?}",
            eval(&r, ok)
        );
        let bad = "# sorted below\nbravo\nalpha\n";
        let v = eval(&r, bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("alpha"));
    }

    #[test]
    fn start_only_repeated_marker_reopens_a_fresh_block() {
        // C3: a repeated `start` in start-only mode delimits a NEW
        // section (re-opens), it is not flagged as out-of-order data.
        let r = markerless_rule(Some("# S"), None, Comparator::Lexical);
        // Two independently-sorted sections, the marker re-opening each.
        let ok = "# S\nalpha\nbravo\n# S\nyak\nzed\n";
        assert!(eval(&r, ok).is_empty(), "{:?}", eval(&r, ok));
        // Only the second section is unsorted -> exactly one violation,
        // anchored on the second section's start (the marker itself is
        // never reported as an entry).
        let bad = "# S\nalpha\nbravo\n# S\nzed\nyak\n";
        let v = eval(&r, bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("yak"));
        assert!(
            !v.iter().any(|x| x.message.contains("\"# S\"")),
            "marker must not be an entry"
        );
    }

    #[test]
    fn delimited_repeated_start_flags_unclosed() {
        // In fully-delimited mode a 2nd `start` before the `end` means the
        // first block was never closed — flag it, don't silently swallow
        // it. The 2nd block here is properly closed.
        let t = "# keep-sorted start\na\n# keep-sorted start\nb\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "the first unterminated block is flagged: {v:?}");
        assert!(v[0].message.contains("unclosed"), "{}", v[0].message);
    }

    #[test]
    fn end_only_sorts_from_bof_until_the_marker() {
        // No `start`, with `end`: the block opens at BOF and closes at
        // the end marker; lines after it are not entries.
        let r = markerless_rule(None, Some("# end"), Comparator::Lexical);
        let ok = "alpha\nbravo\n# end\nzeta\naardvark\n"; // post-`end` unsorted, ignored
        assert!(eval(&r, ok).is_empty());
        let bad = "bravo\nalpha\n# end\n";
        assert_eq!(eval(&r, bad).len(), 1);
    }

    #[test]
    fn end_only_with_no_marker_present_sorts_to_eof() {
        // No `start`, `end` configured but the marker never appears:
        // the BOF-opened block runs to EOF (no "unclosed" — end is the
        // only marker, and start is absent).
        let r = markerless_rule(None, Some("# end"), Comparator::Lexical);
        assert!(eval(&r, "alpha\nbravo\ncherry\n").is_empty());
        assert_eq!(eval(&r, "cherry\nalpha\n").len(), 1);
    }

    #[test]
    fn empty_and_all_blank_files_are_silent() {
        let r = markerless_rule(None, None, Comparator::Lexical);
        assert!(eval(&r, "").is_empty(), "empty file");
        assert!(eval(&r, "\n\n\n").is_empty(), "all-blank file");
    }

    #[test]
    fn crlf_lines_sort_like_lf() {
        // `str::lines()` strips the trailing `\r`, so CRLF content
        // compares the same as LF.
        let r = markerless_rule(None, None, Comparator::Lexical);
        assert!(eval(&r, "apple\r\nbanana\r\ncherry\r\n").is_empty());
        assert_eq!(eval(&r, "banana\r\napple\r\n").len(), 1);
    }

    #[test]
    fn unclosed_start_is_a_violation() {
        let t = "before\n# keep-sorted start\nalpha\nbravo\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("unclosed"));
        assert_eq!(v[0].line, Some(2));
    }

    #[test]
    fn blank_lines_inside_a_block_are_ignored() {
        let t = "# keep-sorted start\nalpha\n\nbravo\n\ncharlie\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }

    #[test]
    fn select_sorts_only_matching_lines() {
        let mut r = rule(Comparator::Lexical, false);
        // Only `require '…'` lines must be sorted; other lines pass.
        r.select = Some(Regex::new(r"^\s*require ").unwrap());
        // The `require` lines are in order; the interleaved comments
        // and the out-of-order `gem` line are ignored.
        let ok = "# keep-sorted start\n\
                  require 'a'\n# a comment\nrequire 'b'\ngem 'z'\nrequire 'c'\n\
                  # keep-sorted end\n";
        assert!(eval(&r, ok).is_empty(), "{:?}", eval(&r, ok));
        // Out-of-order `require` lines fire even with non-require
        // lines interleaved.
        let bad = "# keep-sorted start\nrequire 'c'\ngem 'z'\nrequire 'a'\n# keep-sorted end\n";
        let v = eval(&r, bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("require 'a'"));
    }

    #[test]
    fn build_requires_paths() {
        use crate::test_support::spec_yaml;
        let no_paths = "id: t\nkind: ordered_block\nlevel: error\n";
        assert!(build(&spec_yaml(no_paths)).is_err(), "paths is required");
    }

    #[test]
    fn build_rejects_an_empty_marker() {
        use crate::test_support::spec_yaml;
        let bad = "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: ''\nlevel: error\n";
        let err = build(&spec_yaml(bad)).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "{err}");
    }

    #[test]
    fn build_rejects_identical_start_and_end_markers() {
        use crate::test_support::spec_yaml;
        let bad =
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: SAME\nend: SAME\nlevel: error\n";
        let err = build(&spec_yaml(bad)).unwrap_err();
        assert!(err.to_string().contains("must differ"), "{err}");
    }

    #[test]
    fn build_rejects_invalid_select_regex() {
        use crate::test_support::spec_yaml;
        let bad = "id: t\nkind: ordered_block\npaths: [\"x\"]\nselect: '(unclosed'\nlevel: error\n";
        let err = build(&spec_yaml(bad)).unwrap_err();
        assert!(err.to_string().contains("select"), "{err}");
    }
}
