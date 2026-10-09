use super::cli::{
    CliProse, StrippedHelp, code_list, global_options, parse_cli_prose, render_cli_page,
    strip_global_options, top_level_only,
};
use super::exported_pages::set_frontmatter_description;
use super::rulesets::rule_sources;
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
        // The CONFIG block is the ```yaml under "With this `.alint.yml`:", NOT
        // the first ```yaml on the page: file-content rendering (ADR-0014) now
        // shows a `.yml` fixture file (e.g. a workflow) as a ```yaml block that
        // can precede the config. Anchor on the marker to reach the real config.
        let Some(marker) = text.find("With this `.alint.yml`:") else {
            continue;
        };
        let Some(rel) = text[marker..].find("```yaml") else {
            continue;
        };
        let open = marker + rel;
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
    let out = format_top_help(sample, &[]).expect("well-formed help parses");
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
    assert!(format_top_help("Usage: alint\n\nCommands:\n  check  Lint\n", &[]).is_none());
    // Every option works with every subcommand unless it is named as top-level only.
    assert!(out.contains("These apply to every subcommand.\n"), "{out}");
    let out = format_top_help(sample, &["--version".to_string()]).expect("parses");
    assert!(
        out.contains("These apply to every subcommand except `--version`.\n"),
        "{out}"
    );
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
    let out = format_top_help(sample, &[]).expect("next-line help parses");
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

