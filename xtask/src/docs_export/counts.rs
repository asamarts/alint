//! Canonical drift-gate counts: parse the authoritative source files and
//! count the rule kinds / bundled rulesets / subcommands / output formats /
//! auto-fix ops that the docs manifest embeds. Self-contained parsers (no
//! other `docs_export` function is called); split out of `docs_export.rs`
//! for size (Dog2). The `count_canonical_*` entry points are `pub(super)`;
//! `write_manifest` in the parent module calls them.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Canonical subcommand count = `enum Command` variants in
/// `crates/alint/src/cli.rs`. Mirrors
/// `coverage_audit_readme_claims::readme_subcommands_count_matches_command_enum`.
pub(super) fn count_canonical_subcommands() -> Result<usize> {
    let path = crate::bench_release::workspace_root()?
        .join("crates")
        .join("alint")
        .join("src")
        .join("cli.rs");
    let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(count_enum_variants(&src, "Command"))
}

/// Canonical output-format count = `enum Format` variants in
/// `crates/alint-output/src/lib.rs`. Mirrors
/// `coverage_audit_readme_claims::readme_output_formats_count_matches_format_enum`.
pub(super) fn count_canonical_output_formats() -> Result<usize> {
    let path = crate::bench_release::workspace_root()?
        .join("crates")
        .join("alint-output")
        .join("src")
        .join("lib.rs");
    let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(count_enum_variants(&src, "Format"))
}

/// Canonical auto-fix-ops count = `pub struct *Fixer` declarations under
/// `crates/alint-rules/src/fixers/`. Mirrors
/// `coverage_audit_readme_claims::readme_auto_fix_ops_count_matches_fixers`.
pub(super) fn count_canonical_auto_fix_ops() -> Result<usize> {
    fn count_fixers(dir: &Path) -> Result<usize> {
        let mut n = 0;
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let p = entry?.path();
            if p.is_dir() {
                n += count_fixers(&p)?;
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                let src =
                    fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
                for line in src.lines() {
                    let line = line.trim_start();
                    if line.starts_with("pub struct ") && line.contains("Fixer") {
                        n += 1;
                    }
                }
            }
        }
        Ok(n)
    }
    let dir = crate::bench_release::workspace_root()?
        .join("crates")
        .join("alint-rules")
        .join("src")
        .join("fixers");
    count_fixers(&dir)
}

/// Find `enum <name> {` in `source` and count comma-terminated or
/// brace-terminated variant identifiers in its body. Tolerates
/// docstrings, attributes, and variants with struct/tuple fields.
/// Mirrors the helper in `coverage_audit_readme_claims` so the
/// manifest can't drift from the README-pinned canonical count.
fn count_enum_variants(source: &str, enum_name: &str) -> usize {
    let needle = format!("enum {enum_name} {{");
    let Some(start_idx) = source.find(&needle) else {
        return 0;
    };
    let start = start_idx + needle.len();
    let body = &source[start..];
    let mut depth = 1usize;
    let mut end = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if end == 0 {
        return 0;
    }
    let body = &body[..end];
    let outer = strip_nested_braces(body);
    let mut count = 0;
    for raw in outer.lines() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with("//") || line.starts_with("#[") {
            continue;
        }
        let first = line
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next();
        if let Some(ident) = first
            && let Some(c) = ident.chars().next()
            && c.is_ascii_uppercase()
        {
            count += 1;
        }
    }
    count
}

/// Strip every `{ … }` region (recursively) from the input so the
/// outer-variant tokens of a struct-style enum body don't include
/// the field names inside `Variant { field: T }`.
pub(super) fn strip_nested_braces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for c in input.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
            }
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Canonical rule-kind count = distinct `kind:` values in the
/// `all_kinds.yaml` test fixture. This is the same source-of-truth that
/// `coverage_audit_readme_claims::readme_rule_kinds_count_matches_fixture`
/// pins README.md against, so this manifest field can never drift from
/// the README claim (the test would fail first). alint.org's
/// `check-version-pins.sh` consumes this value to gate the cross-repo
/// `<N>` rule-kind claim on every static landing.
pub(super) fn count_canonical_rule_kinds() -> Result<usize> {
    let path = crate::bench_release::workspace_root()?
        .join("crates")
        .join("alint-dsl")
        .join("tests")
        .join("fixtures")
        .join("all_kinds.yaml");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut kinds: std::collections::HashSet<String> = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("kind:") {
            let value = rest.trim().trim_end_matches(',');
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                kinds.insert(value.to_string());
            }
        }
    }
    Ok(kinds.len())
}

/// Canonical bundled-ruleset count = recursive `.yml` file count under
/// `crates/alint-dsl/rulesets/v1/`. Mirrors
/// `coverage_audit_readme_claims::readme_bundled_rulesets_count_matches_filesystem`
/// so this manifest field can never drift from the README claim.
pub(super) fn count_canonical_bundled_rulesets() -> Result<usize> {
    fn count_yml(dir: &Path) -> Result<usize> {
        let mut n = 0;
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let p = entry?.path();
            if p.is_dir() {
                n += count_yml(&p)?;
            } else if p.extension().and_then(|e| e.to_str()) == Some("yml") {
                n += 1;
            }
        }
        Ok(n)
    }
    let dir = crate::bench_release::workspace_root()?
        .join("crates")
        .join("alint-dsl")
        .join("rulesets")
        .join("v1");
    count_yml(&dir)
}
