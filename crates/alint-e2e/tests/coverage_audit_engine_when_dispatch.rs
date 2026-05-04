//! Hard audit: every dispatch path in `alint-core::engine` that
//! evaluates a rule (calls `rule.evaluate(...)` or
//! `run_entry(...)`) must consult `entry.when` (or rely on a
//! helper that does).
//!
//! `when:` is engine-owned — `RuleEntry` carries the parsed
//! `Option<WhenExpr>` and the engine gates dispatch on it. The
//! current dispatch sites are:
//!
//! - cross-file partition (`Engine::run`) — uses the `run_entry`
//!   helper, which consults `entry.when` internally.
//! - fix path (`Engine::fix`) — inlines the `entry.when` check
//!   before calling `entry.rule.evaluate(ctx)`.
//! - `run_entry` helper itself — inline check before calling
//!   `rule.evaluate(ctx)`.
//!
//! The per-file partition (`Engine::run_per_file`) is a separate
//! shape: it dispatches `PerFileRule::evaluate_file`, not
//! `Rule::evaluate`. Its `entry.when` consultation is asserted
//! by a separate sub-test below.
//!
//! A future engine extension (e.g., a "fix-only" path, an LSP
//! single-file re-evaluation path) that adds a new dispatch site
//! and forgets the `entry.when` consultation would silently
//! evaluate disabled rules. This audit catches that at PR time.
//!
//! Implementation: for every line in `engine.rs` that contains a
//! dispatch pattern (`rule.evaluate(`, `entry.rule.evaluate(`,
//! or `run_entry(`), search a sliding window of the preceding 60
//! lines for `entry.when`. If no consultation is found within
//! the window, flag the site. 60 lines is generous enough that a
//! `pick_ctx` + `if let Some(expr) = &entry.when { ... } evaluate`
//! pattern with intervening fact/timing setup easily fits.

use std::fs;
use std::path::Path;

const WINDOW: usize = 60;

