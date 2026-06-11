//! `xtask gen-facts` — emit `facts.json`, the committed surface-area
//! contract.
//!
//! `facts.json` is the single machine-readable manifest of alint's
//! surface area: version, the six headline counts (rule kinds,
//! families, bundled rulesets, auto-fix ops, output formats,
//! subcommands), and catalogue lists the README, docs, and alint.org
//! render from instead of restating numbers in prose. Every field
//! derives from the same canonical source `coverage_audit_readme_claims`
//! pins the README to, so the contract can't disagree with the README.
//!
//! Mirrors `gen_schema`: `run(false)` rewrites the file, `run(true)`
//! content-diffs the committed copy and fails on drift. Carries no
//! volatile fields (no git sha / timestamp) so it commits cleanly and
//! gates on content. Design: `docs/design/facts-json.md` (ADR-0001,
//! Phase 3 / `WS1e`). The build-time `manifest.json` is a separate,
//! deliberately-untouched artifact.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// Bumped when the `facts.json` shape changes, so a downstream
/// consumer (alint.org) can pin the schema it understands.
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize)]
struct Facts {
    format_version: u32,
    alint_version: String,
    counts: Counts,
    rule_kinds: Vec<String>,
    families: Vec<String>,
    bundled_rulesets: Vec<String>,
    output_formats: Vec<String>,
    subcommands: Vec<String>,
    fact_predicates: Vec<String>,
}

#[derive(Serialize)]
struct Counts {
    rule_kinds: usize,
    families: usize,
    bundled_rulesets: usize,
    auto_fix_ops: usize,
    output_formats: usize,
    subcommands: usize,
}

fn facts_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("facts.json"))
}

fn build_facts() -> Result<Facts> {
    let root = crate::workspace_root()?;
    let rule_kinds = rule_kinds(&root)?;
    let families = families(&root)?;
    let bundled_rulesets = bundled_rulesets(&root)?;
    let output_formats = output_formats(&root)?;
    let mut subcommands: Vec<String> = crate::docs_export::CLI_REFERENCE_SUBCMDS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    subcommands.sort();
    let fact_predicates = fact_predicates();
    let auto_fix_ops = auto_fix_ops_count(&root)?;

    let counts = Counts {
        rule_kinds: rule_kinds.len(),
        families: families.len(),
        bundled_rulesets: bundled_rulesets.len(),
        auto_fix_ops,
        output_formats: output_formats.len(),
        subcommands: subcommands.len(),
    };

    Ok(Facts {
        format_version: FORMAT_VERSION,
        alint_version: env!("CARGO_PKG_VERSION").to_string(),
        counts,
        rule_kinds,
        families,
        bundled_rulesets,
        output_formats,
        subcommands,
        fact_predicates,
    })
}

/// Pretty JSON plus a trailing newline (git-friendly, and what
/// `--check` compares against byte-for-byte).
fn render(facts: &Facts) -> Result<String> {
    let mut s = serde_json::to_string_pretty(facts).context("serialize facts.json")?;
    s.push('\n');
    Ok(s)
}

pub fn run(check: bool) -> Result<()> {
    let facts = build_facts()?;
    let rendered = render(&facts)?;
    let path = facts_path()?;

    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "facts.json is stale. Run `cargo run -p xtask -- gen-facts` to \
                 regenerate and commit the result."
            );
        }
        println!("facts.json is up to date");
        return Ok(());
    }

    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote facts.json");
    Ok(())
}

// ---- canonical computations -----------------------------------------------
//
// Each mirrors the source `coverage_audit_readme_claims` pins the
// README to, so `facts.json`, the README, and the engine agree.

/// Distinct `kind:` values in the `all_kinds.yaml` fixture, sorted.
fn rule_kinds(root: &Path) -> Result<Vec<String>> {
    let path = root.join("crates/alint-dsl/tests/fixtures/all_kinds.yaml");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut set = BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("kind:") {
            let value = rest.trim().trim_end_matches(',');
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                set.insert(value.to_string());
            }
        }
    }
    Ok(set.into_iter().collect())
}

/// Non-meta `## ` family headings in `docs/rules.md`, sorted.
fn families(root: &Path) -> Result<Vec<String>> {
    const META: &[&str] = &[
        "Contents",
        "Fix operations",
        "Bundled rulesets",
        "Nested `.alint.yml` (monorepo layering)",
    ];
    let path = root.join("docs/rules.md");
    let md = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut set = BTreeSet::new();
    for heading in md
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
    {
        if !META.contains(&heading) {
            set.insert(heading.to_string());
        }
    }
    Ok(set.into_iter().collect())
}

/// `.yml` files (recursive) under `rulesets/v1/`, as sorted
/// extension-stripped relative identifiers (`apache/governance`, `go`).
fn bundled_rulesets(root: &Path) -> Result<Vec<String>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel);
            }
        }
        Ok(())
    }
    let dir = root.join("crates/alint-dsl/rulesets/v1");
    let mut set = BTreeSet::new();
    walk(&dir, &dir, &mut set)?;
    Ok(set.into_iter().collect())
}

