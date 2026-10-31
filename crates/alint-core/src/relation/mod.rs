//! The internal relation layer: general-linter Phase 2 (ADR-0023), work item
//! R1 of `docs/design/v0.21/README.md`.
//!
//! Resources (index entries or literal paths), selectors that pick them,
//! adapters that turn a resource's bytes into rows, typed values that convert
//! to the plugin contract's flat form (`crate::plugin_api`), relations in
//! canonical order with provenance, absence states, cardinality, and limits. R2 puts the built-in formats on these types, and
//! `key_parity` (R3) and the shadowed `cross_file` `equals` port (R4) are
//! their first users.
//!
//! The engine's one internal trait set lives here (`Adapter` first): the
//! built-in formats implement it, and contract adapters reach it through a
//! bridge (R6a). The module is public only so `alint-rules` can build on it.
//! It is hidden from the docs, carries no semver promise until v1.0, and
//! nothing in it appears in any report or schema.

mod adapter;
mod adapters;
mod cache;
mod contract;
mod limits;
mod path;
mod pointer;
mod resource;
mod row;
mod value;

pub use adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput, check_row_count};
pub use adapters::{
    AndroidStringsAdapter, EntryRowsAdapter, ExtractAdapter, ResxAdapter, TomlRowsAdapter,
};
pub use cache::{DecodeCache, ReadCache};
pub use contract::{AnalyzeError, ContractAdapter, ContractAnalyzer, InternalContractAdapter};
pub use limits::{LimitConfigError, LimitExceeded, LimitKind, Limits, RunBudget};
pub use path::RelPath;
pub use pointer::{Pointer, Segment};
pub use resource::{Origin, Resource, ResourceId, ResourceRead, Selection, Selector};
pub use row::{Cardinality, CardinalityError, Relation, Row, RowState};
pub use value::{ContractValueError, Number, Value};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use syn::visit::{self, Visit};

    /// Invariant 6 of the v0.21 design: the layer stays out of the documented
    /// public API.
    #[test]
    fn the_module_is_hidden_from_the_docs() {
        let lib = include_str!("../lib.rs").replace("\r\n", "\n");
        assert!(
            lib.contains("#[doc(hidden)]\npub mod relation;"),
            "lib.rs must declare `#[doc(hidden)] pub mod relation;`"
        );
    }

    fn has_attr(attrs: &[syn::Attribute], name: &str, arg: &str) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident(name)
                && attr
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|ident| ident == arg)
        })
    }

    /// Whether a function names the relation layer or the plugin contract
    /// anywhere, macro arguments included.
    #[derive(Default)]
    struct Mentions(bool);

    impl<'ast> Visit<'ast> for Mentions {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            self.0 |= path
                .segments
                .iter()
                .any(|s| s.ident == "relation" || s.ident == "plugin_api");
            visit::visit_path(self, path);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            fn scan(tokens: proc_macro2::TokenStream) -> bool {
                tokens.into_iter().any(|token| match token {
                    proc_macro2::TokenTree::Ident(ident) => {
                        ident == "relation" || ident == "plugin_api"
                    }
                    proc_macro2::TokenTree::Group(group) => scan(group.stream()),
                    _ => false,
                })
            }
            self.0 |= scan(mac.tokens.clone());
            visit::visit_macro(self, mac);
        }
    }

    /// Public functions outside the hidden modules that reach them.
    struct Exposed<'a> {
        file: &'a Path,
        impl_hidden: bool,
        found: Vec<String>,
    }

    impl Exposed<'_> {
        fn check(
            &mut self,
            vis: &syn::Visibility,
            attrs: &[syn::Attribute],
            name: &syn::Ident,
            mentions: bool,
        ) {
            let hidden = self.impl_hidden || has_attr(attrs, "doc", "hidden");
            if matches!(vis, syn::Visibility::Public(_)) && mentions && !hidden {
                self.found.push(format!("{}: {name}", self.file.display()));
            }
        }
    }

    impl<'ast> Visit<'ast> for Exposed<'_> {
        fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
            if !has_attr(&module.attrs, "cfg", "test") {
                visit::visit_item_mod(self, module);
            }
        }

        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            if has_attr(&item.attrs, "cfg", "test") {
                return;
            }
            let outer = self.impl_hidden;
            self.impl_hidden = has_attr(&item.attrs, "doc", "hidden");
            visit::visit_item_impl(self, item);
            self.impl_hidden = outer;
        }

        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            let mut mentions = Mentions::default();
            mentions.visit_impl_item_fn(item);
            self.check(&item.vis, &item.attrs, &item.sig.ident, mentions.0);
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let mut mentions = Mentions::default();
            mentions.visit_item_fn(item);
            self.check(&item.vis, &item.attrs, &item.sig.ident, mentions.0);
        }
    }

    /// A public function elsewhere in the crate that returns, takes, or uses
    /// a relation or contract type is hidden too, or it would carry the
    /// hidden layer into the documented API (`FileIndex::read_cache`,
    /// `decode_cache`, and `enable_relation_caches` were not).
    #[test]
    fn public_functions_that_reach_the_layer_are_hidden_too() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![src.clone()];
        let mut found = Vec::new();
        let mut scanned = 0;
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path == src.join("relation") || path == src.join("plugin_api") {
                    continue;
                }
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    scanned += 1;
                    let text = std::fs::read_to_string(&path).unwrap();
                    let syntax = syn::parse_file(&text).unwrap();
                    let mut exposed = Exposed {
                        file: &path,
                        impl_hidden: false,
                        found: Vec::new(),
                    };
                    exposed.visit_file(&syntax);
                    found.extend(exposed.found);
                }
            }
        }
        assert!(scanned > 50, "only {scanned} sources scanned");
        assert!(
            found.is_empty(),
            "public functions reach the hidden relation layer without #[doc(hidden)]:\n{}",
            found.join("\n")
        );
    }
}
