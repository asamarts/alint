//! Built-in rule implementations for alint.
//!
//! Rules are registered into an [`alint_core::RuleRegistry`] via
//! [`register_builtin`]. Each kind has its own submodule.

use alint_core::RuleRegistry;

pub mod case;
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
pub mod file_exists;
pub mod file_hash;
pub mod file_header;
pub mod file_is_ascii;
pub mod file_is_text;
pub mod file_max_size;
pub mod filename_case;
pub mod filename_regex;
pub mod final_newline;
pub mod fixers;
pub mod for_each_dir;
pub mod for_each_file;
pub mod io;
pub mod line_endings;
pub mod line_max_width;
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
pub mod pair;
pub mod shebang_has_executable;
pub mod unique_by;

/// Register every built-in rule kind into the given registry.
///
/// Naming convention: rules that have a `dir_*` sibling keep
/// their `file_*` prefix (`file_exists` vs `dir_exists`); rules
/// with no such parallel also register a short alias without the
/// prefix — `content_matches`, `content_forbidden`, `header`,
/// `is_text`, `max_size`. Both forms resolve to the same
/// builder; new rules land under short names only.
pub fn register_builtin(registry: &mut RuleRegistry) {
    registry.register("file_exists", file_exists::build);
    registry.register("file_absent", file_absent::build);
    registry.register("dir_exists", dir_exists::build);
    registry.register("dir_absent", dir_absent::build);

    registry.register("file_content_matches", file_content_matches::build);
    registry.register("content_matches", file_content_matches::build);
    registry.register("file_content_forbidden", file_content_forbidden::build);
    registry.register("content_forbidden", file_content_forbidden::build);
    registry.register("file_header", file_header::build);
    registry.register("header", file_header::build);
    registry.register("file_max_size", file_max_size::build);
    registry.register("max_size", file_max_size::build);
    registry.register("file_is_text", file_is_text::build);
    registry.register("is_text", file_is_text::build);

    registry.register("filename_case", filename_case::build);
    registry.register("filename_regex", filename_regex::build);
    registry.register("pair", pair::build);
    registry.register("for_each_dir", for_each_dir::build);
    registry.register("for_each_file", for_each_file::build);
    registry.register("dir_only_contains", dir_only_contains::build);
    registry.register("unique_by", unique_by::build);
    registry.register("dir_contains", dir_contains::build);
    registry.register("every_matching_has", every_matching_has::build);

    // Text-hygiene family (short names — no `file_` prefix).
    registry.register("no_trailing_whitespace", no_trailing_whitespace::build);
    registry.register("final_newline", final_newline::build);
    registry.register("line_endings", line_endings::build);
    registry.register("line_max_width", line_max_width::build);

    // Security / Unicode sanity.
    registry.register(
        "no_merge_conflict_markers",
        no_merge_conflict_markers::build,
    );
    registry.register("no_bidi_controls", no_bidi_controls::build);
    registry.register("no_zero_width_chars", no_zero_width_chars::build);

    // Encoding + content fingerprint.
    registry.register("file_is_ascii", file_is_ascii::build);
    registry.register("no_bom", no_bom::build);
    registry.register("file_hash", file_hash::build);

    // Structure / layout.
    registry.register("max_directory_depth", max_directory_depth::build);
    registry.register("max_files_per_directory", max_files_per_directory::build);
    registry.register("no_empty_files", no_empty_files::build);

    // Cross-platform / portable metadata.
    registry.register("no_case_conflicts", no_case_conflicts::build);
    registry.register("no_illegal_windows_names", no_illegal_windows_names::build);

    // Unix metadata + git.
    registry.register("no_symlinks", no_symlinks::build);
    registry.register("executable_bit", executable_bit::build);
    registry.register("executable_has_shebang", executable_has_shebang::build);
    registry.register("shebang_has_executable", shebang_has_executable::build);
    registry.register("no_submodules", no_submodules::build);
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
            "file_is_text",
            "is_text",
            // Short-only.
            "filename_case",
            "filename_regex",
            "pair",
            "for_each_dir",
            "for_each_file",
            "dir_only_contains",
            "unique_by",
            "dir_contains",
            "every_matching_has",
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
        ] {
            assert!(
                known.contains(&kind),
                "{kind} missing from builtin registry"
            );
        }
    }
}
