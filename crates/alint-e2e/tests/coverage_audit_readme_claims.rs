//! Coverage audit: README.md marketing claims match their workspace
//! source of truth.
//!
//! README.md makes six numeric claims about alint's surface area
//! ("70 rule kinds across 13 families", "19 bundled ecosystem
//! rulesets", "12 auto-fix ops", "8 output formats", "9 subcommands").
//! These claims drift trivially: a new rule kind ships, a new fixer
//! lands, a new formatter lights up, but the README copy stays at
//! the old number. The v0.9.22 audit caught a multi-month "60 rule
//! kinds" claim that was 10 behind reality.
//!
//! This test parses each claim out of README.md and asserts it
//! matches a deterministic count derived from the canonical source:
//!
//! | Claim                  | Source of truth                                            |
//! |------------------------|------------------------------------------------------------|
//! | rule kinds             | distinct `kind:` values in `crates/alint-dsl/tests/fixtures/all_kinds.yaml` |
//! | families               | non-meta `## ` headings in `docs/rules.md`                 |
//! | bundled rulesets       | `.yml` files under `crates/alint-dsl/rulesets/v1/`          |
//! | auto-fix ops           | `pub struct *Fixer` declarations under `crates/alint-rules/src/fixers/` |
//! | output formats         | variants of `Format` enum in `crates/alint-output/src/lib.rs` |
//! | subcommands            | variants of `Command` enum in `crates/alint/src/cli.rs`     |
//!
//! Failures point the maintainer at exactly which side drifted.
//!
//! `about_page_surface_claims_match_readme` additionally pins
//! `docs/site/about/index.md` (a separate file on the docs-bundle
//! path to alint.org that repeats the README's surface-area
//! sentence) to the README, so the same drift can't slip through
//! the docs site.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for this test = crates/alint-e2e.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/alint-e2e has a parent")
        .parent()
        .expect("crates has a parent (workspace root)")
        .to_path_buf()
}

fn read_readme() -> String {
    let path = workspace_root().join("README.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The synced docs landing (`docs/site/about/index.md`) repeats the
/// same surface-area sentence the README leads with. It's a separate
/// file on the docs-bundle path to alint.org, so it drifts
/// independently — the v0.9.22 audit found it would have escaped this
/// test entirely. Pin it to the same truth via the README.
fn read_about_page() -> String {
    let path = workspace_root().join("docs/site/about/index.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Find the first occurrence of `marker` in `text` and return the
/// integer that immediately precedes it (skipping markdown bold and
/// whitespace).
///
/// "70 rule kinds across 13 families..."   marker="rule kinds across" -> 70
/// "**12 auto-fix ops**, ..."              marker="auto-fix ops"      -> 12
/// ", 19 bundled ecosystem rulesets, ..."  marker="bundled ecosystem rulesets" -> 19
fn num_before(text: &str, marker: &str) -> Option<usize> {
    let pos = text.find(marker)?;
    let preamble = &text[..pos];
    let trimmed = preamble.trim_end_matches(|c: char| c.is_whitespace() || c == '*');
    let digits: String = trimmed
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Distinct `kind:` values in a YAML fixture. Walks line-by-line so
/// nested rules under `require:` count their kinds too. Duplicates
/// across nested invocations collapse via the `HashSet`.
fn count_distinct_kinds(yaml: &str) -> usize {
    let mut kinds = HashSet::new();
    for raw in yaml.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("kind:") {
            let value = rest.trim().trim_end_matches(',');
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                kinds.insert(value.to_string());
            }
        }
    }
    kinds.len()
}

/// Count `## ` headings in docs/rules.md that name a rule family,
/// excluding the meta headings (Contents, Fix operations, etc.).
fn count_family_headings(md: &str) -> usize {
    const META: &[&str] = &[
        "Contents",
        "Fix operations",
        "Bundled rulesets",
        "Nested `.alint.yml` (monorepo layering)",
    ];
    md.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .filter(|h| !META.contains(h))
        .count()
}

/// Count `.yml` files under `dir` recursively. Stable iteration not
/// required (we only need the count).
fn count_yml_files_recursive(dir: &Path) -> usize {
    let mut total = 0;
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            total += count_yml_files_recursive(&entry.path());
        } else if ty.is_file()
            && entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "yml")
        {
            total += 1;
        }
    }
    total
}

/// Count `pub struct ...Fixer` lines across every `.rs` under `dir`.
fn count_fixer_structs(dir: &Path) -> usize {
    let mut total = 0;
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        for line in src.lines() {
            let t = line.trim_start();
            if t.starts_with("pub struct ") && t.contains("Fixer") {
                // Match either `pub struct FooFixer;` (unit struct)
                // or `pub struct FooFixer {` / `pub struct FooFixer(...);`
                // — both indicate one distinct fixer type.
                total += 1;
            }
        }
    }
    total
}

