use super::*;

/// Release-gating of rule-body prose: `<!-- alint:since=X -->` blocks are
/// dropped when X exceeds the released version, and the marker comments never
/// reach the page. Revert-sensitive backstop for ADR-0007 / P1.
#[test]
fn strip_unreleased_prose_gates_since_blocks() {
    let body = "\
Intro line, always shown.

<!-- alint:since=0.14 -->
**Optional `root_only`** requires the match to be at the repo root.
<!-- /alint:since -->

Trailer line, always shown.
";
    // Released 0.13.0: the since=0.14 block is dropped; markers gone.
    let gated = strip_unreleased_prose(body, Some((0, 13, 0)));
    assert!(
        !gated.contains("root_only"),
        "unreleased prose leaked:\n{gated}"
    );
    assert!(
        !gated.contains("alint:since"),
        "marker comment leaked:\n{gated}"
    );
    assert!(gated.contains("Intro line") && gated.contains("Trailer line"));
    // Released 0.14.0: the block content is kept; markers still stripped.
    let shipped = strip_unreleased_prose(body, Some((0, 14, 0)));
    assert!(shipped.contains("root_only") && !shipped.contains("alint:since"));
    // Local/dev (None): content kept, markers stripped.
    let local = strip_unreleased_prose(body, None);
    assert!(local.contains("root_only") && !local.contains("alint:since"));
    // A body with no markers is returned byte-for-byte.
    let plain = "no markers here\n";
    assert_eq!(strip_unreleased_prose(plain, Some((0, 13, 0))), plain);
}

/// P-REF: `copy_site_tree` release-gates the hand-written docs the same way the
/// rule pages are gated, so a `<!-- alint:since=X -->` block in a main-overlaid
/// reference page can't ship ahead of the release. Revert-sensitive.
#[test]
fn copy_site_tree_release_gates_reference_prose() {
    let ws = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(ws.path().join("docs/site/reference")).unwrap();
    fs::write(
        ws.path().join("docs/site/reference/formats.md"),
        "Released line.\n\n<!-- alint:since=0.14 -->\nUnreleased baseline note.\n<!-- /alint:since -->\n\nTrailer.\n",
    )
    .unwrap();

    // Released 0.13.0: the since=0.14 block is stripped from the copied page.
    let out = tempfile::tempdir().unwrap();
    copy_site_tree(ws.path(), out.path(), Some((0, 13, 0))).unwrap();
    let gated = fs::read_to_string(out.path().join("reference/formats.md")).unwrap();
    assert!(
        !gated.contains("Unreleased baseline note"),
        "leaked:\n{gated}"
    );
    assert!(!gated.contains("alint:since"));
    assert!(gated.contains("Released line.") && gated.contains("Trailer."));

    // Released 0.14.0: the block content is kept (markers still stripped).
    let out2 = tempfile::tempdir().unwrap();
    copy_site_tree(ws.path(), out2.path(), Some((0, 14, 0))).unwrap();
    let shipped = fs::read_to_string(out2.path().join("reference/formats.md")).unwrap();
    assert!(shipped.contains("Unreleased baseline note") && !shipped.contains("alint:since"));
}

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
    generate_rules_pages(&workspace, tmp.path(), None, false).expect("generate rules pages");

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
/// Command` variants in `crates/alint/src/cli.rs`. If the
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
        .join("cli.rs");
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
             crates/alint/src/cli.rs. A new subcommand probably landed \
             without its `/docs/cli/<name>.md` reference page being \
             generated; bump CLI_REFERENCE_SUBCMDS in xtask/src/docs_export.rs \
             to match the enum (kebab-case)."
    );
}

