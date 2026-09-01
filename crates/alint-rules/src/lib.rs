//! Built-in rule implementations for alint.
//!
//! Rules are registered into an [`alint_core::RuleRegistry`] via
//! [`register_builtin`]. Each kind has its own submodule.

use alint_core::RuleRegistry;

/// The generated kind-to-category bridge (`xtask gen-categories`, from the
/// `**Categories:**` lines in docs/rules.md). Consumed by `gen-facts` today;
/// will back the Phase-2 CLI category discovery (`alint rules`, `list --category`).
#[path = "categories_gen.rs"]
pub mod categories;

/// The generated per-kind summary bridge (`xtask gen-categories`, from the
/// opening sentence of each kind's docs/rules.md section, cleaned + capped).
/// Backs `alint explain` and `alint rules` per ADR-0011.
#[path = "kind_docs_gen.rs"]
pub mod kind_docs;

/// Generates a migrated rule kind's `options_schema()` fn: the schemars-derived
/// JSON Schema for its `Options` struct. `xtask gen-schema` composes the result
/// with the `kind`/`paths` structure preserved from the committed base branch.
/// See [`migrated_option_schemas`] and ADR-0001.
macro_rules! options_schema_for {
    ($ty:ty) => {
        #[must_use]
        pub fn options_schema() -> ::serde_json::Value {
            ::serde_json::to_value(::schemars::schema_for!($ty))
                .expect("options JSON schema serializes to a value")
        }
    };
}
pub(crate) use options_schema_for;

/// Render a path for a violation message with forward slashes on every platform.
/// `Path::display()` emits the OS-native separator (`\` on Windows), which is
/// inconsistent with the rest of alint's output and makes message contents
/// platform-dependent; this mirrors the normalization the walker already applies
/// to reported paths.
#[must_use]
pub(crate) fn slash(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().display().to_string().replace('\\', "/")
}

/// True when `path` names something NESTED — more than one path component,
/// i.e. not directly at the repository root. The existence family
/// (`file_exists`/`file_absent`/`dir_exists`/`dir_absent`) uses this to honour
/// their `root_only:` option: when set, only root-level paths are considered.
#[must_use]
pub(crate) fn is_nested(path: &std::path::Path) -> bool {
    path.components().count() != 1
}

#[cfg(test)]
mod slash_tests {
    #[test]
    fn renders_forward_slashes_regardless_of_separator() {
        use std::path::Path;
        // On Linux `\` is a legal filename byte, so this exercises exactly the
        // replacement the fix relies on for Windows path separators.
        assert_eq!(super::slash(Path::new("a\\b\\c")), "a/b/c");
        assert_eq!(super::slash(Path::new("a/b/c")), "a/b/c");
    }
}

pub mod case;
pub mod changeset_requires_path;
pub mod command;
pub mod command_idempotent;
pub mod commented_out_code;
mod commit_range;
pub mod cross_file;
pub mod dir_absent;
pub mod dir_contains;
pub mod dir_exists;
pub mod dir_only_contains;
pub mod every_matching_has;
pub mod executable_bit;
pub mod executable_has_shebang;
pub mod file_absent;
pub mod file_content_forbidden;
pub mod file_content_matches;
pub mod file_ends_with;
pub mod file_exists;
pub mod file_footer;
pub mod file_graph;
pub mod file_hash;
pub mod file_header;
pub mod file_is_ascii;
pub mod file_is_text;
pub mod file_max_lines;
pub mod file_max_size;
pub mod file_min_lines;
pub mod file_min_size;
pub mod file_shebang;
pub mod file_starts_with;
pub mod filename_case;
pub mod filename_regex;
pub mod final_newline;
pub mod fixers;
pub mod for_each_dir;
pub mod for_each_file;
pub mod for_each_match;
pub mod generated_file_fresh;
pub mod git_blame_age;
pub mod git_commit_author_allowlist;
pub mod git_commit_gpg_signed;
pub mod git_commit_message;
pub mod git_commit_no_fixup;
pub mod git_commit_signed_off;
pub mod git_commit_subject_matches;
pub mod git_no_denied_paths;
pub mod import_gate;
pub mod indent_style;
pub mod io;
pub mod json_schema_passes;
pub mod line_endings;
pub mod line_max_width;
pub mod markdown_paths_resolve;
pub mod max_consecutive_blank_lines;
pub mod max_directory_depth;
pub mod max_files_per_directory;
pub mod no_bidi_controls;
pub mod no_bom;
pub mod no_case_conflicts;
pub mod no_empty_files;
pub mod no_illegal_windows_names;
pub mod no_merge_conflict_markers;
pub mod no_submodules;
pub mod no_symlinks;
pub mod no_trailing_whitespace;
pub mod no_zero_width_chars;
pub mod ordered_block;
pub mod pair;
pub mod pair_changed_together;
pub mod pair_hash;
mod pathsafe;
pub mod registry_paths_resolve;
pub mod shebang_has_executable;
mod spawn;
pub mod structured_path;
#[cfg(test)]
mod test_support;
pub mod unique_by;