/// Find `enum <name> {` in `source` and count comma-terminated or
/// brace-terminated variant identifiers in its body. Tolerates
/// docstrings, attributes, and variants with struct/tuple fields.
fn count_enum_variants(source: &str, enum_name: &str) -> usize {
    let needle = format!("enum {enum_name} {{");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("enum {enum_name} not found"))
        + needle.len();

    let body = &source[start..];
    // Walk to the matching closing brace, accounting for nested braces
    // inside struct-style variants (`Variant { field: T }`).
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
    assert!(end > 0, "unterminated `enum {enum_name}` body");

    let body = &body[..end];
    // Strip nested-brace regions so we don't count identifiers inside
    // variant fields (`field: SomeType`) as variants.
    let outer = strip_nested_braces(body);

    let mut count = 0;
    for line in outer.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("#[") {
            continue;
        }
        // A variant declaration starts with an uppercase ASCII letter,
        // possibly followed by `,`, `{`, `(`, or `=`.
        if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            count += 1;
        }
    }
    count
}

/// Replace contents of every top-level `{ ... }` pair with spaces so
/// nested struct-variant bodies don't get walked as variant
/// declarations. Preserves outer-line structure.
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

// ---- Tests ----

#[test]
fn readme_rule_kinds_count_matches_fixture() {
    let readme = read_readme();
    let claimed = num_before(&readme, "rule kinds across")
        .expect("README must contain 'N rule kinds across M families'");

    let fixture_path = workspace_root().join("crates/alint-dsl/tests/fixtures/all_kinds.yaml");
    let fixture = fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", fixture_path.display()));
    let actual = count_distinct_kinds(&fixture);

    assert_eq!(
        claimed,
        actual,
        "README claims {claimed} rule kinds; all_kinds.yaml has {actual} distinct kinds.\n\
         Either bump the README claim or update the fixture.\n\
         (Source: README.md '...N rule kinds across...', fixture: {})",
        fixture_path.display()
    );
}

