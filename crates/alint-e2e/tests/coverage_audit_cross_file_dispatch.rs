//! Soft listing: cross-file rules whose `evaluate` body still
//! contains `entries.iter()` or `for ... in ctx.index.files()`
//! patterns — i.e. the O(D × N) shape v0.9.8 closed via
//! [`alint_core::FileIndex::children_of`]. Bench-scale numbers
//! catch the perf regression at runtime; this audit catches the
//! source-level shape at test time so the next refactor knows
//! which files still have the old pattern.
//!
//! Always passes (soft warning) — perf-shape policy is not a
//! correctness requirement, and some scans (`unique_by` over
//! `ctx.index.files()`) are O(N) by design and shouldn't be
//! routed through `children_of`. The point is to surface the
//! gap so Phase C of a future cut knows where to look.
//!
//! Run with `cargo test -- --nocapture` to see the listing.

use std::fs;
use std::path::{Path, PathBuf};

/// Cross-file rules whose dispatch shape benefits from
/// `children_of` (per-dir direct-children iteration). Rules
/// here that still scan `entries.iter()` directly inside the
/// `evaluate` body are flagged.
///
/// `unique_by` is intentionally NOT in this list — its single
/// `for entry in ctx.index.files()` pass is O(N) by design
/// (HashMap-based dedup over the whole tree); routing it
/// through `children_of` would require a different algorithm
/// shape (per-dir grouping with an outer aggregation pass)
/// that's not justified by any current bench scenario.
///
/// `pair` is also NOT in this list — `pair` already uses
/// `contains_file` (the v0.9.5 fast path) and doesn't need
/// `children_of`.
///
/// `for_each_dir`, `for_each_file`, `every_matching_has` go
/// through the shared `evaluate_for_each` helper, which
/// itself uses `children_of` indirectly via the v0.9.8
/// `nested_spec_single_literal` bypass. Rules here are the
/// ones whose own `evaluate` body contains the per-dir loop.
const CROSS_FILE_RULES_WITH_CHILDREN_OF: &[&str] = &[
    "dir_only_contains",
    "dir_contains",
];

#[test]
fn cross_file_rules_use_children_of() {
    let rules_src = workspace_root()
        .join("crates")
        .join("alint-rules")
        .join("src");

    let mut summary = String::new();
    let mut violators: Vec<String> = Vec::new();
    for kind in CROSS_FILE_RULES_WITH_CHILDREN_OF {
        let src_path = rules_src.join(format!("{kind}.rs"));
        let body = match fs::read_to_string(&src_path) {
            Ok(b) => b,
            Err(_) => {
                summary.push_str(&format!("  - {kind}: source missing at {}\n", src_path.display()));
                violators.push((*kind).to_string());
                continue;
            }
        };
        let uses_children_of = body.contains("children_of") || body.contains("file_basenames_of");
        let scans_entries = body.contains("ctx.index.entries.iter()")
            || body.contains("ctx.index.files()")
                && !body.contains("// audit-allow: full-files-scan");
        if scans_entries && !uses_children_of {
            violators.push((*kind).to_string());
            summary.push_str(&format!(
                "  - {kind}: evaluate() still scans ctx.index.entries.iter() / ctx.index.files() \
                 without children_of (file: {})\n",
                src_path.display(),
            ));
        }
    }

    if violators.is_empty() {
        eprintln!(
            "[audit] cross_file_dispatch: all {} cross-file rules with O(D × N) potential are \
             routed through FileIndex::children_of (or use the v0.9.5 contains_file fast path)",
            CROSS_FILE_RULES_WITH_CHILDREN_OF.len(),
        );
    } else {
        eprintln!(
            "[audit] cross_file_dispatch: {} of {} rules still scan the full entries vec per dir:\n{}",
            violators.len(),
            CROSS_FILE_RULES_WITH_CHILDREN_OF.len(),
            summary,
        );
        eprintln!(
            "  See docs/design/v0.9.8/cross-file-fast-paths-v2.md for the optimization shape \
             (FileIndex::children_of). Add a comment `// audit-allow: full-files-scan` to \
             intentionally retain the full scan.",
        );
    }
    // Soft: always pass.
}

#[test]
fn evaluate_for_each_has_literal_path_bypass() {
    let helper_src = workspace_root()
        .join("crates")
        .join("alint-rules")
        .join("src")
        .join("for_each_dir.rs");
    let body = fs::read_to_string(&helper_src).expect("for_each_dir.rs must exist");
    assert!(
        body.contains("nested_spec_single_literal"),
        "evaluate_for_each must keep the v0.9.8 literal-path bypass — see for_each_dir.rs",
    );
    assert!(
        body.contains("evaluate_one_per_file_rule"),
        "evaluate_for_each must keep the per-file-rule fast path — see for_each_dir.rs",
    );
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for the alint-e2e crate is
    // `<workspace>/crates/alint-e2e`. Walk up two levels.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("workspace root above crates/alint-e2e")
        .to_path_buf()
}