/// Rule kinds whose options schema is generated from Rust types via schemars,
/// rather than hand-written in `schemas/v1/config.json`. Each entry maps a
/// `$defs/rule_<kind>` definition name to that kind's derived options schema;
/// `xtask gen-schema` composes it with the `kind`/`paths`/`required` structure
/// preserved from the committed base branch.
///
/// Migration is incremental (see ADR-0001 and
/// `docs/design/spec-driven-development.md`): a kind absent from this list keeps
/// its hand-written branch verbatim, so the published schema stays complete and
/// valid at every step.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn migrated_option_schemas() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("rule_file_exists", file_exists::options_schema()),
        ("rule_file_absent", file_absent::options_schema()),
        ("rule_dir_exists", dir_exists::options_schema()),
        ("rule_dir_absent", dir_absent::options_schema()),
        ("rule_cross_file", cross_file::options_schema()),
        (
            "rule_registry_paths_resolve",
            registry_paths_resolve::options_schema(),
        ),
        ("rule_dir_contains", dir_contains::options_schema()),
        (
            "rule_every_matching_has",
            every_matching_has::options_schema(),
        ),
        (
            "rule_generated_file_fresh",
            generated_file_fresh::options_schema(),
        ),
        ("rule_file_header", file_header::options_schema()),
        ("rule_file_footer", file_footer::options_schema()),
        ("rule_file_max_size", file_max_size::options_schema()),
        ("rule_file_min_size", file_min_size::options_schema()),
        ("rule_file_max_lines", file_max_lines::options_schema()),
        ("rule_file_min_lines", file_min_lines::options_schema()),
        (
            "rule_file_content_matches",
            file_content_matches::options_schema(),
        ),
        (
            "rule_file_content_forbidden",
            file_content_forbidden::options_schema(),
        ),
        ("rule_file_shebang", file_shebang::options_schema()),
        ("rule_filename_regex", filename_regex::options_schema()),
        ("rule_file_starts_with", file_starts_with::options_schema()),
        ("rule_file_ends_with", file_ends_with::options_schema()),
        ("rule_line_max_width", line_max_width::options_schema()),
        (
            "rule_max_directory_depth",
            max_directory_depth::options_schema(),
        ),
        (
            "rule_max_files_per_directory",
            max_files_per_directory::options_schema(),
        ),
        (
            "rule_max_consecutive_blank_lines",
            max_consecutive_blank_lines::options_schema(),
        ),
        ("rule_unique_by", unique_by::options_schema()),
        (
            "rule_markdown_paths_resolve",
            markdown_paths_resolve::options_schema(),
        ),
        (
            "rule_git_commit_message",
            git_commit_message::options_schema(),
        ),
        (
            "rule_git_commit_signed_off",
            git_commit_signed_off::options_schema(),
        ),
        (
            "rule_git_commit_no_fixup",
            git_commit_no_fixup::options_schema(),
        ),
        (
            "rule_git_commit_subject_matches",
            git_commit_subject_matches::options_schema(),
        ),
        (
            "rule_git_commit_gpg_signed",
            git_commit_gpg_signed::options_schema(),
        ),
        (
            "rule_git_commit_author_allowlist",
            git_commit_author_allowlist::options_schema(),
        ),
        ("rule_git_blame_age", git_blame_age::options_schema()),
        (
            "rule_git_no_denied_paths",
            git_no_denied_paths::options_schema(),
        ),
        (
            "rule_changeset_requires_path",
            changeset_requires_path::options_schema(),
        ),
        (
            "rule_pair_changed_together",
            pair_changed_together::options_schema(),
        ),
        ("rule_pair", pair::options_schema()),
        ("rule_line_endings", line_endings::options_schema()),
        ("rule_indent_style", indent_style::options_schema()),
        ("rule_file_is_ascii", file_is_ascii::options_schema()),
        ("rule_ordered_block", ordered_block::options_schema()),
        ("rule_file_hash", file_hash::options_schema()),
        ("rule_pair_hash", pair_hash::options_schema()),
        (
            "rule_commented_out_code",
            commented_out_code::options_schema(),
        ),
        ("rule_command", command::options_schema()),
        (
            "rule_command_idempotent",
            command_idempotent::options_schema(),
        ),
        ("rule_import_gate", import_gate::options_schema()),
        ("rule_executable_bit", executable_bit::options_schema()),
        (
            "rule_json_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_yaml_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_toml_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_xml_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_json_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_yaml_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_toml_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_xml_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_json_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_yaml_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_toml_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_xml_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_json_schema_passes",
            json_schema_passes::options_schema(),
        ),
    ]
}

