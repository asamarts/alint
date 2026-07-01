use super::*;

/// `lead_example_with_kind` brings the matching-kind rule to the
/// front of a multi-variant example, and is a no-op otherwise.
#[test]
fn lead_example_reorders_multivariant_block() {
    let body = "\
```yaml
- id: a
  kind: json_path_equals
  level: error

- id: b
  kind: yaml_path_equals
  level: error
```
";
    // yaml page: the yaml rule moves to the front.
    let out = lead_example_with_kind(body, "yaml_path_equals");
    let first_kind = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("kind: "))
        .unwrap();
    assert_eq!(first_kind, "yaml_path_equals");
    // json page: already first → unchanged.
    assert_eq!(lead_example_with_kind(body, "json_path_equals"), body);
    // single-rule example → unchanged.
    let single = "```yaml\n- id: x\n  kind: file_exists\n```\n";
    assert_eq!(lead_example_with_kind(single, "file_exists"), single);
}

/// Generate the structured-query rule pages and assert each one's
/// first example leads with its OWN kind. Catches the templated-
/// clone bug the external evaluation flagged: the four
/// `*_path_equals` (and `*_path_matches`) pages all showed
/// `kind: json_path_*` because the multi-kind H3's single example
/// was fanned out verbatim. Scoped to these families — other
/// multi-kind H3s (`for_each_dir`/`for_each_file`, the file_*
/// content aliases) deliberately share one example demonstrating
/// the group, which reads correctly.
#[test]
fn structured_query_pages_lead_with_their_own_kind() {
    const FAMILY_KINDS: &[&str] = &[
        "json_path_equals",
        "yaml_path_equals",
        "toml_path_equals",
        "xml_path_equals",
        "json_path_matches",
        "yaml_path_matches",
        "toml_path_matches",
        "xml_path_matches",
    ];
    let workspace = crate::bench_release::workspace_root().expect("workspace_root");
    let tmp = tempfile::tempdir().expect("tempdir");
    generate_rules_pages(&workspace, tmp.path()).expect("generate rules pages");

    let rules_dir = tmp.path().join("rules");
    let mut pages: Vec<std::path::PathBuf> = Vec::new();
    collect_md(&rules_dir, &mut pages);
    assert!(!pages.is_empty(), "no rule pages were generated");

    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for page in &pages {
        let stem = page.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !FAMILY_KINDS.contains(&stem) {
            continue;
        }
        let text = fs::read_to_string(page).unwrap();
        // First `kind:` inside the first fenced yaml block.
        let Some(open) = text.find("```yaml") else {
            continue;
        };
        let after = &text[open..];
        let Some(close) = after[7..].find("```") else {
            continue;
        };
        let block = &after[7..7 + close];
        let Some(first_kind) = block.lines().find_map(|l| l.trim().strip_prefix("kind: ")) else {
            continue;
        };
        checked += 1;
        if first_kind != stem {
            mismatches.push(format!(
                "{}: first example shows `kind: {first_kind}` but the page is `{stem}`",
                page.display()
            ));
        }
    }
    assert_eq!(
        checked,
        FAMILY_KINDS.len(),
        "expected to check all {} structured-query pages, checked {checked} \
             (a page or its example went missing)",
        FAMILY_KINDS.len()
    );
    assert!(
        mismatches.is_empty(),
        "rule page(s) whose lead example names the wrong kind \
             (templated-clone regression):\n{}",
        mismatches.join("\n")
    );
}

fn collect_md(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// Pin the `CLI_REFERENCE_SUBCMDS` list against the `enum
/// Command` variants in `crates/alint/src/main.rs`. If the
/// binary gains a subcommand and the list isn't bumped, the
/// `/docs/cli/<new>/` URL would be a live 404 on alint.org;
/// this test catches that pre-merge.
#[test]
fn cli_reference_subcmds_match_command_enum() {
    let path = crate::bench_release::workspace_root()
        .expect("workspace_root")
        .join("crates")
        .join("alint")
        .join("src")
        .join("main.rs");
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    // Reuse the same variant-extraction approach the
    // `count_enum_variants` helper uses, but return the names
    // not just the count so we can compare set membership.
    let needle = "enum Command {";
    let start = src.find(needle).expect("enum Command {") + needle.len();
    let body = &src[start..];
    let mut depth = 1usize;
    let mut end = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &body[..end];
    let outer = super::counts::strip_nested_braces(body);
    let mut variants: Vec<String> = Vec::new();
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
            variants.push(pascal_to_kebab(ident));
        }
    }
    variants.sort();
    let mut listed: Vec<String> = CLI_REFERENCE_SUBCMDS
        .iter()
        .map(ToString::to_string)
        .collect();
    listed.sort();
    assert_eq!(
        variants, listed,
        "CLI_REFERENCE_SUBCMDS does not match `enum Command` variants in \
             crates/alint/src/main.rs. A new subcommand probably landed \
             without its `/docs/cli/<name>.md` reference page being \
             generated; bump CLI_REFERENCE_SUBCMDS in xtask/src/docs_export.rs \
             to match the enum (kebab-case)."
    );
}

/// `PascalCase` -> `kebab-case`. `ExportAgentsMd` ->
/// `export-agents-md`, matching clap's default conversion.
fn pascal_to_kebab(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 2);
    for (i, c) in ident.char_indices() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn pascal_to_kebab_examples() {
    assert_eq!(pascal_to_kebab("Check"), "check");
    assert_eq!(pascal_to_kebab("ExportAgentsMd"), "export-agents-md");
    assert_eq!(pascal_to_kebab("ValidateConfig"), "validate-config");
    assert_eq!(pascal_to_kebab("Lsp"), "lsp");
}
