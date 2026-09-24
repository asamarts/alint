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
    Applicability, Context, Error, FixContext, FixEdit, FixOutcome, FixSpec, Fixer, Level,
    PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use regex::Regex;
use serde::Deserialize;

/// `baseline_key` prefix marking `ordered_block`'s ONE non-`sort`-fixable
/// violation: an unclosed block (a `start` with no `end`). `sort` reorders
/// entries but cannot invent a missing `end` marker, so the fixer's
/// [`can_fix`](Fixer::can_fix) returns `false` for a violation carrying this
/// key, and [`fix_edit`](Fixer::fix_edit) declines it -- keeping `check`'s
/// per-violation `is_fixable` tag honest. Entry violations (out-of-order /
/// duplicate) carry NO key (their fingerprint stays the offending line, so
/// making the rule fixable does not un-grandfather existing baselines); the
/// sentinel is set only on the rare unclosed finding. `\0`-delimited with the
/// block's start line so two unclosed blocks in one file stay distinct (F4).
const UNCLOSED_KEY_PREFIX: &str = "ordered_block\u{0}unclosed\u{0}";

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
    fixer: Option<OrderedBlockSortFixer>,
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

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

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
                    violations.push(
                        self.violation(
                            path,
                            b.start_line,
                            b.start_line,
                            &format!("unclosed ordered_block - no {end:?} line after the start"),
                        )
                        .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}{}", b.start_line)),
                    );
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
            if b.reported {
                continue;
            }
            // Blank lines, and (with `select:`) non-matching lines such as
            // comments or group headers, are not sortable entries. Shared with
            // the `sort` fixer's block scan via `is_entry_line` so check and fix
            // never disagree on what an entry is.
            if !is_entry_line(self.select.as_ref(), raw, trimmed) {
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
            violations.push(
                self.violation(
                    path,
                    b.start_line,
                    b.start_line,
                    &format!("unclosed ordered_block - no {end:?} line after the start"),
                )
                .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}{}", b.start_line)),
            );
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

/// Whether a line inside a block is a sortable ENTRY: non-blank, and (when the
/// rule sets `select:`) matching that regex. The check and the `sort` fixer's
/// [`scan_blocks`] both route their entry test through this, so a file's entry
/// set is identical for detection and repair -- the correlation invariant a
/// located/whole-file fixer must hold ([[project_alint-autofix-located-fixer-correlation]]).
fn is_entry_line(select: Option<&Regex>, raw: &str, trimmed: &str) -> bool {
    !trimmed.is_empty() && select.is_none_or(|re| re.is_match(raw))
}

/// Enumerate each block's sortable entries as 0-based indices into `lines`
/// (raw lines with no terminators -- exactly what `str::lines()` yields, so this
/// mirrors the check's own scan). A `start` line (re)opens a block; an `end`
/// line closes it; markerless mode (`start` = `None`) is one block from line 1
/// to EOF. An UNCLOSED block's entries are still yielded: its extent (to the
/// next `start` or EOF) is the same run the check reports out-of-order against,
/// so `sort` reorders exactly what `check` flags; the separate "unclosed"
/// structural finding is the one thing `sort` cannot repair (it cannot invent
/// an `end` marker), and `can_fix` declines that via the sentinel key.
fn scan_blocks(
    start: Option<&str>,
    end: Option<&str>,
    select: Option<&Regex>,
    lines: &[&str],
) -> Vec<Vec<usize>> {
    let mut blocks: Vec<Vec<usize>> = Vec::new();
    // Markerless: one block open from the first line.
    let mut current: Option<Vec<usize>> = start.is_none().then(Vec::new);
    for (i, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();
        if start == Some(trimmed) {
            // A `start` always (re)opens, flushing any active block first --
            // uniform across delimited / start-only / markerless, matching the
            // check's `Some(trimmed) == self.start` reopen.
            if let Some(entries) = current.take() {
                blocks.push(entries);
            }
            current = Some(Vec::new());
            continue;
        }
        if current.is_none() {
            continue; // outside any block, and not a `start` line
        }
        if end == Some(trimmed) {
            blocks.push(current.take().expect("current is Some (checked above)"));
            continue;
        }
        if is_entry_line(select, raw, trimmed) {
            current
                .as_mut()
                .expect("current is Some (checked above)")
                .push(i);
        }
    }
    if let Some(entries) = current.take() {
        blocks.push(entries);
    }
    blocks
}