/// The top-level `--help` renders as a formatted landing page: a Commands table
/// (known subcommands linked, clap builtins plain), a Global-options table with
/// wrapped descriptions folded into one cell, and a raw-dump fallback when the
/// help doesn't parse. Everything comes from the captured `--help`, so it can't
/// drift from the binary.
#[test]
fn format_top_help_renders_tables_and_falls_back() {
    let sample = "\
A monorepo linter.

Usage: alint [OPTIONS] [COMMAND]

Commands:
  check    Lint the repository
  fix      Auto-fix violations
  help     Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>  Path to a config file
      --no-gitignore     Disable .gitignore handling
                         (overrides config)
  -h, --help             Print help
";
    let out = format_top_help(sample).expect("well-formed help parses");
    // Global-options table, with the wrapped continuation folded into one cell.
    assert!(out.contains("## Global options"), "{out}");
    assert!(
        out.contains("| `-c, --config <CONFIG>` | Path to a config file |"),
        "{out}"
    );
    assert!(
        out.contains("Disable .gitignore handling (overrides config)"),
        "{out}"
    );
    // Commands table: a known subcommand links to its page; a clap builtin does not.
    assert!(out.contains("[`check`](/docs/cli/check/)"), "{out}");
    assert!(out.contains("| `help` | Print this message"), "{out}");
    assert!(
        !out.contains("[`help`]"),
        "clap builtin must not be linked: {out}"
    );

    // No Options section -> None, so the caller keeps the raw `--help` dump.
    assert!(format_top_help("Usage: alint\n\nCommands:\n  check  Lint\n").is_none());
}

/// Once options carry long help (the `wrap_help` + short/long split), clap renders
/// each option in its *next-line* layout: the flag header sits alone on a shallow
/// line and the (possibly multi-paragraph) help is indented below it. `format_top_help`
/// must still emit one Global-options row per flag with every paragraph, plus trailing
/// `[default:]`/`[possible values:]` metadata, folded into a single cell — the same
/// shape it produces for the same-line layout.
#[test]
fn format_top_help_folds_next_line_option_layout() {
    let sample = "\
A monorepo linter.

Usage: alint [OPTIONS] [COMMAND]

Commands:
  check    Lint the repository
  help     Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>
          Path to a config file

  -f, --format <FORMAT>
          Output format

          [default: human]

      --show-notes
          List informational notes in full on stderr.

          Notes are non-violation findings, e.g. entries a rule
          skipped rather than failed on.

  -h, --help
          Print help (see a summary with '-h')
";
    let out = format_top_help(sample).expect("next-line help parses");
    assert!(out.contains("## Global options"), "{out}");
    // Flag header alone on its line; the single help line below folds into the cell.
    assert!(
        out.contains("| `-c, --config <CONFIG>` | Path to a config file |"),
        "{out}"
    );
    // Trailing `[default:]` metadata folds into the same cell.
    assert!(
        out.contains("| `-f, --format <FORMAT>` | Output format [default: human] |"),
        "{out}"
    );
    // A multi-paragraph long help folds summary + detail into one cell.
    assert!(
        out.contains("List informational notes in full on stderr."),
        "{out}"
    );
    assert!(
        out.contains(
            "Notes are non-violation findings, e.g. entries a rule skipped rather than failed on."
        ),
        "{out}"
    );
    // Commands table is unchanged: known subcommands link, clap builtins stay plain.
    assert!(out.contains("[`check`](/docs/cli/check/)"), "{out}");
    assert!(
        !out.contains("[`help`]"),
        "clap builtin must not be linked: {out}"
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

/// Design invariant (docs/design/rule-categories.md): the `**Categories:**` line
/// is stripped from every H3 body BEFORE it is summarized or rendered, so no
/// generated summary or page body ever carries the literal marker. Tested
/// against the real docs/rules.md so a regression in the stripper is caught by
/// `cargo test`, not just at bundle-build time.
#[test]
fn no_residual_categories_marker_after_strip() {
    let root = crate::workspace_root().expect("workspace root");
    let src = std::fs::read_to_string(root.join("docs/rules.md")).expect("read docs/rules.md");
    for h2 in split_h2_sections(&src) {
        for h3 in split_h3_sections(&h2.body) {
            let (_cats, clean) = crate::categories_line::split_categories_line(&h3.body);
            assert!(
                !clean.contains("**Categories:**"),
                "residual **Categories:** in a stripped H3 body under {:?}",
                h2.title
            );
            assert!(
                !first_sentence(&clean).contains("**Categories:**"),
                "the summary (first_sentence) still contains the marker under {:?}",
                h2.title
            );
        }
    }
}
