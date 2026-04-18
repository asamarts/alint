//! Built-in rule implementations for alint.
//!
//! Rules are registered into an [`alint_core::RuleRegistry`] via
//! [`register_builtin`]. Each kind has its own submodule.

use alint_core::RuleRegistry;

pub mod case;
pub mod dir_absent;
pub mod dir_exists;
pub mod file_absent;
pub mod file_content_forbidden;
pub mod file_content_matches;
pub mod file_exists;
pub mod file_header;
pub mod file_is_text;
pub mod file_max_size;
pub mod filename_case;
pub mod filename_regex;
pub mod io;

/// Register every built-in rule kind into the given registry.
pub fn register_builtin(registry: &mut RuleRegistry) {
    registry.register("file_exists", file_exists::build);
    registry.register("file_absent", file_absent::build);
    registry.register("dir_exists", dir_exists::build);
    registry.register("dir_absent", dir_absent::build);
    registry.register("file_content_matches", file_content_matches::build);
    registry.register("file_content_forbidden", file_content_forbidden::build);
    registry.register("file_header", file_header::build);
    registry.register("file_max_size", file_max_size::build);
    registry.register("file_is_text", file_is_text::build);
    registry.register("filename_case", filename_case::build);
    registry.register("filename_regex", filename_regex::build);
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
            "file_exists",
            "file_absent",
            "dir_exists",
            "dir_absent",
            "file_content_matches",
            "file_content_forbidden",
            "file_header",
            "file_max_size",
            "file_is_text",
            "filename_case",
            "filename_regex",
        ] {
            assert!(known.contains(&kind), "{kind} missing from builtin registry");
        }
    }
}
