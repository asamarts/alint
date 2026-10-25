//! Built-in rule implementations for alint.
//!
//! Rules are registered into an [`alint_core::RuleRegistry`] via
//! [`register_builtin`]. Each kind has its own submodule.

use std::fmt::Write as _;

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

/// Generates a rule kind's `options_schema()` fn: the schemars-derived JSON
/// Schema for its `Options` struct. `xtask gen-schema` composes the result with
/// the `kind` discriminator and metadata-derived common-field surface.
/// See [`rule_option_schemas`] and ADR-0001.
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

/// One policy violation for a structured extractor that selected values a
/// string-only relation cannot consume. Values themselves are deliberately
/// excluded: only stable JSON types and counts enter reports and baselines.
fn non_string_extract_violation(
    path: &std::path::Path,
    what: &str,
    kinds: &[alint_core::NonStringValueKind],
) -> Option<alint_core::Violation> {
    if kinds.is_empty() {
        return None;
    }
    let mut summary = String::new();
    for kind in [
        alint_core::NonStringValueKind::Null,
        alint_core::NonStringValueKind::Boolean,
        alint_core::NonStringValueKind::Number,
        alint_core::NonStringValueKind::Array,
        alint_core::NonStringValueKind::Object,
    ] {
        let count = kinds.iter().filter(|candidate| **candidate == kind).count();
        if count == 0 {
            continue;
        }
        if !summary.is_empty() {
            summary.push_str(", ");
        }
        write!(summary, "{}: {count}", kind.as_str()).expect("writing to String cannot fail");
    }
    Some(
        alint_core::Violation::new(format!(
            "{}: {what} matched non-string value(s) ({summary}); expected strings",
            slash(path)
        ))
        .with_path(path.to_path_buf())
        .with_baseline_key(format!("extract-non-string\u{0}{what}\u{0}{summary}"))
        .with_not_fixable(),
    )
}

/// True when `path` names something NESTED - more than one path component,
/// i.e. not directly at the repository root. The existence family
/// (`file_exists`/`file_absent`/`dir_exists`/`dir_absent`) uses this to honour
/// their `root_only:` option: when set, only root-level paths are considered.
#[must_use]
pub(crate) fn is_nested(path: &std::path::Path) -> bool {
    path.components().count() != 1
}

#[cfg(test)]
mod helper_tests {
    #[test]
    fn renders_forward_slashes_regardless_of_separator() {
        use std::path::Path;
        // On Linux `\` is a legal filename byte, so this exercises exactly the
        // replacement the fix relies on for Windows path separators.
        assert_eq!(super::slash(Path::new("a\\b\\c")), "a/b/c");
        assert_eq!(super::slash(Path::new("a/b/c")), "a/b/c");
    }