/// `example_first_kind` reads ONLY the leading yaml block (the canonical-kind
/// gate polices that block); `example_block_kinds` reads EVERY block (the
/// double-example gate must catch a stale config wherever it sits). The
/// `git_commit_message` shape - a non-config recipe first, a config after -
/// is exactly where the two diverge, so lock both in.
#[test]
fn block_kind_scanners_read_first_vs_all_yaml_blocks() {
    // A CI-workflow recipe (no `kind:`) followed by a config block: the leading
    // scan sees nothing, the all-blocks scan finds the config's kind.
    let body = "\
Intro prose.

```yaml
# .github/workflows/lint.yml
on: [push]
jobs: {}
```

More prose.

```yaml
- id: r
  kind: git_commit_message
  max_subject_length: 50
```
";
    assert_eq!(
        example_first_kind(body),
        None,
        "leading block is a recipe with no kind:"
    );
    assert_eq!(
        example_block_kinds(body),
        vec!["git_commit_message".to_string()],
        "the config in a later block must still be seen"
    );

    // Leading config block: both scanners agree, and a later block's kind is
    // still collected in document order.
    let two_configs = "\
```yaml
- id: a
  kind: file_exists
```

```yaml
- id: b
  kind: file_absent
```
";
    assert_eq!(
        example_first_kind(two_configs).as_deref(),
        Some("file_exists")
    );
    assert_eq!(
        example_block_kinds(two_configs),
        vec!["file_exists".to_string(), "file_absent".to_string()]
    );

    // No yaml at all: both empty.
    assert_eq!(example_first_kind("just prose"), None);
    assert_eq!(example_block_kinds("just prose"), [] as [String; 0]);
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

/// Every generated rule summary is well-formed prose, on BOTH surfaces that
/// share the `first_sentence` splitter: the SERP `description:` frontmatter
/// (`rule_meta_description`) and the website rule-index one-liner (`KindEntry`,
/// via bare `first_sentence`). Checked against the real docs/rules.md so a
/// regression is caught by `cargo test`, not only at bundle-build time. Guards
/// the #97 failure modes: an opening sentence cut at an abbreviation ("differ
/// only by case (e.g. ...)" → "...(e.g.", `no_case_conflicts`), a list lead-in
/// ending in a colon (`no_illegal_windows_names`, `file_is_ascii`), and a cap
/// that strands an unclosed "(" (`ordered_block`).
#[test]
fn rule_meta_descriptions_are_well_formed() {
    // Abbreviation forms `first_sentence` must not treat as a sentence end. If
    // one lands immediately before the composed " alint <kind>" suffix, the
    // opening sentence was truncated mid-abbreviation.
    const ABBREV_DOT: &[&str] = &[
        "e.g.", "i.e.", "vs.", "cf.", "etc.", "al.", "resp.", "approx.", "fig.", "no.",
    ];
    let balanced = |s: &str| s.matches('(').count() == s.matches(')').count();
    let root = crate::workspace_root().expect("workspace root");
    let src = std::fs::read_to_string(root.join("docs/rules.md")).expect("read docs/rules.md");
    let mut descriptions = std::collections::BTreeMap::new();
    for h2 in split_h2_sections(&src) {
        for h3 in split_h3_sections(&h2.body) {
            let (_cats, clean) = crate::categories_line::split_categories_line(&h3.body);
            // The rule-index summary is the same first sentence, markdown intact.
            // A cut at an abbreviation strands an unclosed "(" ("...case (e.g.").
            let idx_summary = first_sentence(&clean);
            assert!(
                balanced(&idx_summary),
                "unbalanced parens in {:?} index summary: {idx_summary:?}",
                h3.title
            );
            for kind in extract_kinds(&h3.title) {
                let desc = rule_meta_description(&kind, &h2.title, &clean);
                assert!(
                    desc.contains(&kind),
                    "{kind} description omits its searchable kind name: {desc:?}"
                );
                assert!(
                    desc.starts_with(&format!("alint {kind} rule (")),
                    "{kind} description does not lead with its searchable name: {desc:?}"
                );
                assert!(
                    desc.chars().count() <= 158,
                    "{kind} description exceeds 158 characters: {desc:?}"
                );
                if let Some(previous) = descriptions.insert(desc.clone(), kind.clone()) {
                    panic!("duplicate rule descriptions for {previous} and {kind}: {desc:?}");
                }
                assert!(
                    desc.ends_with('.') && desc.len() > 10,
                    "{kind} description is empty/unterminated: {desc:?}"
                );
                // A sentence cut mid-clause (inside "(e.g. ...)") leaves a
                // dangling open paren.
                assert!(
                    balanced(&desc),
                    "unbalanced parens in {kind} description: {desc:?}"
                );
                // A lead-in that still carries trailing punctuation leaves
                // these artifacts when composed with its final stop.
                for bad in [":.", ";.", ",."] {
                    assert!(
                        !desc.contains(bad),
                        "punctuation artifact {bad:?} in {kind} description: {desc:?}"
                    );
                }
                // The plain-text SERP snippet must not leak markdown emphasis or
                // unicode arrows (`strip_markup` folds them): a `<meta>`
                // description shows literal "**regex**" / "⇒" otherwise.
                assert!(
                    !desc.contains("**"),
                    "markdown bold leaked into {kind} description: {desc:?}"
                );
                for arrow in ['→', '⇒', '←', '⇐', '↔'] {
                    assert!(
                        !desc.contains(arrow),
                        "unicode arrow {arrow:?} in {kind} description: {desc:?}"
                    );
                }
                // The opening sentence must not have been cut at an abbreviation
                // and then presented as an intentional ellipsis.
                for ab in ABBREV_DOT {
                    assert!(
                        !desc.contains(&format!("{ab}...")),
                        "{kind} description split at abbreviation {ab:?}: {desc:?}"
                    );
                }
                for bad_article in ["a HCL document", "a INI document", "a XML document"] {
                    assert!(
                        !desc.contains(bad_article),
                        "incorrect article in {kind} description: {desc:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn truncated_meta_summaries_are_explicit_and_well_formed() {
    let input = "Validate a long generated description against a repository structure and \
                 its declared constraints.";
    let summary = truncate_meta_summary(input, 54);
    assert!(summary.ends_with("..."), "{summary:?}");
    assert!(summary.chars().count() <= 54, "{summary:?}");
    assert!(!summary.ends_with(" the..."), "{summary:?}");

    let parenthetical = truncate_meta_summary(
        "Validate paths (including nested paths with special handling) against the repository.",
        32,
    );
    assert_eq!(
        parenthetical.matches('(').count(),
        parenthetical.matches(')').count(),
        "{parenthetical:?}"
    );
}

/// Ruleset descriptions are generated from author prose, so validate the real
/// catalogue rather than a toy fixture. This guards the #266 failure mode where
/// the reserved suffix forced a word cap that was then disguised as a complete
/// sentence (for example, "files named." or "authored or co-authored by.").
#[test]
fn ruleset_meta_descriptions_are_well_formed() {
    let root = crate::workspace_root().expect("workspace root");
    let rulesets_root = root.join(docs_paths::RULESETS_DIR);
    let mut descriptions = std::collections::BTreeMap::new();

    for entry in walkdir_plain(&rulesets_root).expect("walk bundled rulesets") {
        if !entry.is_file()
            || !matches!(
                entry.extension().and_then(|ext| ext.to_str()),
                Some("yml" | "yaml")
            )
        {
            continue;
        }
        let rel = entry
            .strip_prefix(&rulesets_root)
            .expect("ruleset relative path");
        let name = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let source = std::fs::read_to_string(&entry).expect("read bundled ruleset");
        let overview = render_overview_from_comments(&source);
        let desc = ruleset_meta_description(&name, &overview);

        assert!(desc.contains(&format!("{name}@v1")), "{name}: {desc:?}");
        assert!(desc.chars().count() <= 158, "{name}: {desc:?}");
        assert!(
            desc.ends_with('.') || desc.ends_with('!') || desc.ends_with('?'),
            "{name}: unterminated description: {desc:?}"
        );
        assert!(
            !desc.contains(['`', '—', '–']),
            "{name}: non-plain-text description: {desc:?}"
        );
        assert_eq!(
            desc.matches('(').count(),
            desc.matches(')').count(),
            "{name}: unbalanced description: {desc:?}"
        );
        if let Some(previous) = descriptions.insert(desc.clone(), name.clone()) {
            panic!("duplicate ruleset descriptions for {previous} and {name}: {desc:?}");
        }

        let lead = format!("{name}@v1 bundled alint ruleset");
        let opening = meta_desc_clean(
            &strip_markup(&first_overview_sentence(&overview)),
            usize::MAX,
        );
        let budget = 158usize.saturating_sub(lead.chars().count() + 3);
        if opening.chars().count() >= 25 && opening.chars().count() > budget {
            assert!(
                desc.contains("..."),
                "{name}: truncated prose is presented as complete: {desc:?}"
            );
        }
    }
}