#[test]
fn engine_dispatch_sites_consult_entry_when() {
    let engine_src_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("alint-core/src/engine.rs");
    assert!(
        engine_src_path.is_file(),
        "expected alint-core/src/engine.rs at {}",
        engine_src_path.display(),
    );
    let src = fs::read_to_string(&engine_src_path).unwrap();
    let lines: Vec<&str> = src.lines().collect();

    let mut violations: Vec<String> = Vec::new();
    let mut sites_seen = 0usize;
    for (i, line) in lines.iter().enumerate() {
        // Skip comments — a "rule.evaluate(" inside a doc
        // comment isn't a dispatch site.
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("///") {
            continue;
        }
        // Skip function declarations — `fn run_entry(`
        // contains `run_entry(` but isn't a callsite.
        let is_decl = (trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn "))
            && trimmed.contains('(');
        if is_decl {
            continue;
        }
        let is_dispatch = line.contains("entry.rule.evaluate(")
            || line.contains("rule.evaluate(")
            || line.contains("run_entry(");
        if !is_dispatch {
            continue;
        }
        // Skip the dispatch site INSIDE `run_entry` itself —
        // it IS the canonical helper that does the
        // consultation, not a caller of one.
        // Check: is the `run_entry` definition above us within
        // 30 lines, before any other `fn ` declaration?
        let inside_run_entry = within_run_entry(&lines, i);
        if inside_run_entry {
            // run_entry must consult entry.when itself — verify.
            let consults = window_contains(&lines, i, WINDOW, "entry.when");
            if !consults {
                violations.push(format!(
                    "engine.rs line {}: `run_entry` calls `rule.evaluate` but \
                     does not consult `entry.when` in the preceding {WINDOW} lines",
                    i + 1,
                ));
            }
            sites_seen += 1;
            continue;
        }

        // For every other dispatch site, the surrounding code
        // must either (a) consult `entry.when` directly within
        // WINDOW lines, OR (b) call `run_entry` (which we've
        // verified consults `entry.when`).
        let consults_inline = window_contains(&lines, i, WINDOW, "entry.when");
        let routes_through_run_entry = line.contains("run_entry(");
        if !consults_inline && !routes_through_run_entry {
            violations.push(format!(
                "engine.rs line {}: `{}` is a rule-dispatch site but no \
                 `entry.when` consultation found in the preceding {WINDOW} lines, \
                 and the line does not delegate to `run_entry`. A rule whose \
                 `when:` evaluates to false would still run from this site. \
                 Either add `if let Some(expr) = &entry.when {{ ... }}` before \
                 the dispatch, or route through `run_entry`.",
                i + 1,
                line.trim(),
            ));
        }
        sites_seen += 1;
    }
    assert!(
        sites_seen >= 3,
        "expected at least 3 `Rule::evaluate`/`run_entry` dispatch sites in \
         engine.rs (cross-file partition, fix path, `run_entry` helper); found \
         {sites_seen}. engine.rs structure changed — re-verify the audit's \
         window matches the new layout. Per-file partition uses \
         `PerFileRule::evaluate_file` so it doesn't show up in this count; see \
         `run_per_file_consults_entry_when` for that path's coverage.",
    );
    assert!(
        violations.is_empty(),
        "engine when-dispatch wiring incomplete:\n  - {}",
        violations.join("\n  - "),
    );
}

/// True iff line `idx` falls inside the body of the
/// `fn run_entry(` definition. Walks upward until either
/// `fn run_entry(` (success) or another `fn ` declaration
/// (failure: this is a different function's body).
fn within_run_entry(lines: &[&str], idx: usize) -> bool {
    for j in (0..=idx).rev() {
        let trimmed = lines[j].trim_start();
        if trimmed.starts_with("fn run_entry(") || trimmed.starts_with("pub fn run_entry(") {
            return true;
        }
        if (trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn "))
            && trimmed.contains('(')
        {
            return false;
        }
    }
    false
}

/// Separate audit for the per-file dispatch path. `run_per_file`
/// builds a `live` list of (idx, entry) pairs that pass the
/// `entry.when` gate, then dispatches `PerFileRule::evaluate_file`
/// against each matched file. The `entry.when` consultation is
/// inline in the live-list build; this test asserts it stays
/// there.
#[test]
fn run_per_file_consults_entry_when() {
    let engine_src_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("alint-core/src/engine.rs");
    let src = fs::read_to_string(&engine_src_path).unwrap();
    let lines: Vec<&str> = src.lines().collect();

    // Find `fn run_per_file` and check its body contains
    // `entry.when` somewhere before any `evaluate_file(` call.
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("fn run_per_file") || t.starts_with("pub fn run_per_file") {
            start = Some(i);
            break;
        }
    }
    let Some(start_idx) = start else {
        panic!("`fn run_per_file` not found in engine.rs — structure changed");
    };

    // Walk forward until the next top-level fn declaration OR
    // EOF. Within that span, ensure `entry.when` appears before
    // `evaluate_file(`.
    let mut when_idx: Option<usize> = None;
    let mut eval_idx: Option<usize> = None;
    for (j, line) in lines.iter().enumerate().skip(start_idx + 1) {
        let t = line.trim_start();
        // Stop at the next top-level fn declaration (not nested).
        if (t.starts_with("fn ") || t.starts_with("pub fn ") || t.starts_with("pub(crate) fn "))
            && t.contains('(')
            && !line.starts_with("        ")
        {
            break;
        }
        if when_idx.is_none() && line.contains("entry.when") {
            when_idx = Some(j);
        }
        if eval_idx.is_none() && line.contains("evaluate_file(") && !t.starts_with("//") {
            eval_idx = Some(j);
        }
    }

    let when_line = when_idx.unwrap_or_else(|| {
        panic!("`run_per_file` does not consult `entry.when` — silent-no-op risk");
    });
    let eval_line = eval_idx.unwrap_or_else(|| {
        panic!(
            "`run_per_file` does not call `evaluate_file(` — structure changed, \
             audit needs revision",
        );
    });
    assert!(
        when_line < eval_line,
        "`run_per_file` consults `entry.when` (line {}) AFTER `evaluate_file` \
         (line {}) — gate must run BEFORE dispatch or disabled rules will fire",
        when_line + 1,
        eval_line + 1,
    );
}

/// True iff any line in `lines[i.saturating_sub(window)..=i]`
/// contains `needle`.
fn window_contains(lines: &[&str], i: usize, window: usize, needle: &str) -> bool {
    let start = i.saturating_sub(window);
    lines[start..=i].iter().any(|l| l.contains(needle))
}