/// Split `text` into `(body, ending)` slots. `body` is the line without its
/// terminator (matching `str::lines()`, so a lone trailing `\r` at EOF stays in
/// the body); `ending` is `"\n"`, `"\r\n"`, or `""` for a final line with no
/// terminator. Concatenating `body + ending` over every slot reproduces `text`
/// byte-for-byte, so a `sort` that only PERMUTES bodies while keeping each
/// slot's ending preserves every line's exact terminator and the file's
/// trailing-newline state (LF stays LF, CRLF stays CRLF, no-final-newline
/// stays).
fn split_lines(text: &str) -> Vec<(&str, &'static str)> {
    let mut slots = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(i) = rest.find('\n') else {
            // The final line has no terminator.
            slots.push((rest, ""));
            break;
        };
        let before = &rest[..i];
        let (body, ending) = match before.strip_suffix('\r') {
            Some(b) => (b, "\r\n"),
            None => (before, "\n"),
        };
        slots.push((body, ending));
        rest = &rest[i + 1..];
    }
    slots
}

/// Whether `entries` are ordered under `comparator`: non-decreasing, or strictly
/// increasing when `unique`. The post-sort convergence guard -- a comparator
/// that is not a strict weak order (a pathological mixed-`numeric` block) could
/// leave `sort_by` output with an out-of-order adjacent pair that `check` would
/// still flag; the fixer verifies this before committing a block (W4
/// verify-per-edit) and skips the block otherwise rather than writing a
/// non-converging file.
fn is_monotonic(comparator: Comparator, unique: bool, entries: &[&str]) -> bool {
    entries
        .windows(2)
        .all(|w| match comparator.order(w[1].trim(), w[0].trim()) {
            Ordering::Greater => true,
            Ordering::Equal => !unique,
            Ordering::Less => false,
        })
}

/// The `sort` fix for `ordered_block`: a whole-file rewrite that re-sorts every
/// marked block's entries under the host rule's comparator (dropping duplicates
/// when the rule sets `unique`), preserving markers, blank lines and
/// `select`-excluded lines in place, and every line's terminator. Idempotent
/// (a sorted file rewrites to itself -> the engine's per-violation `apply` loop
/// converges), and behavior-preserving (a keep-sorted block is order-independent),
/// hence `Safe` by default; a per-rule `applicability:` retunes it.
#[derive(Debug, Clone)]
struct OrderedBlockSortFixer {
    start: Option<String>,
    end: Option<String>,
    comparator: Comparator,
    unique: bool,
    select: Option<Regex>,
    applicability: Applicability,
}

