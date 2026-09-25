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

/// `baseline_key` prefixes for `ordered_block`'s findings, set ONLY when the
/// rule is fixable (a `sort` fix is declared). Two reasons a fixable
/// `ordered_block` MUST key every finding per block (F4, and a hard debug panic
/// otherwise): (1) the fixpoint merge keys fixable findings by `violation_key`,
/// which for a key-LESS path-bearing finding collapses to `(rule_id, path)` --
/// so two out-of-order blocks in ONE file would collide and the merge would drop
/// one (`engine.rs` F4 tripwire); (2) `can_fix` must tell the sortable entry
/// findings (`ENTRY`) apart from the one thing `sort` cannot repair, an unclosed
/// block with no `end` (`UNCLOSED`), so `check` never advertises the latter
/// fixable. Both are `\0`-delimited with the block's 0-based ORDINAL (not its
/// line -- `baseline.rs` never hashes line numbers, so inserting a line above a
/// block does not re-churn its key), unique per block and stable across fixpoint
/// passes. Keyed ONLY when a fixer is attached, so a check-only `ordered_block`
/// keeps its offending-line fingerprint and adding a `sort` fix is the only
/// thing that re-baselines it (an intentional config change, not silent drift).
const UNCLOSED_KEY_PREFIX: &str = "ordered_block\u{0}unclosed\u{0}";
const ENTRY_KEY_PREFIX: &str = "ordered_block\u{0}entry\u{0}";
/// `baseline_key` prefix for a "required line missing" finding (a `require:` line
/// absent from the block), FOLLOWED by the required line. Distinct from the
/// ENTRY/UNCLOSED (sortedness) findings so `can_fix` can route it: only the
/// `insert_line` fix repairs it (by splicing the line at its sorted position);
/// `sort` cannot (it reorders existing lines, it does not add one). Per-require-
/// line (F4), and keyed only when a fixer is attached (no check-only churn).
const REQUIRE_KEY_PREFIX: &str = "ordered_block\u{0}require\u{0}";

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Comparator {
    /// Rust `str` `Ord` - byte-wise over the UTF-8.
    #[default]
    Lexical,
    /// ASCII-case-insensitive lexical.
    LexicalCi,
    /// Leading-integer order; entries without a leading integer (or with one too
    /// large even for `i128` -- 39+ digits) fall back to `lexical` so a mixed
    /// block degrades predictably rather than panicking.
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

/// The leading (optionally negative) integer of `s`, or `None` when it doesn't
/// start with one. Parsed as `i128`, so u64-range identifiers (Discord/Twitter
/// snowflakes ~1.8e19, nanosecond timestamps) and u128 values sort NUMERICALLY
/// rather than falling back to a wrong byte-wise order; only a 39+-digit integer
/// (over `i128::MAX`) still degrades to lexical.
fn leading_int(s: &str) -> Option<i128> {
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
    s[..digits_end].parse::<i128>().ok()
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
    /// Exact lines that must be PRESENT in the block (in addition to it being
    /// sorted). A missing one is repaired by the `insert_line` fix, which splices
    /// it at its sorted position. Currently supported only for a MARKERLESS rule
    /// (the whole file is one sorted list -- the `CODEOWNERS` / allow-list shape).
    #[serde(default)]
    require: Option<Vec<String>>,
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
    /// Exact lines the block must contain (markerless only); a missing one is an
    /// `insert_line`-fixable finding. Empty when `require:` is unset.
    require: Vec<String>,
    /// The `sort` or `insert_line` fixer (a rule declares at most one fix op), or
    /// `None` for a check-only rule.
    fixer: Option<Box<dyn Fixer>>,
}

/// In-flight block state while scanning a file.
struct Block {
    start_line: usize,
    /// 0-based ordinal of this block within the file, used ONLY for a fixable
    /// rule's per-finding `baseline_key`. An ordinal (not `start_line`) keeps the
    /// key line-number-free -- inserting a line above a block does not re-churn
    /// its fingerprint (`baseline.rs`: line/column numbers are never hashed) -- and
    /// stable across fixpoint passes (a `sort` never adds or removes a block).
    index: usize,
    prev: Option<String>,
    /// One violation per block: once set, further entries are
    /// skipped until the `end` marker (keeps output actionable).
    reported: bool,
}

impl Rule for OrderedBlockRule {
    alint_core::rule_common_impl!();

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_deref()
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
        // Assigns each block its 0-based ordinal as it opens (for the fixable
        // rule's per-block `baseline_key`); the markerless block-1 is ordinal 0.
        let mut next_index = 0usize;
        let mut new_index = || {
            let i = next_index;
            next_index += 1;
            i
        };
        // With no `start` marker the block is open from line 1 (the
        // markerless whole-file / sort-to-EOF form); otherwise it
        // opens when the `start` line is seen.
        let mut block: Option<Block> = self.start.is_none().then(|| Block {
            start_line: 1,
            index: new_index(),
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
                    violations.push(self.keyed_violation(
                        path,
                        b.start_line,
                        b.start_line,
                        b.index,
                        UNCLOSED_KEY_PREFIX,
                        &format!("unclosed ordered_block - no {end:?} line after the start"),
                    ));
                }
                block = Some(Block {
                    start_line: line_no,
                    index: new_index(),
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
                    violations.push(self.keyed_violation(
                        path,
                        line_no,
                        b.start_line,
                        b.index,
                        ENTRY_KEY_PREFIX,
                        &format!("{entry:?} is out of order (it comes after {prev:?})"),
                    ));
                    b.reported = true;
                } else if self.unique && ord == Ordering::Equal {
                    violations.push(self.keyed_violation(
                        path,
                        line_no,
                        b.start_line,
                        b.index,
                        ENTRY_KEY_PREFIX,
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
            violations.push(self.keyed_violation(
                path,
                b.start_line,
                b.start_line,
                b.index,
                UNCLOSED_KEY_PREFIX,
                &format!("unclosed ordered_block - no {end:?} line after the start"),
            ));
        }

        violations.extend(self.required_line_findings(text, path));
        Ok(violations)
    }
}

impl OrderedBlockRule {
    /// Findings for each `require:` line missing from the (markerless) block. A
    /// line is present when its trimmed form is an entry (matching the sort entry
    /// comparison). The `insert_line` fix repairs each one by splicing it at its
    /// sorted position. Keyed per require-line (F4) so distinct missing lines never
    /// collide, and only when a fixer is attached (a check-only rule keeps
    /// line-content fingerprints).
    fn required_line_findings(&self, text: &str, path: &Path) -> Vec<Violation> {
        if self.require.is_empty() {
            return Vec::new();
        }
        let present: std::collections::HashSet<&str> = text
            .lines()
            .filter(|raw| is_entry_line(self.select.as_ref(), raw, raw.trim()))
            .map(str::trim)
            .collect();
        let mut out = Vec::new();
        for req in &self.require {
            let req_trimmed = req.trim();
            if present.contains(req_trimmed) {
                continue;
            }
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| format!("required line {req:?} is missing"));
            let mut v = Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(1, 1);
            if self.fixer.is_some() {
                v = v.with_baseline_key(format!("{REQUIRE_KEY_PREFIX}{req_trimmed}"));
            }
            out.push(v);
        }
        out
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

    /// A finding tagged, WHEN this rule is fixable, with a per-block
    /// `baseline_key` (`prefix` + the block's 0-based `block_index`) so the
    /// fixpoint merge keeps distinct blocks' findings apart (F4) and `can_fix` can
    /// classify them. The ordinal (not `start_line`) keeps the key free of line
    /// numbers. Key-less when no fixer is attached, so a check-only rule's baseline
    /// fingerprints are unchanged (see [`UNCLOSED_KEY_PREFIX`]).
    fn keyed_violation(
        &self,
        path: &Path,
        line: usize,
        start_line: usize,
        block_index: usize,
        key_prefix: &str,
        desc: &str,
    ) -> Violation {
        let v = self.violation(path, line, start_line, desc);
        if self.fixer.is_some() {
            v.with_baseline_key(format!("{key_prefix}{block_index}"))
        } else {
            v
        }
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
/// increasing when `unique`. A DEFENSE-IN-DEPTH post-sort guard: all THREE
/// built-in comparators are total orders (verified: `numeric`'s `i64`-then-tie
/// fallback is a strict weak order over any input), so `sort_by` is always
/// monotonic and this never skips a block today. It exists only so that a FUTURE
/// non-strict-weak comparator -- whose `sort_by` output could leave an
/// out-of-order adjacent pair `check` would still flag -- has the fixer skip that
/// block (W4 verify-per-edit) rather than write a non-converging file. (Caveat
/// for that hypothetical: if the ONLY block is skipped, `apply` reports
/// `Skipped("already sorted")` while `can_fix` stayed `true` -- a benign
/// over-promise on unreachable-today input; a real non-total comparator would
/// need a more precise skip message.)
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
        // Preserve the file's trailing-newline STATE. If `unique` deleted the last
        // physical line -- which carried no terminator -- the new last surviving
        // line contributes its own `\n` / `\r\n`, spuriously adding a final
        // newline the original lacked (a change beyond sort/dedup that can also
        // conflict with a `final_newline` policy). Strip it back so a
        // no-final-newline file stays that way. (A pure reorder never trips this:
        // the last slot is preserved, so `out` keeps its `""` ending.)
        if !text.ends_with('\n') {
            if let Some(trimmed) = out.strip_suffix("\r\n").or_else(|| out.strip_suffix('\n')) {
                out.truncate(trimmed.len());
            }
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
        // `sort` reorders existing lines, so it repairs an ENTRY finding
        // (out-of-order / duplicate) but NOT the structural UNCLOSED finding (it
        // cannot add an `end` marker) nor a missing-`require:`-line REQUIRE finding
        // (that is `insert_line`'s job -- sort does not ADD a line). Decline both;
        // a key-less finding (never produced when a fixer is attached) defaults
        // fixable.
        !violation.baseline_key.as_deref().is_some_and(|k| {
            k.starts_with(UNCLOSED_KEY_PREFIX) || k.starts_with(REQUIRE_KEY_PREFIX)
        })
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        // GUARD (audit F1): the engine calls `apply` for EVERY violation, gated
        // only on the tier -- NOT on `can_fix`. Without this, an UNCLOSED finding
        // (which `sort` cannot repair) whose `apply` happens to sort OTHER blocks
        // in the file returns `Applied`; the engine then locks its key as applied
        // and drops the unclosed finding's re-detection on the next pass, so `fix`
        // exits 0 leaving the unclosed block on disk. Declining here keeps the
        // unclosed finding honestly reported (skipped), matching `check`.
        if !self.can_fix(violation) {
            return Ok(FixOutcome::Skipped(
                "an unclosed ordered_block (a `start` with no `end`) is not \
                 sort-fixable"
                    .to_string(),
            ));
        }
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

/// The `insert_line` fix for `ordered_block` + `require:` (markerless): splice a
/// missing required line at its SORTED position among the entries, using the
/// rule's comparator. Content-injecting (the required line is ruleset-authored),
/// `Safe` by default (it inserts a user-declared line, like `file_append`). The
/// specific missing line is read from the violation's `baseline_key`.
#[derive(Debug, Clone)]
struct OrderedBlockInsertLineFixer {
    comparator: Comparator,
    select: Option<Regex>,
    applicability: Applicability,
}

impl OrderedBlockInsertLineFixer {
    /// The missing required line carried by a REQUIRE finding, or `None`.
    fn missing_line(violation: &Violation) -> Option<&str> {
        violation
            .baseline_key
            .as_deref()?
            .strip_prefix(REQUIRE_KEY_PREFIX)
    }

    /// The file with `line` spliced at its sorted position among the entries, or
    /// `None` when it is already present (idempotence). Markerless: the whole file
    /// is one sorted list. Preserves every other line's terminator and the file's
    /// trailing-newline state.
    fn inserted(&self, text: &str, line: &str) -> Option<String> {
        let mut slots = split_lines(text);
        // Idempotence: already present as an entry (trimmed-equal)?
        if slots.iter().any(|(body, _)| {
            is_entry_line(self.select.as_ref(), body, body.trim()) && body.trim() == line
        }) {
            return None;
        }
        // The line ending to give the NEW line: the file's first real ending, or
        // LF when the file has none (empty / single unterminated line).
        let ending = slots
            .iter()
            .map(|(_, e)| *e)
            .find(|e| !e.is_empty())
            .unwrap_or("\n");
        // Insert before the first ENTRY strictly greater than `line`; else append.
        let insert_at = slots.iter().position(|(body, _)| {
            is_entry_line(self.select.as_ref(), body, body.trim())
                && self.comparator.order(body.trim(), line) == Ordering::Greater
        });
        match insert_at {
            Some(i) => slots.insert(i, (line, ending)),
            None => {
                // Append. If the last line has no terminator, add one to it and
                // keep the new line bare, preserving the no-final-newline state.
                if slots.last().is_some_and(|(_, e)| e.is_empty()) {
                    let last = slots.len() - 1;
                    slots[last].1 = ending;
                    slots.push((line, ""));
                } else {
                    slots.push((line, ending));
                }
            }
        }
        let mut out = String::with_capacity(text.len() + line.len() + 2);
        for (body, end) in &slots {
            out.push_str(body);
            out.push_str(end);
        }
        Some(out)
    }
}

impl Fixer for OrderedBlockInsertLineFixer {
    fn describe(&self) -> String {
        "insert a missing required line at its sorted position".to_string()
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, violation: &Violation) -> bool {
        // Only a missing-`require:`-line finding is insert_line-fixable; a
        // sortedness / unclosed finding is not (that is `sort`'s / a marker's job).
        violation
            .baseline_key
            .as_deref()
            .is_some_and(|k| k.starts_with(REQUIRE_KEY_PREFIX))
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        // GUARD (audit F1): the engine calls `apply` for every violation regardless
        // of `can_fix`; decline anything but a missing-required-line finding so a
        // whole-file splice never runs for -- and suppresses -- a sortedness finding.
        if !self.can_fix(violation) {
            return Ok(FixOutcome::Skipped(
                "not a missing-required-line finding; insert_line declined".to_string(),
            ));
        }
        let Some(line) = Self::missing_line(violation) else {
            return Ok(FixOutcome::Skipped(
                "finding carries no required line".to_string(),
            ));
        };
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
            return Ok(FixOutcome::Skipped(format!(
                "{} is not UTF-8; cannot insert",
                path.display()
            )));
        };
        let Some(out) = self.inserted(text, line) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} already contains {line:?}",
                path.display()
            )));
        };
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would insert required line {line:?} into {}",
                path.display()
            )));
        }
        ctx.commit_write(&abs, out.as_bytes())
            .map_err(|source| Error::Io {
                path: abs.clone(),
                source,
            })?;
        Ok(FixOutcome::Applied(format!(
            "inserted required line {line:?} into {}",
            path.display()
        )))
    }

    fn fix_edit(&self, violation: &Violation, bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        if !self.can_fix(violation) {
            return None;
        }
        let line = Self::missing_line(violation)?;
        let path = violation.path.as_deref()?;
        let text = std::str::from_utf8(bytes).ok()?;
        let out = self.inserted(text, line)?;
        Some(FixEdit::SetContent {
            path: path.to_path_buf(),
            content: out.into_bytes(),
        })
    }
}