/// The global options of a trimmed-down top-level `alint --help`, for the
/// strip tests.
fn sample_globals() -> std::collections::HashMap<String, (String, String)> {
    let top = "\
A monorepo linter.

Usage: alint [OPTIONS] [COMMAND]

Options:
  -c, --config <CONFIG>
          Path to a config file

  -f, --format <FORMAT>
          Output format

          [default: human]

      --show-notes
          List informational notes in full on stderr.
<WS>
          Notes are non-violation findings.

  -h, --help
          Print help (see a summary with '-h')
"
    .replace("<WS>", "          ");
    global_options(&top)
}

/// Subcommand `--help` repeats every global option verbatim; the subcommand
/// page keeps only its own arguments and options, since the globals are
/// documented once on the CLI landing page. Kept options keep their
/// multi-paragraph help and `[default:]` metadata, and clap's whitespace-only
/// separator lines don't leave doubled blanks behind.
#[test]
fn strip_global_options_keeps_only_subcommand_flags() {
    let globals = sample_globals();
    for flag in ["--config", "--format", "--show-notes", "--help"] {
        assert!(
            globals.contains_key(flag),
            "{flag} missing from {globals:?}"
        );
    }

    let check = "\
Run linters against the current (or given) directory

Usage: alint check [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to lint

          [default: .]

Options:
  -c, --config <CONFIG>
          Path to a config file

      --changed
          Lint only files changed since the base ref.
<WS>
          Pairs with --base.

  -f, --format <FORMAT>
          Output format

          [default: human]

      --show-notes
          List informational notes in full on stderr.
<WS>
          Notes are non-violation findings.

      --base <REF>
          Base ref for --changed

  -h, --help
          Print help (see a summary with '-h')
"
    .replace("<WS>", "          ");
    let stripped = strip_global_options(&check, &globals);
    let out = &stripped.help;
    assert_eq!(
        stripped.removed,
        ["--config", "--format", "--show-notes", "--help"],
        "{out}"
    );
    assert!(stripped.own.is_empty(), "{out}");
    assert!(out.contains("Arguments:\n  [PATH]"), "{out}");
    assert!(out.contains("[default: .]"), "{out}");
    assert!(out.contains("      --changed\n"), "{out}");
    assert!(out.contains("Pairs with --base."), "{out}");
    assert!(out.contains("      --base <REF>"), "{out}");
    for gone in ["--config", "--format", "--show-notes", "Print help"] {
        assert!(!out.contains(gone), "{gone} should be stripped: {out}");
    }
    assert!(!out.contains("\n\n\n"), "no doubled blank lines: {out}");
}

/// A subcommand's OWN option that shares a global's flag (e.g. `suggest
/// --format`, with its own values and default) is not the global: it stays,
/// and is reported so the page can say it replaces the global. An Options
/// header left with no entries is dropped.
#[test]
fn strip_global_options_keeps_own_options_and_drops_empty_sections() {
    let globals = sample_globals();

    // A subcommand's own `--format` (different help, default and values)
    // shares the global's flag but is not the global; it stays.
    let suggest = "\
Scan for antipatterns and propose rules that would catch them

Usage: alint suggest [OPTIONS]

Options:
  -f, --format <FORMAT>
          Output format for proposals

          [default: human]
          [possible values: human, yaml, json]

  -c, --config <CONFIG>
          Path to a config file
";
    let stripped = strip_global_options(suggest, &globals);
    let out = &stripped.help;
    assert_eq!(stripped.removed, ["--config"], "{out}");
    assert_eq!(stripped.own, ["--format"], "{out}");
    assert!(
        out.contains("  -f, --format <FORMAT>\n          Output format for proposals"),
        "{out}"
    );
    assert!(
        out.contains("[possible values: human, yaml, json]"),
        "{out}"
    );
    assert!(!out.contains("Path to a config file"), "{out}");

    let explain = "\
Show a rule's definition

Usage: alint explain [OPTIONS] <RULE_ID>

Arguments:
  <RULE_ID>
          Rule id to describe

Options:
  -c, --config <CONFIG>
          Path to a config file

  -h, --help
          Print help (see a summary with '-h')
";
    let stripped = strip_global_options(explain, &globals);
    let out = &stripped.help;
    assert_eq!(stripped.removed, ["--config", "--help"], "{out}");
    assert!(!out.contains("Options:"), "{out}");
    assert!(out.ends_with("Rule id to describe\n"), "{out}");
}

/// Hand-written CLI prose (`docs/site/cli/<sub>.md`) splits into its
/// description, intro, middle sections and trailing See also. Headings count
/// only at the start of a line and outside code fences, `### See also` is not
/// the See also section, CRLF checkouts parse the same, and a frontmatter key
/// the generated page would drop is an error.
#[test]
fn parse_cli_prose_splits_intro_sections_and_see_also() {
    let text = "---\ntitle: 'alint fix'\ndescription: 'Fix it, don''t just flag it.'\n---\n\nIntro paragraph.\n\n## Examples\n\n```bash\nalint fix --dry-run\n## not a heading, inside a fence\n```\n\n### See also the flags\n\nText.\n\n## See also\n\n- [Fixing](/docs/concepts/adoption/fixing/)\n";
    for input in [text.to_string(), text.replace('\n', "\r\n")] {
        let prose = parse_cli_prose(&input).expect("valid prose parses");
        assert_eq!(
            prose.description.as_deref(),
            Some("Fix it, don't just flag it.")
        );
        assert_eq!(prose.intro, "Intro paragraph.");
        assert!(prose.sections.starts_with("## Examples"), "{prose:?}");
        assert!(
            prose.sections.contains("## not a heading, inside a fence"),
            "{prose:?}"
        );
        assert!(
            prose.sections.contains("### See also the flags"),
            "{prose:?}"
        );
        assert!(prose.see_also.starts_with("## See also\n"), "{prose:?}");
        assert!(
            prose.see_also.contains("/docs/concepts/adoption/fixing/"),
            "{prose:?}"
        );
    }

    // No frontmatter and no headings: everything is intro.
    let bare = parse_cli_prose("Just an intro.\n").expect("bare prose parses");
    assert_eq!(bare.description, None);
    assert_eq!(bare.intro, "Just an intro.");
    assert!(bare.sections.is_empty() && bare.see_also.is_empty());

    // Frontmatter that isn't YAML, or carries a key the page would drop, fails.
    assert!(parse_cli_prose("---\ndescription: [unclosed\n---\nbody\n").is_err());
    let err = parse_cli_prose("---\ntitle: x\nsidebar:\n  order: 2\n---\nbody\n").unwrap_err();
    assert!(err.to_string().contains("sidebar"), "{err}");

    // A description that isn't a non-empty string, or that a search snippet
    // would cut, fails rather than being replaced or truncated.
    for bad in ["description: 42", "description: ''", "description: [a, b]"] {
        let err = parse_cli_prose(&format!("---\n{bad}\n---\nbody\n")).unwrap_err();
        assert!(err.to_string().contains("non-empty string"), "{bad}: {err}");
    }
    let long = "x".repeat(156);
    let err = parse_cli_prose(&format!("---\ndescription: '{long}'\n---\nbody\n")).unwrap_err();
    assert!(err.to_string().contains("156 characters"), "{err}");
    let max = "x".repeat(155);
    let prose = parse_cli_prose(&format!("---\ndescription: '{max}'\n---\nbody\n")).unwrap();
    assert_eq!(prose.description.as_deref(), Some(max.as_str()));
}

/// Section headings are found the way Markdown finds them: `~~~` fences and
/// longer backtick fences hide headings until a matching close, an indent of
/// up to three spaces still makes a heading (four is code), and the See also
/// heading matches in any case and with closing hashes.
#[test]
fn parse_cli_prose_follows_markdown_fences_and_headings() {
    let tilde = parse_cli_prose("Intro.\n\n~~~\n## hidden\n~~~\n\n## Examples\n").unwrap();
    assert_eq!(tilde.intro, "Intro.\n\n~~~\n## hidden\n~~~");
    assert_eq!(tilde.sections, "## Examples");

    let long_fence =
        "Intro.\n\n````md\n```\n## hidden\n````\n\n## Examples\n\n## See also\n\n- x\n";
    let prose = parse_cli_prose(long_fence).unwrap();
    assert!(prose.intro.ends_with("````"), "{prose:?}");
    assert_eq!(prose.sections, "## Examples");
    assert_eq!(prose.see_also, "## See also\n\n- x");

    let indented = parse_cli_prose("Intro.\n\n   ## Examples\n\n    ## code\n").unwrap();
    assert_eq!(indented.intro, "Intro.");
    assert!(indented.sections.contains("    ## code"), "{indented:?}");

    for heading in ["## See Also", "## see also ##", "  ## SEE ALSO"] {
        let prose = parse_cli_prose(&format!("## Examples\n\nx\n\n{heading}\n\n- y\n")).unwrap();
        assert_eq!(prose.sections, "## Examples\n\nx", "{heading}");
        assert!(prose.see_also.ends_with("- y"), "{heading}: {prose:?}");
    }
}

/// A CLI page with prose: the prose description wins, the intro leads, the
/// help capture sits under a Reference heading after the prose sections, the
/// global-options pointer follows it, and See also comes last. Without prose
/// the page is just the capture (plus the derived description).
#[test]
fn render_cli_page_orders_prose_around_the_reference() {
    let prose = CliProse {
        description: Some("Hand-written description.".into()),
        intro: "Intro.".into(),
        sections: "## Examples\n\n```bash\nalint explain x\n```".into(),
        see_also: "## See also\n\n- [List](/docs/cli/list/)".into(),
    };
    let help = "Show a rule's definition\n\nUsage: alint explain <RULE_ID>\n";
    let stripped = StrippedHelp {
        help: help.into(),
        removed: vec!["--config".into()],
        own: vec![],
    };
    let page = render_cli_page("explain", "Derived.", Some(&prose), &stripped);
    assert!(
        page.starts_with(
            "---\ntitle: 'alint explain'\ndescription: 'Hand-written description.'\n---\n"
        ),
        "{page}"
    );
    let pos = |needle: &str| {
        page.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing: {page}"))
    };
    assert!(pos("Intro.") < pos("## Examples"));
    assert!(pos("## Examples") < pos("## Reference"));
    assert!(pos("## Reference") < pos("Usage: alint explain"));
    assert!(pos("Usage: alint explain") < pos("[global options](/docs/cli/#global-options)"));
    assert!(pos("[global options]") < pos("## See also"));

    assert!(!page.contains("Its own"), "{page}");

    // A subcommand that redefines a global says which of its options replace it.
    let own = |flags: &[&str]| StrippedHelp {
        own: flags.iter().map(|f| (*f).to_string()).collect(),
        ..stripped.clone()
    };
    let page = render_cli_page("suggest", "Derived.", None, &own(&["--format"]));
    assert!(
        page.contains(
            "where they are relevant. Its own `--format` above replaces the global one.\n"
        ),
        "{page}"
    );
    let page = render_cli_page("x", "Derived.", None, &own(&["--format", "--config"]));
    assert!(
        page.contains("Its own `--format` and `--config` above replace the global ones.\n"),
        "{page}"
    );

    let unstripped = StrippedHelp {
        help: help.into(),
        ..StrippedHelp::default()
    };
    let bare = render_cli_page("explain", "Derived.", None, &unstripped);
    assert!(bare.contains("description: 'Derived.'"), "{bare}");
    assert!(
        !bare.contains("## Reference") && !bare.contains("global options"),
        "{bare}"
    );
}

/// Prose files that no generated page would merge fail the export: a name
/// that isn't a subcommand would ship bare, and `index.md` would be silently
/// overwritten by the landing page generated from `alint --help`.
#[test]
fn check_cli_prose_files_rejects_unmatched_prose() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("check.md"), "---\ntitle: x\n---\n").unwrap();
    super::cli::check_cli_prose_files(dir.path()).expect("a subcommand's prose is fine");
    std::fs::write(dir.path().join("diagram.svg"), "<svg/>").unwrap();
    super::cli::check_cli_prose_files(dir.path()).expect("a non-page asset is fine");
    for stray in ["index.md", "nonsense.md", "check.mdx"] {
        std::fs::write(dir.path().join(stray), "---\ntitle: x\n---\n").unwrap();
        let err = super::cli::check_cli_prose_files(dir.path()).unwrap_err();
        assert!(err.to_string().contains(stray), "{err}");
        std::fs::remove_file(dir.path().join(stray)).unwrap();
    }
    std::fs::create_dir(dir.path().join("rules")).unwrap();
    let err = super::cli::check_cli_prose_files(dir.path()).unwrap_err();
    assert!(err.to_string().contains("\"rules\""), "{err}");
    super::cli::check_cli_prose_files(&dir.path().join("missing")).expect("no dir is fine");
}