impl OrderedBlockSortFixer {
    /// The sorted form of `text`, or `None` when it is already sorted (nothing
    /// to write). Reorders each block's entry bodies under the comparator,
    /// removing `unique` duplicates' slots; non-entry lines and terminators stay.
    fn sorted(&self, text: &str) -> Option<String> {
        let slots = split_lines(text);
        let bodies: Vec<&str> = slots.iter().map(|(b, _)| *b).collect();
        let blocks = scan_blocks(
            self.start.as_deref(),
            self.end.as_deref(),
            self.select.as_ref(),
            &bodies,
        );
        // Per slot: the replacement body (default = unchanged) and whether the
        // slot is DELETED (a `unique` duplicate collapsed away).
        let mut new_body: Vec<&str> = bodies.clone();
        let mut deleted = vec![false; slots.len()];
        let mut changed = false;
        for entry_idxs in &blocks {
            if entry_idxs.is_empty() {
                continue;
            }
            let mut sorted: Vec<&str> = entry_idxs.iter().map(|&i| bodies[i]).collect();
            sorted.sort_by(|a, b| self.comparator.order(a.trim(), b.trim()));
            if self.unique {
                sorted
                    .dedup_by(|a, b| self.comparator.order(a.trim(), b.trim()) == Ordering::Equal);
            }
            // Never write a block the comparator could not fully order (see
            // `is_monotonic`): leave it, so `fix` never emits a file `check`
            // would still flag.
            if !is_monotonic(self.comparator, self.unique, &sorted) {
                continue;
            }
            for (pos, &slot) in entry_idxs.iter().enumerate() {
                if let Some(&body) = sorted.get(pos) {
                    if new_body[slot] != body {
                        changed = true;
                    }
                    new_body[slot] = body;
                } else {
                    // `unique` collapsed the block: the surplus entry slots are
                    // removed, so the block shrinks by exactly its duplicates.
                    deleted[slot] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return None;
        }
        let mut out = String::with_capacity(text.len());
        for (i, (_, ending)) in slots.iter().enumerate() {
            if deleted[i] {
                continue;
            }
            out.push_str(new_body[i]);
            out.push_str(ending);
        }
        Some(out)
    }
}

impl Fixer for OrderedBlockSortFixer {
    fn describe(&self) -> String {
        if self.unique {
            "sort each ordered_block's entries and drop duplicates".to_string()
        } else {
            "sort each ordered_block's entries".to_string()
        }
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, violation: &Violation) -> bool {
        // Every ordered_block violation is sort-fixable EXCEPT the structural
        // "unclosed block" finding: `sort` reorders entries but cannot add a
        // missing `end` marker. That one carries the sentinel key; all entry
        // (out-of-order / duplicate) findings are key-less and fixable.
        !violation
            .baseline_key
            .as_deref()
            .is_some_and(|k| k.starts_with(UNCLOSED_KEY_PREFIX))
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
            // The detector also skips non-UTF-8 (no violation), so this is
            // defensive: a fix is never dispatched for such a file.
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot sort",
                path.display()
            )));
        };
        let Some(sorted) = self.sorted(text) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} already sorted",
                path.display()
            )));
        };
        // Dry-run AFTER the read + sort, so a preview matches the real run.
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would sort ordered_block entries in {}",
                path.display()
            )));
        }
        ctx.commit_write(&abs, sorted.as_bytes())
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "sorted ordered_block entries in {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        // Decline the unclosed-block finding (see `can_fix`): the whole-file
        // sort would fix OTHER blocks, but this violation is not sort-repairable,
        // so the LSP must not offer it an "Apply fix".
        if !self.can_fix(violation) {
            return None;
        }
        let path = violation.path.as_deref()?;
        let text = std::str::from_utf8(bytes).ok()?;
        let sorted = self.sorted(text)?;
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: sorted.into_bytes(),
        })
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
    // `sort` reuses the rule's own `start`/`end`/`comparator`/`unique`/`select`
    // (cloned so the rule keeps ownership); the spec adds only the tier override.
    let fixer = match &spec.fix {
        Some(FixSpec::Sort { sort }) => Some(OrderedBlockSortFixer {
            start: start.clone(),
            end: end.clone(),
            comparator: opts.comparator,
            unique: opts.unique,
            select: select.clone(),
            applicability: sort.applicability.unwrap_or(Applicability::Safe),
        }),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with ordered_block (only `sort` is)",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
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
        fixer,
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
            fixer: None,
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
            fixer: None,
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

    // ----- the `sort` fix -----------------------------------------------

    fn sort_fixer(comparator: Comparator, unique: bool) -> OrderedBlockSortFixer {
        OrderedBlockSortFixer {
            start: Some("# keep-sorted start".into()),
            end: Some("# keep-sorted end".into()),
            comparator,
            unique,
            select: None,
            applicability: Applicability::Safe,
        }
    }

    fn markerless_sort_fixer(
        start: Option<&str>,
        end: Option<&str>,
        comparator: Comparator,
        unique: bool,
        select: Option<&str>,
    ) -> OrderedBlockSortFixer {
        OrderedBlockSortFixer {
            start: start.map(Into::into),
            end: end.map(Into::into),
            comparator,
            unique,
            select: select.map(|p| Regex::new(p).unwrap()),
            applicability: Applicability::Safe,
        }
    }

    #[test]
    fn split_lines_round_trips_every_ending() {
        for text in [
            "",
            "a",
            "a\n",
            "a\nb",
            "a\nb\n",
            "a\r\nb\r\n",
            "a\r\nb\n",     // mixed
            "\n\n",         // blank lines
            "a\r",          // lone trailing CR (no LF) stays in body
            "# start\nx\n", // markers
        ] {
            let rebuilt: String = split_lines(text)
                .iter()
                .flat_map(|(b, e)| [*b, *e])
                .collect();
            assert_eq!(rebuilt, text, "round-trip failed for {text:?}");
        }
    }

    #[test]
    fn split_lines_bodies_match_str_lines() {
        for text in ["a\nb\n", "a\nb", "\n", "", "a\r\nb", "x\r"] {
            let bodies: Vec<&str> = split_lines(text).iter().map(|(b, _)| *b).collect();
            let stdlib: Vec<&str> = text.lines().collect();
            assert_eq!(
                bodies, stdlib,
                "bodies diverge from str::lines() for {text:?}"
            );
        }
    }

    #[test]
    fn sorted_reorders_a_delimited_block_leaving_surroundings() {
        let t = "head\n# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\ntail\n";
        let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
        assert_eq!(
            out,
            "head\n# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\ntail\n"
        );
    }

    #[test]
    fn sorted_returns_none_when_already_sorted() {
        let t = "# keep-sorted start\nalpha\nbravo\n# keep-sorted end\n";
        assert!(sort_fixer(Comparator::Lexical, false).sorted(t).is_none());
    }

    #[test]
    fn sorted_is_idempotent() {
        let t = "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n";
        let once = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
        // A second pass finds nothing to do (the engine's per-violation apply loop
        // relies on this to converge).
        assert!(
            sort_fixer(Comparator::Lexical, false)
                .sorted(&once)
                .is_none()
        );
    }

    #[test]
    fn sorted_preserves_crlf_per_line() {
        let t = "# keep-sorted start\r\ncharlie\r\nalpha\r\n# keep-sorted end\r\n";
        let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
        assert_eq!(
            out,
            "# keep-sorted start\r\nalpha\r\ncharlie\r\n# keep-sorted end\r\n"
        );
    }

    #[test]
    fn sorted_preserves_missing_final_newline() {
        // Markerless whole-file sort where the last entry has no terminator: the
        // file must still end without a newline (the ending is positional).
        let out = markerless_sort_fixer(None, None, Comparator::Lexical, false, None)
            .sorted("charlie\nalpha\nbravo")
            .unwrap();
        assert_eq!(out, "alpha\nbravo\ncharlie");
    }

    #[test]
    fn sorted_numeric_comparator() {
        let t = "# keep-sorted start\n10\n2\n1\n# keep-sorted end\n";
        let out = sort_fixer(Comparator::Numeric, false).sorted(t).unwrap();
        assert_eq!(out, "# keep-sorted start\n1\n2\n10\n# keep-sorted end\n");
    }

    #[test]
    fn sorted_unique_drops_duplicate_slots() {
        let t = "# keep-sorted start\nbravo\nalpha\nbravo\n# keep-sorted end\n";
        let out = sort_fixer(Comparator::Lexical, true).sorted(t).unwrap();
        // One `bravo` remains; the block shrank by exactly the duplicate.
        assert_eq!(
            out,
            "# keep-sorted start\nalpha\nbravo\n# keep-sorted end\n"
        );
    }

    #[test]
    fn sorted_select_reorders_only_matching_lines_in_place() {
        // Non-matching lines (the `# group` comment) stay at their slot; only
        // `dep:`-prefixed entries are reordered around them.
        let f = markerless_sort_fixer(
            Some("# start"),
            Some("# end"),
            Comparator::Lexical,
            false,
            Some("^dep:"),
        );
        let t = "# start\ndep:z\n# group comment\ndep:a\n# end\n";
        let out = f.sorted(t).unwrap();
        assert_eq!(out, "# start\ndep:a\n# group comment\ndep:z\n# end\n");
    }

    #[test]
    fn sorted_handles_multiple_blocks_each_independently() {
        let t = "# keep-sorted start\nb\na\n# keep-sorted end\nx\n# keep-sorted start\nd\nc\n# keep-sorted end\n";
        let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
        assert_eq!(
            out,
            "# keep-sorted start\na\nb\n# keep-sorted end\nx\n# keep-sorted start\nc\nd\n# keep-sorted end\n"
        );
    }

    #[test]
    fn sort_fix_correlates_with_the_check() {
        // The load-bearing invariant: `sort` resolves exactly the entry
        // violations `check` flags. Sort every unsorted fixture, then re-check
        // and assert no entry (non-`unclosed`) violation survives.
        for t in [
            "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n",
            "# keep-sorted start\nz\ny\nx\nw\n# keep-sorted end\n",
            "head\n# keep-sorted start\nb\na\n# keep-sorted end\nmid\n# keep-sorted start\nd\nc\n# keep-sorted end\n",
        ] {
            let rule = rule(Comparator::Lexical, false);
            assert!(!eval(&rule, t).is_empty(), "fixture should flag: {t:?}");
            let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
            let after: Vec<_> = eval(&rule, &out)
                .into_iter()
                .filter(|v| {
                    v.baseline_key
                        .as_deref()
                        .is_none_or(|k| !k.starts_with(UNCLOSED_KEY_PREFIX))
                })
                .collect();
            assert!(
                after.is_empty(),
                "entry violations survived sort: {after:?}"
            );
        }
    }

    #[test]
    fn sort_fix_unique_correlates_with_the_check() {
        let rule = rule(Comparator::Lexical, true);
        let t = "# keep-sorted start\nbravo\nalpha\nbravo\n# keep-sorted end\n";
        assert!(!eval(&rule, t).is_empty());
        let out = sort_fixer(Comparator::Lexical, true).sorted(t).unwrap();
        assert!(eval(&rule, &out).is_empty(), "dup/order survived: {out:?}");
    }

    #[test]
    fn can_fix_declines_only_the_unclosed_finding() {
        let f = sort_fixer(Comparator::Lexical, false);
        let entry = Violation::new("x is out of order");
        assert!(f.can_fix(&entry), "an entry violation is sort-fixable");
        let unclosed = Violation::new("unclosed ordered_block")
            .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}7"));
        assert!(
            !f.can_fix(&unclosed),
            "the unclosed finding is not sort-fixable"
        );
    }

    #[test]
    fn check_marks_the_unclosed_finding_with_the_sentinel_key() {
        // A start with no end: the check emits the unclosed finding carrying the
        // sentinel, so the engine tags it non-fixable.
        let t = "# keep-sorted start\nalpha\nbravo\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].baseline_key
                .as_deref()
                .is_some_and(|k| k.starts_with(UNCLOSED_KEY_PREFIX)),
            "unclosed finding must carry the sentinel key: {:?}",
            v[0].baseline_key
        );
    }

    #[test]
    fn entry_violation_keeps_the_default_fingerprint_no_key() {
        // Out-of-order entries stay key-less so making the rule fixable does not
        // un-grandfather existing baselines (their fingerprint stays the line).
        let t = "# keep-sorted start\nbravo\nalpha\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].baseline_key.is_none(),
            "entry finding must be key-less"
        );
    }

    #[test]
    fn apply_sorts_the_file_on_disk() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let t = "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n";
        std::fs::write(tmp.path().join("f.txt"), t).unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = sort_fixer(Comparator::Lexical, false)
            .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
            "# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\n"
        );
    }

    #[test]
    fn apply_dry_run_leaves_disk_untouched() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let t = "# keep-sorted start\nb\na\n# keep-sorted end\n";
        std::fs::write(tmp.path().join("f.txt"), t).unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: true,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = sort_fixer(Comparator::Lexical, false)
            .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
            .unwrap();
        assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
            t
        );
    }

    #[test]
    fn apply_skips_an_already_sorted_file() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let t = "# keep-sorted start\na\nb\n# keep-sorted end\n";
        std::fs::write(tmp.path().join("f.txt"), t).unwrap();
        let ctx = FixContext {
            root: tmp.path(),
            dry_run: false,
            fix_size_limit: None,
            allow_out_of_root: false,
            compose: None,
            stage_ops: None,
        };
        let outcome = sort_fixer(Comparator::Lexical, false)
            .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
            .unwrap();
        assert!(
            matches!(&outcome, FixOutcome::Skipped(s) if s.contains("already sorted")),
            "{outcome:?}"
        );
    }

    #[test]
    fn fix_edit_emits_set_content_and_declines_unclosed() {
        let f = sort_fixer(Comparator::Lexical, false);
        let bytes = b"# keep-sorted start\nb\na\n# keep-sorted end\n";
        let edit = f
            .fix_edit(
                &Violation::new("x").with_path(Path::new("f.txt")),
                bytes,
                Path::new("/"),
            )
            .expect("an entry violation yields an edit");
        match edit {
            FixEdit::SetContent { content, .. } => assert_eq!(
                content,
                b"# keep-sorted start\na\nb\n# keep-sorted end\n".to_vec()
            ),
            other => panic!("expected SetContent, got {other:?}"),
        }
        // The unclosed finding is declined even though other blocks could sort.
        let unclosed = Violation::new("unclosed")
            .with_path(Path::new("f.txt"))
            .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}1"));
        assert!(f.fix_edit(&unclosed, bytes, Path::new("/")).is_none());
    }

    #[test]
    fn build_wires_the_sort_fixer() {
        use crate::test_support::spec_yaml;
        let yaml = "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nlevel: error\nfix: { sort: {} }\n";
        let rule = build(&spec_yaml(yaml)).unwrap();
        assert!(rule.fixer().is_some(), "sort fix should wire a fixer");
    }

    #[test]
    fn build_rejects_an_incompatible_fix_op() {
        use crate::test_support::spec_yaml;
        let yaml =
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nlevel: error\nfix: { file_remove: {} }\n";
        let err = build(&spec_yaml(yaml)).unwrap_err();
        assert!(err.to_string().contains("not compatible"), "{err}");
    }
}