/// Validate + normalize `require:` (exact lines the block must contain). It is
/// MARKERLESS-only for now (with markers a multi-block file makes the insert
/// target ambiguous); list entries compare trimmed, so each required line is
/// trimmed, and an empty one is rejected.
fn parse_require(
    rule_id: &str,
    require: Option<&[String]>,
    has_marker: bool,
) -> Result<Vec<String>> {
    let Some(lines) = require else {
        return Ok(Vec::new());
    };
    if has_marker {
        return Err(Error::rule_config(
            rule_id,
            "ordered_block `require:` is currently supported only for a MARKERLESS rule \
             (omit `start` / `end`); a multi-block insert target is ambiguous",
        ));
    }
    let trimmed: Vec<String> = lines.iter().map(|l| l.trim().to_string()).collect();
    if trimmed.iter().any(String::is_empty) {
        return Err(Error::rule_config(
            rule_id,
            "ordered_block `require:` lines must not be empty",
        ));
    }
    Ok(trimmed)
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
    // DEFAULT TIER depends on `unique`: a pure reorder is behavior-preserving
    // (`Safe`), but `unique` DELETES lines -- and equality is on the COMPARATOR
    // (trimmed / case-folded), so a `unique` dedup can drop a line that is not
    // byte-identical to its survivor (e.g. `Foo` vs `foo` under `lexical-ci`, or
    // `  a` vs `a`). That is silent data loss, so it defaults to `Unsafe` like
    // every other deleting fixer (`file_remove`, `remove_value`, `replace`): a
    // bare `alint fix` SUGGESTS it, `--unsafe-fixes` (or a per-rule
    // `applicability: safe`) applies it. An explicit `applicability:` always wins.
    let require = parse_require(
        &spec.id,
        opts.require.as_deref(),
        start.is_some() || end.is_some(),
    )?;
    // `sort` reindexes the rule's own config; `insert_line` inserts a missing
    // `require:` line at its sorted position. DEFAULT TIER for `sort` depends on
    // `unique`: a pure reorder is behavior-preserving (`Safe`), but `unique` DELETES
    // lines (equality is on the COMPARATOR -- `Foo`/`foo` under `lexical-ci`, `  a`/
    // `a` -- so a dropped line need not be byte-identical), which is silent data
    // loss, so it defaults to `Unsafe` like every other deleting fixer. `insert_line`
    // inserts a user-declared line (`Safe`, like `file_append`). An explicit
    // `applicability:` always wins.
    let fixer: Option<Box<dyn Fixer>> = match &spec.fix {
        Some(FixSpec::Sort { sort }) => Some(Box::new(OrderedBlockSortFixer {
            start: start.clone(),
            end: end.clone(),
            comparator: opts.comparator,
            unique: opts.unique,
            select: select.clone(),
            applicability: sort.applicability.unwrap_or(if opts.unique {
                Applicability::Unsafe
            } else {
                Applicability::Safe
            }),
        })),
        Some(FixSpec::InsertLine { insert_line }) => {
            if require.is_empty() {
                return Err(Error::rule_config(
                    &spec.id,
                    "the `insert_line` fix requires a `require:` list (the exact lines to ensure \
                     present); without it there is nothing to insert",
                ));
            }
            Some(Box::new(OrderedBlockInsertLineFixer {
                comparator: opts.comparator,
                select: select.clone(),
                applicability: insert_line.applicability.unwrap_or(Applicability::Safe),
            }))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with ordered_block (only `sort` and `insert_line` \
                     are)",
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
        require,
        fixer,
    }))
}

#[cfg(test)]
mod tests;