/// Pages exported verbatim get a `description:` added to their frontmatter
/// (quotes escaped); a page that already has one keeps it.
#[test]
fn set_frontmatter_description_adds_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.md");
    std::fs::write(&path, "---\ntitle: Changelog\n---\n\nBody.\n").unwrap();
    set_frontmatter_description(&path, "It's every release.").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "---\ntitle: Changelog\ndescription: 'It''s every release.'\n---\n\nBody.\n"
    );
    set_frontmatter_description(&path, "Something else.").unwrap();
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("Something else")
    );

    std::fs::write(&path, "No frontmatter.\n").unwrap();
    assert!(set_frontmatter_description(&path, "x").is_err());
}

/// Every subcommand page ships with hand-written prose. Without a
/// `docs/site/cli/<sub>.md`, the page is the bare `--help` capture with a
/// description derived from it: the thin, near-duplicate page the prose exists
/// to replace.
#[test]
fn every_cli_subcommand_has_prose() {
    let cli_dir = crate::workspace_root()
        .expect("workspace root")
        .join(docs_paths::SITE_DIR)
        .join("cli");
    let missing: Vec<&str> = CLI_REFERENCE_SUBCMDS
        .iter()
        .copied()
        .filter(|sub| !cli_dir.join(format!("{sub}.md")).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "no prose in {} for: {missing:?}",
        cli_dir.display()
    );
}

