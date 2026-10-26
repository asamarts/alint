//! The plugin contract `0.x` (general-linter Phase 2, ADR-0023 and ADR-0024):
//! the types a plugin and the engine exchange, and, from work item R6a, the
//! `Adapter` and `Analyzer` contract traits.
//!
//! Only contract types live here, never engine internals: this module may
//! import nothing but `std`, `core`, `alloc`, and itself, which a test below
//! enforces. `crate::relation` converts between these types and the engine's
//! own. Public only so other crates can reach it; hidden from the docs, with
//! no semver promise until the contract reaches 1.0. It moves into a published
//! `alint-plugin-api` crate with slice P2.

mod contract;
mod value;

pub use contract::{
    Adapter, AdapterError, AdapterInput, AdapterOutput, Analyzer, AnalyzerError, AnalyzerInput,
    Capabilities, Finding, Limits, PointerSegment, RelatedLocation, Resource, Row, RowState,
};
pub use value::{FlatValue, FlatValueError, FlatValueLimits, Node};

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// `line` with the contents of its string literals removed.
    fn without_strings(line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut in_string = false;
        let mut escaped = false;
        for c in line.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                    out.push(c);
                }
            } else {
                if c == '"' {
                    in_string = true;
                }
                out.push(c);
            }
        }
        out
    }

    /// Every `use` and every `crate::` path in this module stays inside it or
    /// in the standard library, so no engine type can cross the contract.
    #[test]
    fn imports_nothing_but_std_and_itself() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugin_api");
        let mut problems = Vec::new();
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            checked += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            for (number, line) in text.lines().enumerate() {
                let code = without_strings(line.split("//").next().unwrap_or_default());
                let code = code.trim();
                let allowed = |p: &str| {
                    [
                        "std::",
                        "core::",
                        "alloc::",
                        "self::",
                        "super::",
                        "crate::plugin_api",
                        "contract::",
                        "value::",
                    ]
                    .iter()
                    .any(|prefix| p.starts_with(prefix))
                };
                if let Some(imported) = code
                    .strip_prefix("use ")
                    .or_else(|| code.strip_prefix("pub use "))
                {
                    let root = imported.trim_start_matches('{').trim();
                    if !allowed(root) {
                        problems.push(format!("{}:{}: {code}", path.display(), number + 1));
                    }
                }
                let mut rest = code;
                while let Some(at) = rest.find("crate::") {
                    let tail = &rest[at..];
                    let path_after = &tail["crate::".len()..];
                    if path_after != "plugin_api" && !path_after.starts_with("plugin_api::") {
                        problems.push(format!("{}:{}: {code}", path.display(), number + 1));
                        break;
                    }
                    rest = &tail["crate::".len()..];
                }
            }
        }
        assert!(checked >= 2, "plugin_api sources not found");
        assert!(
            problems.is_empty(),
            "engine imports in plugin_api:\n{}",
            problems.join("\n")
        );
    }

    #[test]
    fn the_module_is_hidden_from_the_docs() {
        let lib = include_str!("../lib.rs").replace("\r\n", "\n");
        assert!(
            lib.contains("#[doc(hidden)]\npub mod plugin_api;"),
            "lib.rs must declare `#[doc(hidden)] pub mod plugin_api;`"
        );
    }
}