    #[test]
    fn non_string_diagnostic_is_ordered_and_never_contains_values() {
        use alint_core::NonStringValueKind::{Boolean, Number, Object};
        use std::path::Path;

        let violation = super::non_string_extract_violation(
            Path::new("config.json"),
            "registry extract",
            &[Object, Number, Boolean, Number],
        )
        .expect("non-string matches produce a violation");

        assert_eq!(
            violation.message,
            "config.json: registry extract matched non-string value(s) \
             (boolean: 1, number: 2, object: 1); expected strings"
        );
        assert!(!violation.message.contains("do-not-report"));
        assert!(
            violation.not_fixable,
            "a typed extraction mismatch cannot be repaired by the owning rule's fixer"
        );
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
pub mod editorconfig_conforms;
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
pub mod key_parity;
pub mod line_endings;
pub mod line_max_width;
mod markdown;
pub mod markdown_fragments_resolve;
pub mod markdown_images_have_alt;
pub mod markdown_links_resolve;
pub mod markdown_no_duplicate_headings;
pub mod markdown_paths_resolve;
pub mod markdown_required_headings;
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
pub mod pinned_references;
pub mod registry_paths_resolve;
pub mod shebang_has_executable;
mod spawn;
pub mod spdx_identifier_valid;
pub mod structured_path;
#[cfg(test)]
mod test_support;
pub mod unique_by;

fn optionless_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

/// Rule-kind option schemas generated from Rust types via schemars. Each entry
/// maps a `$defs/rule_<kind>` definition name to that kind's derived options
/// schema; optionless kinds share an exact empty-object schema.
///
/// `xtask gen-schema` composes these with the discriminator and metadata-derived
/// common-field restrictions. This list is complete for every canonical kind;
/// the generator's record/schema parity gate rejects omissions and extras.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn rule_option_schemas() -> Vec<(&'static str, serde_json::Value)> {
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
        ("rule_key_parity", key_parity::options_schema()),
        ("rule_unique_by", unique_by::options_schema()),
        (
            "rule_markdown_links_resolve",
            markdown_links_resolve::options_schema(),
        ),
        (
            "rule_markdown_fragments_resolve",
            markdown_fragments_resolve::options_schema(),
        ),
        (
            "rule_markdown_required_headings",
            markdown_required_headings::options_schema(),
        ),
        (
            "rule_markdown_paths_resolve",
            markdown_paths_resolve::options_schema(),
        ),
        (
            "rule_pinned_references",
            pinned_references::options_schema(),
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
        // The dotenv / properties / ini / hcl path kinds share the SAME option
        // structs as their json/yaml/toml/xml siblings, so derive their schema
        // branches here too rather than hand-maintaining them in the base -- this
        // keeps all 24 structured kinds' option field descriptions identical and
        // prevents silent drift when a shared option field is added.
        (
            "rule_dotenv_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_dotenv_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_dotenv_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_properties_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_properties_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_properties_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_ini_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_ini_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_ini_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_hcl_path_equals",
            structured_path::equals_options_schema(),
        ),
        (
            "rule_hcl_path_matches",
            structured_path::matches_options_schema(),
        ),
        (
            "rule_hcl_path_absent",
            structured_path::absent_options_schema(),
        ),
        (
            "rule_dir_only_contains",
            dir_only_contains::options_schema(),
        ),
        ("rule_file_graph", file_graph::options_schema()),
        ("rule_filename_case", filename_case::options_schema()),
        ("rule_for_each_dir", for_each_dir::options_schema()),
        ("rule_for_each_file", for_each_file::options_schema()),
        ("rule_for_each_match", for_each_match::options_schema()),
        ("rule_editorconfig_conforms", optionless_schema()),
        ("rule_executable_has_shebang", optionless_schema()),
        ("rule_file_is_text", optionless_schema()),
        ("rule_markdown_images_have_alt", optionless_schema()),
        ("rule_markdown_no_duplicate_headings", optionless_schema()),
        ("rule_final_newline", optionless_schema()),
        ("rule_no_bidi_controls", optionless_schema()),
        ("rule_no_bom", optionless_schema()),
        ("rule_no_case_conflicts", optionless_schema()),
        ("rule_no_empty_files", optionless_schema()),
        ("rule_no_illegal_windows_names", optionless_schema()),
        ("rule_no_merge_conflict_markers", optionless_schema()),
        ("rule_no_submodules", optionless_schema()),
        ("rule_no_symlinks", optionless_schema()),
        ("rule_no_trailing_whitespace", optionless_schema()),
        ("rule_no_zero_width_chars", optionless_schema()),
        ("rule_spdx_identifier_valid", optionless_schema()),
        ("rule_shebang_has_executable", optionless_schema()),
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
/// prefix - `content_matches`, `content_forbidden`, `header`,
/// `is_text`, `max_size`. Both forms resolve to the same
/// builder; new rules land under short names only.
// A flat one-line-per-kind registration table that grows with every
// rule kind; splitting it into arbitrary sub-functions would obscure
// the "every kind registered here" invariant the drift audits rely on.
#[allow(clippy::too_many_lines)]
pub fn register_builtin(registry: &mut RuleRegistry) {
    registry.register("file_exists", &file_exists::META, file_exists::build);
    registry.register("file_absent", &file_absent::META, file_absent::build);
    registry.register("dir_exists", &dir_exists::META, dir_exists::build);
    registry.register("dir_absent", &dir_absent::META, dir_absent::build);

    registry.register(
        "file_content_matches",
        &file_content_matches::META,
        file_content_matches::build,
    );
    registry.register_alias("content_matches", "file_content_matches");
    registry.register(
        "file_content_forbidden",
        &file_content_forbidden::META,
        file_content_forbidden::build,
    );
    registry.register_alias("content_forbidden", "file_content_forbidden");
    registry.register("file_header", &file_header::META, file_header::build);
    registry.register_alias("header", "file_header");
    registry.register("file_max_size", &file_max_size::META, file_max_size::build);
    registry.register_alias("max_size", "file_max_size");
    registry.register("file_min_size", &file_min_size::META, file_min_size::build);
    registry.register_alias("min_size", "file_min_size");
    registry.register(
        "file_min_lines",
        &file_min_lines::META,
        file_min_lines::build,
    );
    registry.register_alias("min_lines", "file_min_lines");
    registry.register(
        "file_max_lines",
        &file_max_lines::META,
        file_max_lines::build,
    );
    registry.register_alias("max_lines", "file_max_lines");
    registry.register("file_footer", &file_footer::META, file_footer::build);
    registry.register_alias("footer", "file_footer");
    registry.register("file_shebang", &file_shebang::META, file_shebang::build);
    registry.register_alias("shebang", "file_shebang");

    // Structured-query family — JSONPath queries over
    // JSON / YAML / TOML documents.
    registry.register(
        "json_path_equals",
        &structured_path::JSON_PATH_EQUALS_META,
        structured_path::json_path_equals_build,
    );
    registry.register(
        "json_path_matches",
        &structured_path::JSON_PATH_MATCHES_META,
        structured_path::json_path_matches_build,
    );
    registry.register(
        "yaml_path_equals",
        &structured_path::YAML_PATH_EQUALS_META,
        structured_path::yaml_path_equals_build,
    );
    registry.register(
        "yaml_path_matches",
        &structured_path::YAML_PATH_MATCHES_META,
        structured_path::yaml_path_matches_build,
    );
    registry.register(
        "toml_path_equals",
        &structured_path::TOML_PATH_EQUALS_META,
        structured_path::toml_path_equals_build,
    );
    registry.register(
        "toml_path_matches",
        &structured_path::TOML_PATH_MATCHES_META,
        structured_path::toml_path_matches_build,
    );
    registry.register(
        "xml_path_equals",
        &structured_path::XML_PATH_EQUALS_META,
        structured_path::xml_path_equals_build,
    );
    registry.register(
        "xml_path_matches",
        &structured_path::XML_PATH_MATCHES_META,
        structured_path::xml_path_matches_build,
    );
    registry.register(
        "dotenv_path_equals",
        &structured_path::DOTENV_PATH_EQUALS_META,
        structured_path::dotenv_path_equals_build,
    );
    registry.register(
        "dotenv_path_matches",
        &structured_path::DOTENV_PATH_MATCHES_META,
        structured_path::dotenv_path_matches_build,
    );
    registry.register(
        "properties_path_equals",
        &structured_path::PROPERTIES_PATH_EQUALS_META,
        structured_path::properties_path_equals_build,
    );
    registry.register(
        "properties_path_matches",
        &structured_path::PROPERTIES_PATH_MATCHES_META,
        structured_path::properties_path_matches_build,
    );
    registry.register(
        "ini_path_equals",
        &structured_path::INI_PATH_EQUALS_META,
        structured_path::ini_path_equals_build,
    );
    registry.register(
        "ini_path_matches",
        &structured_path::INI_PATH_MATCHES_META,
        structured_path::ini_path_matches_build,
    );
    registry.register(
        "hcl_path_equals",
        &structured_path::HCL_PATH_EQUALS_META,
        structured_path::hcl_path_equals_build,
    );
    registry.register(
        "hcl_path_matches",
        &structured_path::HCL_PATH_MATCHES_META,
        structured_path::hcl_path_matches_build,
    );
    // Existence assertion for the full {json,yaml,toml,xml,dotenv,properties,ini,hcl} family.
    // Symmetry with the equals/matches ops is enforced by `structured_family_is_symmetric`.
    registry.register(
        "json_path_absent",
        &structured_path::JSON_PATH_ABSENT_META,
        structured_path::json_path_absent_build,
    );
    registry.register(
        "yaml_path_absent",
        &structured_path::YAML_PATH_ABSENT_META,
        structured_path::yaml_path_absent_build,
    );
    registry.register(
        "toml_path_absent",
        &structured_path::TOML_PATH_ABSENT_META,
        structured_path::toml_path_absent_build,
    );
    registry.register(
        "xml_path_absent",
        &structured_path::XML_PATH_ABSENT_META,
        structured_path::xml_path_absent_build,
    );
    registry.register(
        "dotenv_path_absent",
        &structured_path::DOTENV_PATH_ABSENT_META,
        structured_path::dotenv_path_absent_build,
    );
    registry.register(
        "properties_path_absent",
        &structured_path::PROPERTIES_PATH_ABSENT_META,
        structured_path::properties_path_absent_build,
    );
    registry.register(
        "ini_path_absent",
        &structured_path::INI_PATH_ABSENT_META,
        structured_path::ini_path_absent_build,
    );
    registry.register(
        "hcl_path_absent",
        &structured_path::HCL_PATH_ABSENT_META,
        structured_path::hcl_path_absent_build,
    );
    registry.register(
        "json_schema_passes",
        &json_schema_passes::META,
        json_schema_passes::build,
    );
    registry.register(
        "markdown_links_resolve",
        &markdown_links_resolve::META,
        markdown_links_resolve::build,
    );
    registry.register(
        "markdown_fragments_resolve",
        &markdown_fragments_resolve::META,
        markdown_fragments_resolve::build,
    );
    registry.register_optionless(
        "markdown_images_have_alt",
        &markdown_images_have_alt::META,
        markdown_images_have_alt::build,
    );
    registry.register_optionless(
        "markdown_no_duplicate_headings",
        &markdown_no_duplicate_headings::META,
        markdown_no_duplicate_headings::build,
    );
    registry.register(
        "markdown_required_headings",
        &markdown_required_headings::META,
        markdown_required_headings::build,
    );
    registry.register(
        "pinned_references",
        &pinned_references::META,
        pinned_references::build,
    );
    registry.register_optionless(
        "editorconfig_conforms",
        &editorconfig_conforms::META,
        editorconfig_conforms::build,
    );
    registry.register_optionless(
        "spdx_identifier_valid",
        &spdx_identifier_valid::META,
        spdx_identifier_valid::build,
    );
    registry.register(
        "markdown_paths_resolve",
        &markdown_paths_resolve::META,
        markdown_paths_resolve::build,
    );
    registry.register(
        "commented_out_code",
        &commented_out_code::META,
        commented_out_code::build,
    );
    registry.register(
        "git_no_denied_paths",
        &git_no_denied_paths::META,
        git_no_denied_paths::build,
    );
    registry.register(
        "git_commit_message",
        &git_commit_message::META,
        git_commit_message::build,
    );
    registry.register(
        "git_commit_signed_off",
        &git_commit_signed_off::META,
        git_commit_signed_off::build,
    );
    registry.register(
        "git_commit_subject_matches",
        &git_commit_subject_matches::META,
        git_commit_subject_matches::build,
    );
    registry.register(
        "git_commit_no_fixup",
        &git_commit_no_fixup::META,
        git_commit_no_fixup::build,
    );
    registry.register(
        "git_commit_author_allowlist",
        &git_commit_author_allowlist::META,
        git_commit_author_allowlist::build,
    );
    registry.register(
        "git_commit_gpg_signed",
        &git_commit_gpg_signed::META,
        git_commit_gpg_signed::build,
    );
    registry.register("git_blame_age", &git_blame_age::META, git_blame_age::build);
    registry.register(
        "changeset_requires_path",
        &changeset_requires_path::META,
        changeset_requires_path::build,
    );
    registry.register_optionless("file_is_text", &file_is_text::META, file_is_text::build);
    registry.register_alias("is_text", "file_is_text");

    registry.register("filename_case", &filename_case::META, filename_case::build);
    registry.register(
        "filename_regex",
        &filename_regex::META,
        filename_regex::build,
    );
    registry.register("pair", &pair::META, pair::build);
    registry.register(
        "pair_changed_together",
        &pair_changed_together::META,
        pair_changed_together::build,
    );
    registry.register("pair_hash", &pair_hash::META, pair_hash::build);
    registry.register("for_each_dir", &for_each_dir::META, for_each_dir::build);
    registry.register("for_each_file", &for_each_file::META, for_each_file::build);
    registry.register(
        "dir_only_contains",
        &dir_only_contains::META,
        dir_only_contains::build,
    );
    registry.register("unique_by", &unique_by::META, unique_by::build);
    registry.register("key_parity", &key_parity::META, key_parity::build);
    registry.register("dir_contains", &dir_contains::META, dir_contains::build);
    registry.register(
        "every_matching_has",
        &every_matching_has::META,
        every_matching_has::build,
    );
    registry.register(
        "registry_paths_resolve",
        &registry_paths_resolve::META,
        registry_paths_resolve::build,
    );
    // The unified cross-file value-relation kind; `cross_file_value_equals`
    // (v0.10) is a byte-compatible alias with `relation` defaulting to `equals`.
    registry.register("cross_file", &cross_file::META, cross_file::build);
    registry.register_alias("cross_file_value_equals", "cross_file");
    registry.register("file_graph", &file_graph::META, file_graph::build);
    registry.register("ordered_block", &ordered_block::META, ordered_block::build);
    registry.register(
        "for_each_match",
        &for_each_match::META,
        for_each_match::build,
    );
    registry.register(
        "generated_file_fresh",
        &generated_file_fresh::META,
        generated_file_fresh::build,
    );
    registry.register("import_gate", &import_gate::META, import_gate::build);
    registry.register(
        "command_idempotent",
        &command_idempotent::META,
        command_idempotent::build,
    );

    // Text-hygiene family (short names — no `file_` prefix).
    registry.register_optionless(
        "no_trailing_whitespace",
        &no_trailing_whitespace::META,
        no_trailing_whitespace::build,
    );
    registry.register_optionless("final_newline", &final_newline::META, final_newline::build);
    registry.register("line_endings", &line_endings::META, line_endings::build);
    registry.register(
        "line_max_width",
        &line_max_width::META,
        line_max_width::build,
    );

    // Security / Unicode sanity.
    registry.register_optionless(
        "no_merge_conflict_markers",
        &no_merge_conflict_markers::META,
        no_merge_conflict_markers::build,
    );
    registry.register_optionless(
        "no_bidi_controls",
        &no_bidi_controls::META,
        no_bidi_controls::build,
    );
    registry.register_optionless(
        "no_zero_width_chars",
        &no_zero_width_chars::META,
        no_zero_width_chars::build,
    );

    // Encoding + content fingerprint.
    registry.register("file_is_ascii", &file_is_ascii::META, file_is_ascii::build);
    registry.register_optionless("no_bom", &no_bom::META, no_bom::build);
    registry.register("file_hash", &file_hash::META, file_hash::build);

    // Structure / layout.
    registry.register(
        "max_directory_depth",
        &max_directory_depth::META,
        max_directory_depth::build,
    );
    registry.register(
        "max_files_per_directory",
        &max_files_per_directory::META,
        max_files_per_directory::build,
    );
    registry.register_optionless(
        "no_empty_files",
        &no_empty_files::META,
        no_empty_files::build,
    );

    // Cross-platform / portable metadata.
    registry.register_optionless(
        "no_case_conflicts",
        &no_case_conflicts::META,
        no_case_conflicts::build,
    );
    registry.register_optionless(
        "no_illegal_windows_names",
        &no_illegal_windows_names::META,
        no_illegal_windows_names::build,
    );

    // Unix metadata + git.
    registry.register_optionless("no_symlinks", &no_symlinks::META, no_symlinks::build);
    registry.register(
        "executable_bit",
        &executable_bit::META,
        executable_bit::build,
    );
    registry.register_optionless(
        "executable_has_shebang",
        &executable_has_shebang::META,
        executable_has_shebang::build,
    );
    registry.register_optionless(
        "shebang_has_executable",
        &shebang_has_executable::META,
        shebang_has_executable::build,
    );
    registry.register_optionless("no_submodules", &no_submodules::META, no_submodules::build);

    // Hygiene + byte fingerprint.
    registry.register("indent_style", &indent_style::META, indent_style::build);
    registry.register(
        "max_consecutive_blank_lines",
        &max_consecutive_blank_lines::META,
        max_consecutive_blank_lines::build,
    );
    registry.register(
        "file_starts_with",
        &file_starts_with::META,
        file_starts_with::build,
    );
    registry.register(
        "file_ends_with",
        &file_ends_with::META,
        file_ends_with::build,
    );

    // Plugin tier 1 — shell out to an external CLI per matched
    // file. Trust-gated at config-load: only the user's own
    // top-level config can declare these.
    registry.register("command", &command::META, command::build);
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
        // The 11 known aliases: 10 short-name aliases of `file_*` kinds
        // (including `is_text`), plus `cross_file_value_equals`. A drift in
        // that count is a deliberate change.
        assert_eq!(alias_count, 11, "unexpected number of registered aliases");
    }

    #[test]
    fn generated_alias_table_matches_the_registry_exactly() {
        let registry = builtin_registry();
        let registered: std::collections::BTreeMap<&str, &str> = registry.aliases().collect();
        let generated: std::collections::BTreeMap<&str, &str> =
            crate::categories::ALIAS_TO_CANONICAL
                .iter()
                .copied()
                .collect();
        assert_eq!(
            registered, generated,
            "docs-generated aliases must match registry aliases and canonical targets"
        );
    }

    #[test]
    fn every_canonical_kind_has_exactly_one_metadata_record() {
        let registry = builtin_registry();
        let canonical: std::collections::BTreeSet<&str> = registry.canonical_kinds().collect();
        let documented: std::collections::BTreeSet<&str> = crate::categories::KIND_CATEGORIES
            .iter()
            .map(|(kind, _)| *kind)
            .collect();
        let metadata: std::collections::BTreeSet<&str> =
            registry.kind_metadata().map(|(kind, _)| kind).collect();

        assert_eq!(canonical, documented, "registry and generated docs drifted");
        assert_eq!(
            metadata, canonical,
            "every canonical builder must share its registration with one metadata record"
        );
        assert_eq!(metadata.len(), 103, "the v0.19 inventory covers 103 kinds");

        for (alias, canonical) in registry.aliases() {
            let alias_meta = registry.meta(alias).expect("registered alias has metadata");
            let canonical_meta = registry
                .meta(canonical)
                .expect("registered canonical kind has metadata");
            assert!(
                std::ptr::eq(alias_meta, canonical_meta),
                "alias `{alias}` must resolve the exact `{canonical}` record"
            );
        }
    }

    #[test]
    fn every_canonical_kind_has_valid_complete_trace_metadata() {
        use alint_core::TraceStageClass;

        let registry = builtin_registry();
        for (kind, meta) in registry.kind_metadata() {
            meta.trace.validate_for_kind(kind).unwrap();
            assert!(
                meta.trace
                    .required_stages
                    .contains(&TraceStageClass::Assertion),
                "`{kind}` must require its terminal policy assertion"
            );
            assert_eq!(
                meta.nested_rules, meta.trace.nested_evaluation,
                "`{kind}` nested capability and trace inventory drifted"
            );
            if meta.trace.emits_preview_rows {
                assert!(
                    meta.trace.allowed_stages.iter().any(|stage| matches!(
                        stage,
                        TraceStageClass::Extraction | TraceStageClass::Nested
                    )),
                    "`{kind}` advertises preview rows without an evidence stage"
                );
            }
            if !meta.selectors.is_empty() {
                assert!(
                    meta.trace
                        .allowed_stages
                        .contains(&TraceStageClass::Selection),
                    "`{kind}` declares selectors without a selection trace stage"
                );
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn metadata_inventory_matches_the_audited_v019_shape() {
        use alint_core::{
            Capability, CapabilityCondition, ContentAccess, DispatchClass, FixSpec, GitOperation,
            GitTrackedSupport, OptionPredicate, PathsSupport, ScopeFilterSupport, SinceSupport,
        };
        use std::collections::{BTreeMap, BTreeSet};

        let registry = builtin_registry();
        let records: Vec<(&str, &alint_core::KindMeta)> = registry.kind_metadata().collect();

        let count = |predicate: &dyn Fn(&alint_core::KindMeta) -> bool| {
            records.iter().filter(|(_, meta)| predicate(meta)).count()
        };
        assert_eq!(
            [
                count(&|m| matches!(m.dispatch, DispatchClass::PerFile { .. })),
                count(&|m| matches!(m.dispatch, DispatchClass::FullIndex { .. })),
                count(&|m| matches!(m.dispatch, DispatchClass::RuleMajorScoped)),
                count(&|m| matches!(m.dispatch, DispatchClass::SingleShot)),
            ],
            [56, 23, 14, 10],
            "dispatch-class inventory drifted"
        );
        assert_eq!(
            [
                count(&|m| m.paths == PathsSupport::Required),
                count(&|m| m.paths == PathsSupport::Optional),
                count(&|m| m.paths == PathsSupport::Forbidden),
            ],
            [79, 0, 24],
            "paths support inventory drifted"
        );
        assert_eq!(
            [
                count(&|m| matches!(m.scope_filter, ScopeFilterSupport::Full)),
                count(&|m| matches!(m.scope_filter, ScopeFilterSupport::AncestorOnly)),
                count(&|m| matches!(m.scope_filter, ScopeFilterSupport::Rejected { .. })),
            ],
            [73, 1, 29],
            "scope-filter inventory drifted"
        );
        assert_eq!(
            [
                count(&|m| m.git_tracked_only == GitTrackedSupport::None),
                count(&|m| m.git_tracked_only == GitTrackedSupport::FileOnly),
                count(&|m| m.git_tracked_only == GitTrackedSupport::DirAware),
            ],
            [99, 2, 2],
            "git-tracked support inventory drifted"
        );
        assert_eq!(
            [
                count(&|m| m.since == SinceSupport::None),
                count(&|m| m.since == SinceSupport::Optional),
                count(&|m| m.since == SinceSupport::Required),
            ],
            [94, 7, 2],
            "since support inventory drifted"
        );
        assert_eq!(count(&|m| m.expect_matches), 75);
        assert_eq!(count(&|m| m.nested_rules), 3);
        assert_eq!(
            [
                count(&|m| m.content == ContentAccess::PathOnly),
                count(&|m| m.content == ContentAccess::Metadata),
                count(&|m| matches!(m.content, ContentAccess::Prefix { .. })),
                count(&|m| matches!(m.content, ContentAccess::Suffix { .. })),
                count(&|m| m.content == ContentAccess::WholeFile),
                count(&|m| m.content == ContentAccess::OtherFiles),
                count(&|m| m.content == ContentAccess::Blame),
                count(&|m| m.content == ContentAccess::Delegated),
            ],
            [23, 5, 6, 1, 55, 6, 1, 6],
            "content-access inventory drifted"
        );

        let kinds_where = |predicate: &dyn Fn(&alint_core::KindMeta) -> bool| {
            records
                .iter()
                .filter_map(|(kind, meta)| predicate(meta).then_some(*kind))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            kinds_where(&|m| matches!(m.dispatch, DispatchClass::PerFile { full_index: true })),
            BTreeSet::from(["markdown_paths_resolve"])
        );
        assert_eq!(
            kinds_where(&|m| matches!(m.dispatch, DispatchClass::FullIndex { path_scope: true })),
            BTreeSet::from([
                "file_absent",
                "file_exists",
                "key_parity",
                "max_files_per_directory",
                "no_case_conflicts",
            ])
        );
        assert_eq!(
            kinds_where(&|m| matches!(m.scope_filter, ScopeFilterSupport::AncestorOnly)),
            BTreeSet::from(["dir_absent"])
        );
        assert_eq!(
            kinds_where(&|m| m.respect_gitignore),
            BTreeSet::from(["file_exists"])
        );
        assert_eq!(
            kinds_where(&|m| m.root_only),
            BTreeSet::from(["dir_absent", "dir_exists", "file_absent", "file_exists"])
        );
        assert_eq!(
            kinds_where(&|m| m.nested_rules),
            BTreeSet::from(["every_matching_has", "for_each_dir", "for_each_file"])
        );
        assert_eq!(
            kinds_where(&|m| !m.dependencies.is_empty()),
            BTreeSet::from(["json_schema_passes"])
        );
        assert_eq!(
            registry.meta("json_schema_passes").unwrap().dependencies[0].field,
            "schema_path"
        );
        let all_format_kinds = BTreeSet::from([
            "cross_file",
            "file_graph",
            "json_schema_passes",
            "registry_paths_resolve",
        ]);
        assert_eq!(
            kinds_where(&|m| m.formats.as_ref() == alint_core::Format::ALL),
            all_format_kinds
        );
        let extract_format_kinds = BTreeSet::from(["key_parity"]);
        assert_eq!(
            registry.meta("key_parity").unwrap().formats.as_ref(),
            &[
                alint_core::Format::Json,
                alint_core::Format::Yaml,
                alint_core::Format::Toml,
                alint_core::Format::Dotenv,
                alint_core::Format::Properties,
                alint_core::Format::Ini,
            ]
        );
        let mut structured_format_kinds = BTreeSet::new();
        for (prefix, format) in [
            ("json_", alint_core::Format::Json),
            ("yaml_", alint_core::Format::Yaml),
            ("toml_", alint_core::Format::Toml),
            ("xml_", alint_core::Format::Xml),
            ("dotenv_", alint_core::Format::Dotenv),
            ("properties_", alint_core::Format::Properties),
            ("ini_", alint_core::Format::Ini),
            ("hcl_", alint_core::Format::Hcl),
        ] {
            for (kind, meta) in records
                .iter()
                .filter(|(kind, _)| kind.starts_with(prefix) && kind.contains("_path_"))
            {
                assert_eq!(
                    meta.formats.as_ref(),
                    &[format],
                    "{kind}: structured kind declares the wrong parser format"
                );
                assert!(structured_format_kinds.insert(*kind));
            }
        }
        assert_eq!(structured_format_kinds.len(), 24);
        for &(kind, meta) in &records {
            if !all_format_kinds.contains(kind)
                && !structured_format_kinds.contains(kind)
                && !extract_format_kinds.contains(kind)
            {
                assert!(
                    meta.formats.is_empty(),
                    "{kind}: only structured/extract-capable kinds may declare formats"
                );
            }
        }

        let capability_kinds = |predicate: &dyn Fn(&Capability) -> bool| {
            records
                .iter()
                .filter_map(|(kind, meta)| {
                    meta.capabilities
                        .iter()
                        .any(|entry| predicate(&entry.capability))
                        .then_some(*kind)
                })
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::SpawnUserCommand)),
            BTreeSet::from(["command", "command_idempotent", "generated_file_fresh"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::HostKeyring)),
            BTreeSet::from(["git_commit_gpg_signed"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::OutOfRootRead)),
            BTreeSet::from(["json_schema_passes", "pair_hash", "registry_paths_resolve"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::WritesDuringCheck)),
            BTreeSet::from(["generated_file_fresh"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::UnixOnly)),
            BTreeSet::from([
                "executable_bit",
                "executable_has_shebang",
                "shebang_has_executable",
            ])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::EnvRead)),
            BTreeSet::from(["git_commit_message"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::EnvExport)),
            BTreeSet::from(["command", "command_idempotent", "generated_file_fresh"])
        );
        assert_eq!(
            capability_kinds(&|cap| matches!(cap, Capability::AnalysisTime)),
            BTreeSet::from(["git_blame_age"])
        );

        let git_operation_kinds = |operation: GitOperation| {
            records
                .iter()
                .filter_map(|(kind, meta)| {
                    meta.capabilities
                        .iter()
                        .any(|entry| {
                            matches!(
                                &entry.capability,
                                Capability::Git { operations } if operations.contains(&operation)
                            )
                        })
                        .then_some(*kind)
                })
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            git_operation_kinds(GitOperation::TrackedSet),
            BTreeSet::from([
                "dir_absent",
                "dir_exists",
                "file_absent",
                "file_exists",
                "git_blame_age",
                "git_no_denied_paths",
            ])
        );
        assert_eq!(
            git_operation_kinds(GitOperation::Blame),
            BTreeSet::from(["git_blame_age"])
        );
        assert_eq!(
            git_operation_kinds(GitOperation::Diff),
            BTreeSet::from([
                "changeset_requires_path",
                "git_no_denied_paths",
                "pair_changed_together",
            ])
        );
        assert_eq!(
            git_operation_kinds(GitOperation::Log),
            BTreeSet::from([
                "git_commit_author_allowlist",
                "git_commit_gpg_signed",
                "git_commit_message",
                "git_commit_no_fixup",
                "git_commit_signed_off",
                "git_commit_subject_matches",
            ])
        );
        assert_eq!(
            git_operation_kinds(GitOperation::VerifyCommit),
            BTreeSet::from(["git_commit_gpg_signed"])
        );
        assert_eq!(
            git_operation_kinds(GitOperation::Untrack),
            BTreeSet::from(["file_absent"])
        );

        let unconditional_git: BTreeSet<&str> = records
            .iter()
            .filter_map(|(kind, meta)| {
                meta.capabilities
                    .iter()
                    .any(|entry| {
                        matches!(entry.capability, Capability::Git { .. })
                            && entry.condition == CapabilityCondition::Always
                    })
                    .then_some(*kind)
            })
            .collect();
        assert_eq!(
            unconditional_git,
            BTreeSet::from([
                "changeset_requires_path",
                "git_blame_age",
                "git_commit_author_allowlist",
                "git_commit_gpg_signed",
                "git_commit_message",
                "git_commit_no_fixup",
                "git_commit_signed_off",
                "git_commit_subject_matches",
                "git_no_denied_paths",
                "pair_changed_together",
            ])
        );
        let capability_name = |capability: &Capability| match capability {
            Capability::SpawnUserCommand => "spawn_user_command",
            Capability::Git { .. } => "git",
            Capability::HostKeyring => "host_keyring",
            Capability::EnvRead => "env_read",
            Capability::EnvExport => "env_export",
            Capability::AnalysisTime => "analysis_time",
            Capability::OutOfRootRead => "out_of_root_read",
            Capability::WritesDuringCheck => "writes_during_check",
            Capability::UnixOnly => "unix_only",
        };
        let conditional_capabilities: BTreeSet<(&str, &str, &str, &str)> = records
            .iter()
            .flat_map(|(kind, meta)| {
                meta.capabilities.iter().filter_map(|entry| {
                    let (condition, target) = match &entry.condition {
                        CapabilityCondition::Always => return None,
                        CapabilityCondition::OptionSet { option } => {
                            ("option_set", option.as_ref())
                        }
                        CapabilityCondition::OptionMatches {
                            option,
                            predicate: OptionPredicate::ContainsLegacyEnvReference,
                        } => ("option_matches_legacy_env", option.as_ref()),
                        CapabilityCondition::FixOpConfigured { op } => {
                            ("fix_op_configured", op.as_ref())
                        }
                        CapabilityCondition::CommonFieldSet { field } => {
                            ("common_field_set", field.as_ref())
                        }
                    };
                    Some((*kind, capability_name(&entry.capability), condition, target))
                })
            })
            .collect();
        assert_eq!(
            conditional_capabilities,
            BTreeSet::from([
                ("dir_absent", "git", "option_set", "git_tracked_only"),
                ("dir_exists", "git", "option_set", "git_tracked_only"),
                ("file_absent", "git", "fix_op_configured", "git_untrack"),
                ("file_absent", "git", "option_set", "git_tracked_only"),
                ("file_exists", "git", "option_set", "git_tracked_only"),
                (
                    "generated_file_fresh",
                    "writes_during_check",
                    "option_set",
                    "outputs",
                ),
                (
                    "git_commit_message",
                    "env_read",
                    "option_matches_legacy_env",
                    "since",
                ),
                ("git_no_denied_paths", "git", "option_set", "since"),
                (
                    "json_schema_passes",
                    "out_of_root_read",
                    "common_field_set",
                    "allow_out_of_root",
                ),
                (
                    "pair_hash",
                    "out_of_root_read",
                    "common_field_set",
                    "allow_out_of_root",
                ),
                (
                    "registry_paths_resolve",
                    "out_of_root_read",
                    "common_field_set",
                    "allow_out_of_root",
                ),
            ])
        );

        let mut fix_hosts: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (kind, meta) in &records {
            let mut seen_fix_ops = BTreeSet::new();
            for op in meta.fix_ops.iter() {
                assert!(
                    FixSpec::ALL_OP_NAMES.contains(&op.as_ref()),
                    "{kind}: unknown metadata fix operation `{op}`"
                );
                assert!(
                    seen_fix_ops.insert(op.as_ref()),
                    "{kind}: duplicate fix operation `{op}`"
                );
                fix_hosts.entry(op.as_ref()).or_default().insert(kind);
            }
            let mut seen_selectors = BTreeSet::new();
            for selector in meta.selectors.iter() {
                assert!(
                    seen_selectors.insert(selector.name.as_ref()),
                    "{kind}: duplicate selector `{}`",
                    selector.name
                );
            }
            for (index, capability) in meta.capabilities.iter().enumerate() {
                assert!(
                    !meta.capabilities[..index].contains(capability),
                    "{kind}: duplicate capability declaration {capability:?}"
                );
                if let Capability::Git { operations } = &capability.capability {
                    for (operation_index, operation) in operations.iter().enumerate() {
                        assert!(
                            !operations[..operation_index].contains(operation),
                            "{kind}: duplicate Git operation {operation:?}"
                        );
                    }
                }
                if let CapabilityCondition::FixOpConfigured { op } = &capability.condition {
                    assert!(
                        meta.fix_ops.contains(op),
                        "{kind}: conditional capability references undeclared fix op `{op}`"
                    );
                }
            }
            for (index, format) in meta.formats.iter().enumerate() {
                assert!(
                    !meta.formats[..index].contains(format),
                    "{kind}: duplicate format {format:?}"
                );
            }
            let mut seen_dependencies = BTreeSet::new();
            for dependency in meta.dependencies.iter() {
                assert!(
                    seen_dependencies.insert(dependency.field.as_ref()),
                    "{kind}: duplicate dependency `{}`",
                    dependency.field
                );
            }
        }
        assert_eq!(
            fix_hosts.keys().copied().collect::<BTreeSet<_>>(),
            FixSpec::ALL_OP_NAMES.iter().copied().collect(),
            "the records must account for all 26 fix operations"
        );
        assert_eq!(
            fix_hosts,
            BTreeMap::from([
                (
                    "chmod",
                    BTreeSet::from(["executable_bit", "shebang_has_executable"])
                ),
                ("command", BTreeSet::from(["command"])),
                ("create_and_register", BTreeSet::from(["cross_file"])),
                ("dir_create", BTreeSet::from(["dir_exists"])),
                (
                    "file_append",
                    BTreeSet::from(["file_content_matches", "file_footer"]),
                ),
                (
                    "file_append_final_newline",
                    BTreeSet::from(["final_newline"]),
                ),
                (
                    "file_collapse_blank_lines",
                    BTreeSet::from(["max_consecutive_blank_lines"]),
                ),
                ("file_create", BTreeSet::from(["file_exists"])),
                ("file_prepend", BTreeSet::from(["file_header"])),
                (
                    "file_remove",
                    BTreeSet::from([
                        "file_absent",
                        "no_empty_files",
                        "no_submodules",
                        "no_symlinks",
                    ]),
                ),
                ("file_rename", BTreeSet::from(["filename_case"])),
                (
                    "file_normalize_line_endings",
                    BTreeSet::from(["line_endings"]),
                ),
                ("file_strip_bidi", BTreeSet::from(["no_bidi_controls"])),
                ("file_strip_bom", BTreeSet::from(["no_bom"])),
                (
                    "file_strip_zero_width",
                    BTreeSet::from(["no_zero_width_chars"]),
                ),
                (
                    "file_trim_trailing_whitespace",
                    BTreeSet::from(["no_trailing_whitespace"]),
                ),
                ("git_untrack", BTreeSet::from(["file_absent"])),
                ("indent_style", BTreeSet::from(["indent_style"])),
                ("insert_header", BTreeSet::from(["file_header"])),
                ("insert_line", BTreeSet::from(["ordered_block"])),
                ("relocate", BTreeSet::from(["file_absent"])),
                (
                    "remove_value",
                    BTreeSet::from([
                        "dotenv_path_absent",
                        "hcl_path_absent",
                        "ini_path_absent",
                        "json_path_absent",
                        "properties_path_absent",
                        "toml_path_absent",
                        "xml_path_absent",
                        "yaml_path_absent",
                    ]),
                ),
                (
                    "replace",
                    BTreeSet::from([
                        "dotenv_path_matches",
                        "file_content_forbidden",
                        "hcl_path_matches",
                        "ini_path_matches",
                        "json_path_matches",
                        "properties_path_matches",
                        "toml_path_matches",
                        "xml_path_matches",
                        "yaml_path_matches",
                    ]),
                ),
                (
                    "set_value",
                    BTreeSet::from([
                        "dotenv_path_equals",
                        "hcl_path_equals",
                        "ini_path_equals",
                        "json_path_equals",
                        "properties_path_equals",
                        "toml_path_equals",
                        "xml_path_equals",
                        "yaml_path_equals",
                    ]),
                ),
                ("sort", BTreeSet::from(["ordered_block"])),
                ("sync_from", BTreeSet::from(["cross_file"])),
            ])
        );
    }
    #[test]
    fn glob_selector_metadata_matches_the_transition_table() {
        use alint_core::SelectorType;
        use std::collections::{BTreeMap, BTreeSet};

        let registry = builtin_registry();
        let mut declared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (kind, meta) in registry.kind_metadata() {
            for selector in meta.selectors.iter().filter(|selector| {
                matches!(
                    selector.selector_type,
                    SelectorType::Glob
                        | SelectorType::GlobList
                        | SelectorType::BasenameGlob
                        | SelectorType::DiffGlob
                )
            }) {
                declared
                    .entry(kind.to_string())
                    .or_default()
                    // The transition table walks sequences implicitly; metadata
                    // spells that traversal explicitly as `[]`.
                    .insert(selector.name.replace("[]", ""));
            }
        }

        let mut transition: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (spellings, fields) in alint_core::template::NESTED_GLOB_OPTIONS {
            for spelling in *spellings {
                let canonical = registry.canonical_kind(spelling);
                transition
                    .entry(canonical.to_string())
                    .or_default()
                    .extend(fields.iter().map(|field| (*field).to_string()));
            }
        }

        assert_eq!(
            declared, transition,
            "typed selector records and the one-release transition table must agree"
        );
    }

    #[test]
    fn regex_select_metadata_covers_the_nested_config_exemptions() {
        use alint_core::SelectorType;
        use std::collections::BTreeSet;

        let registry = builtin_registry();
        let kinds: BTreeSet<&str> = registry
            .kind_metadata()
            .filter_map(|(kind, meta)| {
                meta.selectors
                    .iter()
                    .any(|selector| {
                        selector.name == "select" && selector.selector_type == SelectorType::Regex
                    })
                    .then_some(kind)
            })
            .collect();
        assert_eq!(kinds, BTreeSet::from(["for_each_match", "ordered_block"]));
    }

    #[test]
    fn structured_family_is_symmetric() {
        // Every config-format-specific op (`<fmt>_path_<op>`) must exist for ALL
        // formats. Guards against shipping an asymmetric structured-query family
        // -- e.g. a lone `yaml_path_absent` with no json/toml/xml siblings (the
        // gap that motivated this test). Adding a new op or a new format means
        // adding every (format, op) pair, or this fails.
        //
        // The format list is DERIVED from `Format::ALL` (the enum SSOT), not
        // hardcoded: a 9th format added to `Format::ALL` is then automatically
        // required to carry all three ops. A hardcoded list could silently omit
        // the new format, and the extract/target/hint parity gates check wiring,
        // not kind existence, so none of them would catch an asymmetric family.
        let formats: Vec<String> = alint_core::Format::ALL
            .iter()
            .map(|f| f.label().to_lowercase())
            .collect();
        let r = builtin_registry();
        let known: std::collections::HashSet<&str> = r.known_kinds().collect();
        // Discover ops: any `<fmt>_path_<op>` kind contributes the suffix `path_<op>`.
        let mut ops: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for kind in &known {
            for fmt in &formats {
                if let Some(rest) = kind
                    .strip_prefix(fmt.as_str())
                    .and_then(|r| r.strip_prefix('_'))
                    && rest.starts_with("path_")
                {
                    ops.insert(rest);
                }
            }
        }
        assert!(
            !ops.is_empty(),
            "no structured-query ops discovered -- the test is vacuous (naming scheme change?)"
        );
        let mut missing = Vec::new();
        for op in &ops {
            for fmt in &formats {
                let kind = format!("{fmt}_{op}");
                if !known.contains(kind.as_str()) {
                    missing.push(kind);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "structured-query family is asymmetric -- every op must exist for all of \
             {formats:?}. Missing: {missing:?}"
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
    /// silently make them miss violations in PR mode - there was
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
            (
                "for_each_dir",
                "id: t\nkind: for_each_dir\nselect: \"src/*\"\nrequire:\n  \
                 - kind: file_exists\n    paths: \"{dir}/mod.rs\"\nlevel: error\n",
            ),
            (
                "for_each_file",
                "id: t\nkind: for_each_file\nselect: \"**/*.c\"\nrequire:\n  \
                 - kind: file_exists\n    paths: \"{dir}/{stem}.h\"\nlevel: error\n",
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
                "for_each_dir" => crate::for_each_dir::build(&spec),
                "for_each_file" => crate::for_each_file::build(&spec),
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