/// Register every built-in rule kind into the given registry.
///
/// Naming convention: rules that have a `dir_*` sibling keep
/// their `file_*` prefix (`file_exists` vs `dir_exists`); rules
/// with no such parallel also register a short alias without the
/// prefix — `content_matches`, `content_forbidden`, `header`,
/// `is_text`, `max_size`. Both forms resolve to the same
/// builder; new rules land under short names only.
// A flat one-line-per-kind registration table that grows with every
// rule kind; splitting it into arbitrary sub-functions would obscure
// the "every kind registered here" invariant the drift audits rely on.
#[allow(clippy::too_many_lines)]
pub fn register_builtin(registry: &mut RuleRegistry) {
    registry.register("file_exists", file_exists::build);
    registry.register("file_absent", file_absent::build);
    registry.register("dir_exists", dir_exists::build);
    registry.register("dir_absent", dir_absent::build);

    registry.register("file_content_matches", file_content_matches::build);
    registry.register_alias(
        "content_matches",
        "file_content_matches",
        file_content_matches::build,
    );
    registry.register("file_content_forbidden", file_content_forbidden::build);
    registry.register_alias(
        "content_forbidden",
        "file_content_forbidden",
        file_content_forbidden::build,
    );
    registry.register("file_header", file_header::build);
    registry.register_alias("header", "file_header", file_header::build);
    registry.register("file_max_size", file_max_size::build);
    registry.register_alias("max_size", "file_max_size", file_max_size::build);
    registry.register("file_min_size", file_min_size::build);
    registry.register_alias("min_size", "file_min_size", file_min_size::build);
    registry.register("file_min_lines", file_min_lines::build);
    registry.register_alias("min_lines", "file_min_lines", file_min_lines::build);
    registry.register("file_max_lines", file_max_lines::build);
    registry.register_alias("max_lines", "file_max_lines", file_max_lines::build);
    registry.register("file_footer", file_footer::build);
    registry.register_alias("footer", "file_footer", file_footer::build);
    registry.register("file_shebang", file_shebang::build);
    registry.register_alias("shebang", "file_shebang", file_shebang::build);

    // Structured-query family — JSONPath queries over
    // JSON / YAML / TOML documents.
    registry.register("json_path_equals", structured_path::json_path_equals_build);
    registry.register(
        "json_path_matches",
        structured_path::json_path_matches_build,
    );
    registry.register("yaml_path_equals", structured_path::yaml_path_equals_build);
    registry.register(
        "yaml_path_matches",
        structured_path::yaml_path_matches_build,
    );
    registry.register("toml_path_equals", structured_path::toml_path_equals_build);
    registry.register(
        "toml_path_matches",
        structured_path::toml_path_matches_build,
    );
    registry.register("xml_path_equals", structured_path::xml_path_equals_build);
    registry.register("xml_path_matches", structured_path::xml_path_matches_build);
    registry.register(
        "dotenv_path_equals",
        structured_path::dotenv_path_equals_build,
    );
    registry.register(
        "dotenv_path_matches",
        structured_path::dotenv_path_matches_build,
    );
    registry.register(
        "properties_path_equals",
        structured_path::properties_path_equals_build,
    );
    registry.register(
        "properties_path_matches",
        structured_path::properties_path_matches_build,
    );
    // Existence assertion for the full {json,yaml,toml,xml,dotenv,properties} family. Symmetry
    // with the equals/matches ops is enforced by `structured_family_is_symmetric`.
    registry.register("json_path_absent", structured_path::json_path_absent_build);
    registry.register("yaml_path_absent", structured_path::yaml_path_absent_build);
    registry.register("toml_path_absent", structured_path::toml_path_absent_build);
    registry.register("xml_path_absent", structured_path::xml_path_absent_build);
    registry.register(
        "dotenv_path_absent",
        structured_path::dotenv_path_absent_build,
    );
    registry.register(
        "properties_path_absent",
        structured_path::properties_path_absent_build,
    );
    registry.register("json_schema_passes", json_schema_passes::build);
    registry.register("markdown_paths_resolve", markdown_paths_resolve::build);
    registry.register("commented_out_code", commented_out_code::build);
    registry.register("git_no_denied_paths", git_no_denied_paths::build);
    registry.register("git_commit_message", git_commit_message::build);
    registry.register("git_commit_signed_off", git_commit_signed_off::build);
    registry.register(
        "git_commit_subject_matches",
        git_commit_subject_matches::build,
    );
    registry.register("git_commit_no_fixup", git_commit_no_fixup::build);
    registry.register(
        "git_commit_author_allowlist",
        git_commit_author_allowlist::build,
    );
    registry.register("git_commit_gpg_signed", git_commit_gpg_signed::build);
    registry.register("git_blame_age", git_blame_age::build);
    registry.register("changeset_requires_path", changeset_requires_path::build);
    registry.register_optionless("file_is_text", file_is_text::build);
    registry.register_alias_optionless("is_text", "file_is_text", file_is_text::build);

    registry.register("filename_case", filename_case::build);
    registry.register("filename_regex", filename_regex::build);
    registry.register("pair", pair::build);
    registry.register("pair_changed_together", pair_changed_together::build);
    registry.register("pair_hash", pair_hash::build);
    registry.register("for_each_dir", for_each_dir::build);
    registry.register("for_each_file", for_each_file::build);
    registry.register("dir_only_contains", dir_only_contains::build);
    registry.register("unique_by", unique_by::build);
    registry.register("dir_contains", dir_contains::build);
    registry.register("every_matching_has", every_matching_has::build);
    registry.register("registry_paths_resolve", registry_paths_resolve::build);
    // The unified cross-file value-relation kind; `cross_file_value_equals`
    // (v0.10) is a byte-compatible alias with `relation` defaulting to `equals`.
    registry.register("cross_file", cross_file::build);
    registry.register_alias("cross_file_value_equals", "cross_file", cross_file::build);
    registry.register("file_graph", file_graph::build);
    registry.register("ordered_block", ordered_block::build);
    registry.register("for_each_match", for_each_match::build);
    registry.register("generated_file_fresh", generated_file_fresh::build);
    registry.register("import_gate", import_gate::build);
    registry.register("command_idempotent", command_idempotent::build);

    // Text-hygiene family (short names — no `file_` prefix).
    registry.register_optionless("no_trailing_whitespace", no_trailing_whitespace::build);
    registry.register_optionless("final_newline", final_newline::build);
    registry.register("line_endings", line_endings::build);
    registry.register("line_max_width", line_max_width::build);

    // Security / Unicode sanity.
    registry.register_optionless(
        "no_merge_conflict_markers",
        no_merge_conflict_markers::build,
    );
    registry.register_optionless("no_bidi_controls", no_bidi_controls::build);
    registry.register_optionless("no_zero_width_chars", no_zero_width_chars::build);

    // Encoding + content fingerprint.
    registry.register("file_is_ascii", file_is_ascii::build);
    registry.register_optionless("no_bom", no_bom::build);
    registry.register("file_hash", file_hash::build);

    // Structure / layout.
    registry.register("max_directory_depth", max_directory_depth::build);
    registry.register("max_files_per_directory", max_files_per_directory::build);
    registry.register_optionless("no_empty_files", no_empty_files::build);

    // Cross-platform / portable metadata.
    registry.register_optionless("no_case_conflicts", no_case_conflicts::build);
    registry.register_optionless("no_illegal_windows_names", no_illegal_windows_names::build);

    // Unix metadata + git.
    registry.register_optionless("no_symlinks", no_symlinks::build);
    registry.register("executable_bit", executable_bit::build);
    registry.register_optionless("executable_has_shebang", executable_has_shebang::build);
    registry.register_optionless("shebang_has_executable", shebang_has_executable::build);
    registry.register_optionless("no_submodules", no_submodules::build);

    // Hygiene + byte fingerprint.
    registry.register("indent_style", indent_style::build);
    registry.register(
        "max_consecutive_blank_lines",
        max_consecutive_blank_lines::build,
    );
    registry.register("file_starts_with", file_starts_with::build);
    registry.register("file_ends_with", file_ends_with::build);

    // Plugin tier 1 — shell out to an external CLI per matched
    // file. Trust-gated at config-load: only the user's own
    // top-level config can declare these.
    registry.register("command", command::build);
}