/// The landing page names the global options no subcommand takes, as a list
/// of code spans.
#[test]
fn top_level_only_lists_the_globals_no_subcommand_takes() {
    let globals: std::collections::HashMap<String, (String, String)> =
        ["--config", "--version", "--help"]
            .iter()
            .map(|f| ((*f).to_string(), (String::new(), String::new())))
            .collect();
    let seen: std::collections::HashSet<String> = ["--config", "--help"]
        .iter()
        .map(|f| (*f).to_string())
        .collect();
    assert_eq!(top_level_only(&globals, &seen), ["--version"]);
    assert_eq!(
        top_level_only(&globals, &globals.keys().cloned().collect()),
        [] as [String; 0]
    );

    let flags = |list: &[&str]| list.iter().map(|f| (*f).to_string()).collect::<Vec<_>>();
    assert_eq!(code_list(&flags(&[])), "");
    assert_eq!(code_list(&flags(&["--a"])), "`--a`");
    assert_eq!(code_list(&flags(&["--a", "--b"])), "`--a` and `--b`");
    assert_eq!(
        code_list(&flags(&["--a", "--b", "--c"])),
        "`--a`, `--b` and `--c`"
    );
}

/// Rule notes come from the comments above a rule and at its key indent; a
/// divider or blank line cuts the comments above off, a comment nested in a
/// value stays in the definition, and a paragraph opening with a release tag
/// reads "Changed in".
#[test]
fn rule_sources_split_notes_from_definitions() {
    let yaml = "\
version: 1
rules:
  # --- Section divider -------------------------------
  - id: first-rule
    # Why the first rule exists.
    # It spans two lines.
    kind: file_exists
    paths:
      # which files count
      - README.md
    level: warning

  # A note that floats, cut off by the blank line below.

  # v0.9.18: broadened to more names.
  #
  # Why the second rule exists.
  - id: second-rule
    kind: file_absent
    paths: [\".DS_Store\"]
    level: info
";
    let sources = rule_sources(yaml);
    assert_eq!(sources.len(), 2, "{sources:#?}");
    assert_eq!(sources[0].id, "first-rule");
    assert_eq!(
        sources[0].notes_md,
        "Why the first rule exists. It spans two lines."
    );
    assert_eq!(
        sources[0].definition,
        "- id: first-rule\n  kind: file_exists\n  paths:\n    # which files count\n    - README.md\n  level: warning"
    );
    assert_eq!(sources[1].id, "second-rule");
    assert_eq!(
        sources[1].notes_md,
        "Changed in v0.9.18: broadened to more names.\n\nWhy the second rule exists."
    );
    assert!(
        !sources[1].definition.contains('#'),
        "{:?}",
        sources[1].definition
    );
}

/// Hard-wrapped comment prose is rejoined and escaped, so a wrapped line
/// can't turn into a list item and a bare glob can't turn into emphasis;
/// a list stays a list and a column-aligned block stays preformatted.
#[test]
fn rule_notes_render_as_safe_markdown() {
    let yaml = "\
rules:
  # Excludes cover `src/doc/**` and **/*.miri.rs, unique to rust-lang/rust
  # + similar projects. Copyright <year> holders.
  #
  # Two categories:
  #
  #   src/doc/**    — doc examples
  #   tests/ui/**   — UI fixtures
  #
  # - first item,
  #   continued
  # - second item
  - id: r
    kind: file_exists
    paths: x
";
    let notes = &rule_sources(yaml)[0].notes_md;
    assert_eq!(
        notes,
        "Excludes cover `src/doc/**` and \\*\\*/\\*.miri.rs, unique to rust-lang/rust + \
         similar projects. Copyright `<year>` holders.\n\n\
         Two categories:\n\n\
         ```text\nsrc/doc/**    — doc examples\ntests/ui/**   — UI fixtures\n```\n\n\
         - first item, continued\n- second item"
    );
}

#[test]
fn escape_inline_keeps_code_spans_and_urls() {
    use super::rulesets::escape_inline;
    assert_eq!(
        escape_inline("set <Nullable>enable</Nullable>, or `<Project Sdk=\"x\">`"),
        "set `<Nullable>enable</Nullable>`, or `<Project Sdk=\"x\">`"
    );
    assert_eq!(
        escape_inline("pin @<sha> (\"Copyright <year>\") if a < b"),
        "pin @`<sha>` (\"Copyright `<year>`\") if a &lt; b"
    );
    assert_eq!(
        escape_inline("see https://example.com/a_b_c for snake_case"),
        "see https://example.com/a_b_c for snake\\_case"
    );
    assert_eq!(escape_inline("an `unclosed span"), "an \\`unclosed span");
}

/// Every rule serde sees in a bundled ruleset is found by the comment-aware
/// splitter, in order, with a definition that parses back to the same rule,
/// and its id slugs to itself (the summary table links `#<id>`).
#[test]
fn rule_sources_cover_every_bundled_rule() {
    let root = crate::workspace_root().expect("workspace root");
    let rulesets_root = root.join(docs_paths::RULESETS_DIR);
    for entry in walkdir_plain(&rulesets_root).expect("walk bundled rulesets") {
        if !entry.is_file()
            || !matches!(
                entry.extension().and_then(|ext| ext.to_str()),
                Some("yml" | "yaml")
            )
        {
            continue;
        }
        let source = std::fs::read_to_string(&entry).expect("read bundled ruleset");
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source).expect("parse");
        let rules = yaml
            .get("rules")
            .and_then(|r| r.as_sequence())
            .cloned()
            .unwrap_or_default();
        let split = rule_sources(&source);
        let ids: Vec<&str> = rules
            .iter()
            .map(|r| r.get("id").and_then(|v| v.as_str()).unwrap_or(""))
            .collect();
        let split_ids: Vec<&str> = split.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, split_ids, "{}", entry.display());
        for (rule, s) in rules.iter().zip(&split) {
            let parsed: Vec<serde_yaml_ng::Value> = serde_yaml_ng::from_str(&s.definition)
                .unwrap_or_else(|e| panic!("{} {}: {e}\n{}", entry.display(), s.id, s.definition));
            assert_eq!(parsed.first(), Some(rule), "{} {}", entry.display(), s.id);
            assert!(
                s.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} {}: id would not slug to itself",
                entry.display(),
                s.id
            );
        }
    }
}

/// A hard-wrapped overview line that happens to start with a list marker
/// stays prose; a list after a colon, or opening its paragraph, stays a list.
#[test]
fn overview_wrapped_marker_lines_stay_prose() {
    let yaml = "\
# alint://bundled/x@v1
#
# Layouts like `ext/*` + `runtime/`
# + `cli/` will no-op, and so will
# 1. this line.
#
# Conventions:
# - `packages/*` for npm,
#   one per package
# - `crates/*` for Rust
version: 1
";
    assert_eq!(
        render_overview_from_comments(yaml),
        "Layouts like `ext/*` + `runtime/`\n\\+ `cli/` will no-op, and so will\n1\\. this line.\n\n\
         Conventions:\n- `packages/*` for npm,\n  one per package\n- `crates/*` for Rust"
    );
}