/// Lowercased `enum Format` variant names from `alint-output`, sorted.
fn output_formats(root: &Path) -> Result<Vec<String>> {
    let path = root.join("crates/alint-output/src/lib.rs");
    let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut names: Vec<String> = enum_variant_names(&src, "Format")?
        .into_iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    names.sort();
    Ok(names)
}

/// `pub struct *Fixer` declarations (recursive) under `fixers/`.
fn auto_fix_ops_count(root: &Path) -> Result<usize> {
    fn walk(dir: &Path) -> Result<usize> {
        let mut n = 0;
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                n += walk(&path)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let src = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
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
    walk(&root.join("crates/alint-rules/src/fixers"))
}

/// The `FactSpec::name()` arms in `crates/alint-core/src/facts.rs`,
/// sorted. A small, stable set kept in lockstep with that match by the
/// list being short enough to eyeball in review.
fn fact_predicates() -> Vec<String> {
    let mut v: Vec<String> = [
        "all_files_exist",
        "any_file_exists",
        "count_files",
        "custom",
        "file_content_matches",
        "git_branch",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    v.sort();
    v
}

/// Variant identifiers of `enum <name>` in `source` (`PascalCase`, in
/// declaration order). Adapted from
/// `coverage_audit_readme_claims::count_enum_variants` — walks to the
/// matching close brace and ignores nested struct-variant fields.
fn enum_variant_names(source: &str, enum_name: &str) -> Result<Vec<String>> {
    let needle = format!("enum {enum_name} {{");
    let start = source
        .find(&needle)
        .with_context(|| format!("enum {enum_name} not found"))?
        + needle.len();
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
        bail!("unterminated `enum {enum_name}` body");
    }

    let outer = strip_nested_braces(&body[..end]);
    let mut names = Vec::new();
    for line in outer.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("#[") {
            continue;
        }
        if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let ident: String = t
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                names.push(ident);
            }
        }
    }
    Ok(names)
}

/// Blank out every top-level `{ ... }` so struct-variant field
/// identifiers aren't read as variants.
fn strip_nested_braces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '{' => {
                out.push(' ');
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                out.push(' ');
            }
            '\n' if depth > 0 => out.push('\n'),
            _ if depth > 0 => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `facts.json` must be regenerated and committed when any source
    /// changes — `--check` is what CI + preflight run.
    #[test]
    fn gen_facts_check_passes_on_committed_tree() {
        run(true).expect("gen-facts --check should pass on the committed tree");
    }

    /// The five list-backed counts equal their list lengths, and every
    /// list is sorted + de-duplicated.
    #[test]
    fn counts_equal_list_lengths_and_lists_are_sorted() {
        let f = build_facts().expect("build facts");
        assert_eq!(f.counts.rule_kinds, f.rule_kinds.len());
        assert_eq!(f.counts.families, f.families.len());
        assert_eq!(f.counts.bundled_rulesets, f.bundled_rulesets.len());
        assert_eq!(f.counts.output_formats, f.output_formats.len());
        assert_eq!(f.counts.subcommands, f.subcommands.len());

        for list in [
            &f.rule_kinds,
            &f.families,
            &f.bundled_rulesets,
            &f.output_formats,
            &f.subcommands,
            &f.fact_predicates,
        ] {
            let mut sorted = list.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                list, &sorted,
                "list must be sorted + de-duplicated: {list:?}"
            );
        }
    }

    /// Bind `facts.json` to the README's claimed numbers. The README is
    /// pinned to the canonical sources by `coverage_audit_readme_claims`,
    /// so this transitively pins `facts.json` to the same truth — and
    /// fails if this module's counting diverges from the audit's.
    #[test]
    fn counts_match_readme_claims() {
        let root = crate::workspace_root().expect("workspace_root");
        let readme = fs::read_to_string(root.join("README.md")).expect("read README.md");
        let f = build_facts().expect("build facts");

        let num_before = |marker: &str| -> usize {
            let pos = readme
                .find(marker)
                .unwrap_or_else(|| panic!("README missing marker {marker:?}"));
            let pre = readme[..pos].trim_end_matches(|c: char| c.is_whitespace() || c == '*');
            let digits: String = pre
                .chars()
                .rev()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            digits.parse().expect("integer before marker")
        };
        // "N rule kinds across M families"
        let after_kinds = {
            let m = "rule kinds across ";
            let pos = readme.find(m).expect("README 'rule kinds across'") + m.len();
            readme[pos..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<usize>()
                .expect("families integer")
        };

        assert_eq!(f.counts.rule_kinds, num_before("rule kinds across"));
        assert_eq!(f.counts.families, after_kinds);
        assert_eq!(
            f.counts.bundled_rulesets,
            num_before("bundled ecosystem rulesets")
        );
        assert_eq!(f.counts.auto_fix_ops, num_before("auto-fix ops"));
        assert_eq!(f.counts.output_formats, num_before("output formats"));
        assert_eq!(f.counts.subcommands, num_before("subcommands"));
    }
}