/// Convenience constructor that returns a fresh registry pre-populated with
/// every built-in rule.
pub fn builtin_registry() -> RuleRegistry {
    let mut r = RuleRegistry::new();
    register_builtin(&mut r);
    r
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn every_alias_resolves_to_a_known_non_alias_canonical() {
        // Guards `register_alias`: each alias must point at a canonical kind
        // that is itself registered and is NOT an alias (no alias chains), so
        // `canonical_kind` always lands on a real rule/page in one hop. Catches
        // a typo'd or reordered `register_alias` at test time.
        let r = builtin_registry();
        let known: std::collections::HashSet<&str> = r.known_kinds().collect();
        let mut alias_count = 0usize;
        for kind in r.known_kinds() {
            let canon = r.canonical_kind(kind);
            if canon == kind {
                continue; // canonical (or standalone) kind
            }
            alias_count += 1;
            assert!(
                known.contains(canon),
                "alias `{kind}` resolves to unregistered canonical `{canon}`"
            );
            assert_eq!(
                r.canonical_kind(canon),
                canon,
                "alias `{kind}` points at `{canon}`, which is itself an alias (chain)"
            );
        }
        // The 11 known aliases: the 10 short-name `file_*` forms + is_text, plus
        // `cross_file_value_equals`. A drift in that count is a deliberate change.
        assert_eq!(alias_count, 11, "unexpected number of registered aliases");
    }

    #[test]
    fn structured_family_is_symmetric() {
        // Every config-format-specific op (`<fmt>_path_<op>`) must exist for ALL
        // four formats. Guards against shipping an asymmetric structured-query
        // family -- e.g. a lone `yaml_path_absent` with no json/toml/xml siblings
        // (the gap that motivated this test). Adding a new op or a new format means
        // adding every (format, op) pair, or this fails.
        const FORMATS: &[&str] = &["json", "yaml", "toml", "xml", "dotenv", "properties"];
        let r = builtin_registry();
        let known: std::collections::HashSet<&str> = r.known_kinds().collect();
        // Discover ops: any `<fmt>_path_<op>` kind contributes the suffix `path_<op>`.
        let mut ops: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for kind in &known {
            for &fmt in FORMATS {
                if let Some(rest) = kind.strip_prefix(fmt).and_then(|r| r.strip_prefix('_')) {
                    if rest.starts_with("path_") {
                        ops.insert(rest);
                    }
                }
            }
        }
        assert!(
            !ops.is_empty(),
            "no structured-query ops discovered -- the test is vacuous (naming scheme change?)"
        );
        let mut missing = Vec::new();
        for op in &ops {
            for fmt in FORMATS {
                let kind = format!("{fmt}_{op}");
                if !known.contains(kind.as_str()) {
                    missing.push(kind);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "structured-query family is asymmetric -- every op must exist for all of \
             {FORMATS:?}. Missing: {missing:?}"
        );
    }

    #[test]
    fn kind_summaries_are_terminal_ready() {
        // The generated ADR-0011 bridge is only guarded by a byte-exact `--check`
        // drift gate, which ratifies whatever `kind_summary` emits (the very
        // anti-pattern ADR-0012 warns against). Assert well-formedness over the
        // shipped artifact directly, so a malformed summary (residual markup,
        // unbalanced parens/quotes, over-cap, or a dangling function word before
        // the "..." marker - the class found on 9 kinds) can't ship silently.
        const STOP: &[&str] = &[
            "a", "an", "the", "and", "or", "of", "to", "in", "on", "at", "for", "with", "by",
            "from", "as", "that", "this", "its", "into", "than", "per",
        ];
        for (kind, summary) in crate::kind_docs::KIND_SUMMARIES {
            assert!(
                summary.chars().count() <= 100,
                "{kind}: summary over 100 chars ({}): {summary:?}",
                summary.chars().count()
            );
            assert!(
                !summary.contains('`') && !summary.contains("**"),
                "{kind}: residual markdown markup in summary: {summary:?}"
            );
            assert_eq!(
                summary.matches('(').count(),
                summary.matches(')').count(),
                "{kind}: unbalanced parens in summary: {summary:?}"
            );
            assert_eq!(
                summary.matches('"').count() % 2,
                0,
                "{kind}: unbalanced quotes in summary: {summary:?}"
            );
            if let Some(body) = summary.strip_suffix("...") {
                // Trim the SAME trailing punctuation the generator's
                // `drop_trailing_stopwords` strips (plus `)`), or a dangling
                // stop-word followed by `"` / `)` / `'` / `;` would slip the gate
                // (`last` would be e.g. `the"` and miss the STOP match).
                let last = body
                    .trim_end_matches([',', ';', ':', ' ', '(', ')', '"', '\''])
                    .rsplit(' ')
                    .next()
                    .unwrap_or("");
                assert!(
                    !STOP.contains(&last.to_ascii_lowercase().as_str()),
                    "{kind}: summary truncates on a dangling stop-word {last:?}: {summary:?}"
                );
            }
        }
    }

    #[test]
    fn every_documented_kind_is_registered() {
        let r = builtin_registry();
        let known: Vec<&str> = r.known_kinds().collect();
        for kind in [
            // Prefixed kinds (parallel with dir_*).
            "file_exists",
            "file_absent",
            "dir_exists",
            "dir_absent",
            // Prefixed + short alias pairs.
            "file_content_matches",
            "content_matches",
            "file_content_forbidden",
            "content_forbidden",
            "file_header",
            "header",
            "file_max_size",
            "max_size",
            "file_min_size",
            "min_size",
            "file_min_lines",
            "min_lines",
            "file_max_lines",
            "max_lines",
            "file_footer",
            "footer",
            "file_shebang",
            "shebang",
            // Structured-query family.
            "json_path_equals",
            "json_path_matches",
            "yaml_path_equals",
            "yaml_path_matches",
            "toml_path_equals",
            "toml_path_matches",
            "xml_path_equals",
            "xml_path_matches",
            "json_schema_passes",
            "git_no_denied_paths",
            "git_commit_message",
            "git_commit_signed_off",
            "git_commit_subject_matches",
            "git_commit_no_fixup",
            "git_commit_author_allowlist",
            "git_commit_gpg_signed",
            "git_blame_age",
            "changeset_requires_path",
            "file_is_text",
            "is_text",
            // Short-only.
            "filename_case",
            "filename_regex",
            "pair",
            "pair_changed_together",
            "pair_hash",
            "for_each_dir",
            "for_each_file",
            "dir_only_contains",
            "unique_by",
            "dir_contains",
            "every_matching_has",
            "registry_paths_resolve",
            "cross_file",
            "cross_file_value_equals",
            "file_graph",
            "ordered_block",
            "for_each_match",
            "generated_file_fresh",
            "import_gate",
            "command_idempotent",
            // Text-hygiene family.
            "no_trailing_whitespace",
            "final_newline",
            "line_endings",
            "line_max_width",
            // Security / Unicode sanity.
            "no_merge_conflict_markers",
            "no_bidi_controls",
            "no_zero_width_chars",
            // Encoding + fingerprint.
            "file_is_ascii",
            "no_bom",
            "file_hash",
            // Structure / layout.
            "max_directory_depth",
            "max_files_per_directory",
            "no_empty_files",
            // Portable metadata.
            "no_case_conflicts",
            "no_illegal_windows_names",
            // Unix metadata + git.
            "no_symlinks",
            "executable_bit",
            "executable_has_shebang",
            "shebang_has_executable",
            "no_submodules",
            // Hygiene + byte fingerprint.
            "indent_style",
            "max_consecutive_blank_lines",
            "file_starts_with",
            "file_ends_with",
            // Plugin (tier 1).
            "command",
        ] {
            assert!(
                known.contains(&kind),
                "{kind} missing from builtin registry"
            );
        }
    }

    /// The v0.10 cross-file rule kinds must opt out of
    /// `--changed` filtering (`requires_full_index() == true`)
    /// and declare no `path_scope` (so the engine never
    /// skip-by-intersects them). A refactor breaking this would
    /// silently make them miss violations in PR mode — there was
    /// no test guarding it before the v0.10 post-audit pass.
    #[test]
    fn v010_cross_file_kinds_require_full_index_and_no_path_scope() {
        use crate::test_support::spec_yaml;
        use alint_core::Rule;

        let cases: &[(&str, &str)] = &[
            (
                "registry_paths_resolve",
                "id: t\nkind: registry_paths_resolve\nsource: Cargo.toml\n\
                 extract:\n  toml: \"$.x\"\nlevel: error\n",
            ),
            (
                "cross_file",
                "id: t\nkind: cross_file\nsource:\n  file: a.json\n  \
                 extract:\n    json: \"$.x[*]\"\ntargets:\n  files: \"b/*.json\"\n  \
                 extract:\n    json: \"$.y[*]\"\nrelation: subset\nlevel: error\n",
            ),
            (
                "cross_file_value_equals",
                "id: t\nkind: cross_file_value_equals\nsource:\n  file: a.toml\n  \
                 extract:\n    toml: \"$.x\"\ntargets:\n  files: \"b/*.toml\"\n  \
                 extract:\n    toml: \"$.y\"\nlevel: error\n",
            ),
            (
                "generated_file_fresh",
                "id: t\nkind: generated_file_fresh\nfile: x\ncommand: [\"true\"]\n\
                 level: error\n",
            ),
            (
                "command_idempotent",
                "id: t\nkind: command_idempotent\ncommand: [\"true\"]\nlevel: error\n",
            ),
            (
                "pair_hash",
                "id: t\nkind: pair_hash\nsource: a\ntarget: b\nlevel: error\n",
            ),
            (
                "file_graph",
                "id: t\nkind: file_graph\nnodes: \"src/**/*.ts\"\nedges:\n  \
                 from_content:\n    extract:\n      lines: {}\nrequire: acyclic\nlevel: error\n",
            ),
        ];

        for (kind, yaml) in cases {
            let spec = spec_yaml(yaml);
            let built: alint_core::Result<Box<dyn Rule>> = match *kind {
                "registry_paths_resolve" => crate::registry_paths_resolve::build(&spec),
                "cross_file" | "cross_file_value_equals" => crate::cross_file::build(&spec),
                "generated_file_fresh" => crate::generated_file_fresh::build(&spec),
                "command_idempotent" => crate::command_idempotent::build(&spec),
                "pair_hash" => crate::pair_hash::build(&spec),
                "file_graph" => crate::file_graph::build(&spec),
                _ => unreachable!(),
            };
            let rule = built.unwrap_or_else(|e| panic!("{kind} build failed: {e}"));
            assert!(
                rule.requires_full_index(),
                "{kind}: requires_full_index() must be true (cross-file; \
                 must not be --changed-filtered)"
            );
            assert!(
                rule.path_scope().is_none(),
                "{kind}: must declare no path_scope (cross-file dispatch)"
            );
        }
    }
}