#[test]
fn readme_families_count_matches_docs_rules_md() {
    let readme = read_readme();
    // The marker "across" is followed by the family count then "families".
    // Walk past the rule-kinds claim and pick up the integer before
    // " families" on the same line.
    let marker = "rule kinds across ";
    let pos = readme
        .find(marker)
        .expect("README must contain 'N rule kinds across M families'");
    let after = &readme[pos + marker.len()..];
    let m_str: String = after.chars().take_while(char::is_ascii_digit).collect();
    let claimed: usize = m_str
        .parse()
        .expect("expected an integer immediately after 'rule kinds across '");

    let rules_md_path = workspace_root().join("docs/rules.md");
    let rules_md = fs::read_to_string(&rules_md_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", rules_md_path.display()));
    let actual = count_family_headings(&rules_md);

    assert_eq!(
        claimed, actual,
        "README claims {claimed} rule families; docs/rules.md has {actual} family `## ` headings.\n\
         Update README.md or add/remove a family section in docs/rules.md (meta headings\n\
         Contents / Fix operations / Bundled rulesets / Nested are excluded by design).",
    );
}

#[test]
fn readme_bundled_rulesets_count_matches_filesystem() {
    let readme = read_readme();
    let claimed = num_before(&readme, "bundled ecosystem rulesets")
        .expect("README must contain 'N bundled ecosystem rulesets'");

    let dir = workspace_root().join("crates/alint-dsl/rulesets/v1");
    let actual = count_yml_files_recursive(&dir);

    assert_eq!(
        claimed,
        actual,
        "README claims {claimed} bundled rulesets; {} has {actual} .yml files.\n\
         Update README.md (lines 11, 44, 221, plus the bullet list in 'Bundled rulesets').",
        dir.display()
    );
}

#[test]
fn readme_auto_fix_ops_count_matches_fixers() {
    let readme = read_readme();
    let claimed =
        num_before(&readme, "auto-fix ops").expect("README must contain 'N auto-fix ops'");

    let dir = workspace_root().join("crates/alint-rules/src/fixers");
    let actual = count_fixer_structs(&dir);

    assert_eq!(
        claimed,
        actual,
        "README claims {claimed} auto-fix ops; {actual} `pub struct *Fixer` declarations in {}.\n\
         Update README.md or check whether a new fixer was added without bumping the count.",
        dir.display()
    );
}

#[test]
fn readme_output_formats_count_matches_format_enum() {
    let readme = read_readme();
    let claimed =
        num_before(&readme, "output formats").expect("README must contain 'N output formats'");

    let path = workspace_root().join("crates/alint-output/src/lib.rs");
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let actual = count_enum_variants(&src, "Format");

    assert_eq!(
        claimed,
        actual,
        "README claims {claimed} output formats; `enum Format` in {} has {actual} variants.\n\
         Update README.md or check that a new formatter wasn't added without bumping the count.",
        path.display()
    );
}

#[test]
fn readme_subcommands_count_matches_command_enum() {
    let readme = read_readme();
    let claimed = num_before(&readme, "subcommands").expect("README must contain 'N subcommands'");

    let path = workspace_root().join("crates/alint/src/cli.rs");
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let actual = count_enum_variants(&src, "Command");

    assert_eq!(
        claimed,
        actual,
        "README claims {claimed} subcommands; `enum Command` in {} has {actual} variants.\n\
         Update README.md or check that a new subcommand wasn't added without bumping the count.",
        path.display()
    );
}

/// Extract the "N rule kinds across M families" pair from a doc body.
/// Returns `(rule_kinds, families)`.
fn rule_kinds_and_families(text: &str) -> (usize, usize) {
    let kinds = num_before(text, "rule kinds across")
        .expect("text must contain 'N rule kinds across M families'");
    let marker = "rule kinds across ";
    let pos = text.find(marker).expect("marker present (checked above)");
    let after = &text[pos + marker.len()..];
    let m_str: String = after.chars().take_while(char::is_ascii_digit).collect();
    let families: usize = m_str
        .parse()
        .expect("expected an integer immediately after 'rule kinds across '");
    (kinds, families)
}

#[test]
fn about_page_surface_claims_match_readme() {
    // docs/site/about/index.md leads with the same surface-area
    // sentence as README.md but is a distinct file on the
    // docs-bundle path to alint.org. The README claims are pinned
    // to truth by the tests above; asserting the about page equals
    // the README transitively pins the about page too — and closes
    // the drift gap the v0.9.22 audit flagged (the about page was
    // not covered by any test).
    let readme = read_readme();
    let about = read_about_page();

    let (readme_kinds, readme_families) = rule_kinds_and_families(&readme);
    let (about_kinds, about_families) = rule_kinds_and_families(&about);

    assert_eq!(
        about_kinds, readme_kinds,
        "docs/site/about/index.md claims {about_kinds} rule kinds; README.md claims \
         {readme_kinds}. They must agree (README is pinned to the all_kinds.yaml fixture \
         by readme_rule_kinds_count_matches_fixture)."
    );
    assert_eq!(
        about_families, readme_families,
        "docs/site/about/index.md claims {about_families} families; README.md claims \
         {readme_families}. They must agree (README is pinned to docs/rules.md)."
    );

    let readme_rulesets = num_before(&readme, "bundled ecosystem rulesets")
        .expect("README must contain 'N bundled ecosystem rulesets'");
    let about_rulesets = num_before(&about, "bundled ecosystem rulesets")
        .expect("about page must contain 'N bundled ecosystem rulesets'");
    assert_eq!(
        about_rulesets, readme_rulesets,
        "docs/site/about/index.md claims {about_rulesets} bundled rulesets; README.md \
         claims {readme_rulesets}. They must agree."
    );
}

// ---- Sanity unit tests for the parser helpers ----
//
// These guard the parsing logic itself: if num_before stops finding
// digits correctly (e.g. someone changes markdown style), the
// README-claim tests above would silently parse 0 and assert against
// a real count, masking a regression.

#[test]
fn num_before_handles_plain_and_bold_markdown() {
    assert_eq!(
        num_before("70 rule kinds across", "rule kinds across"),
        Some(70)
    );
    assert_eq!(num_before("**70 rule kinds**", "rule kinds"), Some(70));
    assert_eq!(num_before(", 12 auto-fix ops, ", "auto-fix ops"), Some(12));
    assert_eq!(num_before("xx 8 output formats", "output formats"), Some(8));
    assert_eq!(num_before("no number marker", "marker"), None);
}

#[test]
fn count_distinct_kinds_dedupes_repeated_kinds() {
    let yaml = "rules:\n\
                  - id: a\n\
                    kind: file_exists\n\
                  - id: b\n\
                    kind: file_exists\n\
                  - id: c\n\
                    kind: pair\n";
    assert_eq!(count_distinct_kinds(yaml), 2);
}

#[test]
fn count_family_headings_excludes_meta() {
    let md = "## Contents\n\
              ## Existence\n\
              ## Content\n\
              ## Fix operations\n\
              ## Bundled rulesets\n\
              ## Nested `.alint.yml` (monorepo layering)\n";
    assert_eq!(count_family_headings(md), 2);
}

#[test]
fn count_enum_variants_skips_doc_attrs_and_nested_struct_fields() {
    let src = "pub enum Format {\n\
                  /// Human-readable output.\n\
                  #[serde(rename = \"human\")]\n\
                  Human,\n\
                  Json,\n\
                  Github {\n\
                      annotated: bool,\n\
                  },\n\
              }\n";
    assert_eq!(count_enum_variants(src, "Format"), 3);
}
