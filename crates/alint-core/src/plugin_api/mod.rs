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
    Capabilities, Finding, PointerSegment, RelatedLocation, Resource, Row, RowState,
};
pub use value::{FlatValue, FlatValueError, FlatValueLimits, Node};

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use proc_macro2::{Spacing, TokenStream, TokenTree};
    use syn::visit::{self, Visit};

    /// Roots a path may start with to reach the standard library.
    const STD_ROOTS: &[&str] = &["std", "core", "alloc"];
    /// Names usable without an import: the prelude, the primitive types, and
    /// the built-in derives.
    const PRELUDE: &[&str] = &[
        "Option",
        "Some",
        "None",
        "Result",
        "Ok",
        "Err",
        "Vec",
        "String",
        "Box",
        "ToString",
        "ToOwned",
        "Clone",
        "Copy",
        "Default",
        "Debug",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "Hash",
        "Iterator",
        "IntoIterator",
        "DoubleEndedIterator",
        "ExactSizeIterator",
        "FromIterator",
        "Extend",
        "From",
        "Into",
        "TryFrom",
        "TryInto",
        "AsRef",
        "AsMut",
        "Send",
        "Sync",
        "Sized",
        "Drop",
        "Fn",
        "FnMut",
        "FnOnce",
        "bool",
        "char",
        "str",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "f32",
        "f64",
    ];
    /// Built-in attributes; any other attribute is a macro from outside the
    /// module.
    const ATTRIBUTES: &[&str] = &[
        "doc",
        "derive",
        "cfg",
        "must_use",
        "non_exhaustive",
        "allow",
        "expect",
        "warn",
        "deny",
        "inline",
        "repr",
        "track_caller",
        "deprecated",
    ];

    fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|ident| ident == "test")
        })
    }

    fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
        match item {
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::ExternCrate(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::ForeignMod(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Macro(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::TraitAlias(i) => &i.attrs,
            syn::Item::Type(i) => &i.attrs,
            syn::Item::Union(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            _ => &[],
        }
    }

    /// The names the module declares or imports, and its generic
    /// parameters: a path may start with any of them.
    #[derive(Default)]
    struct Names(BTreeSet<String>);

    impl<'ast> Visit<'ast> for Names {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let ident = match item {
                syn::Item::Const(i) => Some(&i.ident),
                syn::Item::Enum(i) => Some(&i.ident),
                syn::Item::Fn(i) => Some(&i.sig.ident),
                syn::Item::Macro(i) => i.ident.as_ref(),
                syn::Item::Mod(i) => Some(&i.ident),
                syn::Item::Static(i) => Some(&i.ident),
                syn::Item::Struct(i) => Some(&i.ident),
                syn::Item::Trait(i) => Some(&i.ident),
                syn::Item::Type(i) => Some(&i.ident),
                syn::Item::Union(i) => Some(&i.ident),
                _ => None,
            };
            self.0.extend(ident.map(ToString::to_string));
            visit::visit_item(self, item);
        }

        fn visit_use_name(&mut self, name: &'ast syn::UseName) {
            self.0.insert(name.ident.to_string());
        }

        fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
            self.0.insert(rename.rename.to_string());
        }

        fn visit_type_param(&mut self, param: &'ast syn::TypeParam) {
            self.0.insert(param.ident.to_string());
            visit::visit_type_param(self, param);
        }
    }

    /// Every path in one file of the module, outside `#[cfg(test)]` items:
    /// expression, type, pattern, and trait paths, `use` trees, attributes and
    /// derives, and the paths inside macro arguments.
    struct Boundary<'a> {
        file: &'a str,
        /// How many leading `super`s stay inside `plugin_api` here.
        depth: usize,
        names: &'a BTreeSet<String>,
        /// The names the `use` item being checked introduces, which cannot
        /// vouch for its own path (`use serde;`).
        introducing: BTreeSet<String>,
        problems: Vec<String>,
    }

    impl Boundary<'_> {
        fn check(&mut self, leading_colon: bool, segments: &[String]) {
            let Some(root) = segments.first().map(String::as_str) else {
                return;
            };
            let allowed = if leading_colon {
                STD_ROOTS.contains(&root)
            } else {
                match root {
                    "crate" => segments.get(1).is_some_and(|next| next == "plugin_api"),
                    "super" => segments.iter().take_while(|s| *s == "super").count() <= self.depth,
                    "self" | "Self" => true,
                    _ => {
                        STD_ROOTS.contains(&root)
                            || PRELUDE.contains(&root)
                            || (self.names.contains(root) && !self.introducing.contains(root))
                    }
                }
            };
            if !allowed {
                let colon = if leading_colon { "::" } else { "" };
                self.problems
                    .push(format!("{}: {colon}{}", self.file, segments.join("::")));
            }
        }

        fn check_path(&mut self, path: &syn::Path) {
            let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            // A single name is a local item, a prelude name, or a macro: an
            // outside one would need a `use`, which is checked.
            if path.leading_colon.is_some() || segments.len() > 1 {
                self.check(path.leading_colon.is_some(), &segments);
            }
        }

        fn check_use(
            &mut self,
            leading_colon: bool,
            prefix: &mut Vec<String>,
            tree: &syn::UseTree,
        ) {
            match tree {
                syn::UseTree::Path(path) => {
                    prefix.push(path.ident.to_string());
                    self.check_use(leading_colon, prefix, &path.tree);
                    prefix.pop();
                }
                syn::UseTree::Name(name) => {
                    prefix.push(name.ident.to_string());
                    self.check(leading_colon, prefix);
                    prefix.pop();
                }
                syn::UseTree::Rename(rename) => {
                    prefix.push(rename.ident.to_string());
                    self.check(leading_colon, prefix);
                    prefix.pop();
                }
                syn::UseTree::Glob(_) => self.check(leading_colon, prefix),
                syn::UseTree::Group(group) => {
                    for tree in &group.items {
                        self.check_use(leading_colon, prefix, tree);
                    }
                }
            }
        }

        /// The paths in a macro's argument tokens, which `syn` leaves
        /// unparsed: each `a::b` chain, and a leading `::a`.
        fn check_tokens(&mut self, tokens: TokenStream) {
            let tokens: Vec<TokenTree> = tokens.into_iter().collect();
            let colons_at = |at: usize| {
                matches!(
                    (tokens.get(at), tokens.get(at + 1)),
                    (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                        if a.as_char() == ':' && a.spacing() == Spacing::Joint && b.as_char() == ':'
                )
            };
            let mut at = 0;
            while at < tokens.len() {
                match &tokens[at] {
                    TokenTree::Group(group) => self.check_tokens(group.stream()),
                    TokenTree::Ident(ident)
                        if colons_at(at + 1) || (at >= 2 && colons_at(at - 2)) =>
                    {
                        let leading_colon = at >= 2
                            && colons_at(at - 2)
                            && !matches!(tokens.get(at.wrapping_sub(3)), Some(TokenTree::Ident(_)))
                            && !matches!(tokens.get(at.wrapping_sub(3)), Some(TokenTree::Punct(p)) if p.as_char() == '>');
                        let continues = at >= 2 && colons_at(at - 2) && !leading_colon;
                        let mut segments = vec![ident.to_string()];
                        let mut next = at + 1;
                        while colons_at(next) {
                            let Some(TokenTree::Ident(ident)) = tokens.get(next + 2) else {
                                break;
                            };
                            segments.push(ident.to_string());
                            next += 3;
                        }
                        if !continues {
                            self.check(leading_colon, &segments);
                        }
                        at = next;
                        continue;
                    }
                    _ => {}
                }
                at += 1;
            }
        }
    }

    impl<'ast> Visit<'ast> for Boundary<'_> {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            if is_cfg_test(item_attrs(item)) {
                return;
            }
            visit::visit_item(self, item);
        }

        fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
            self.depth += 1;
            visit::visit_item_mod(self, module);
            self.depth -= 1;
        }

        fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
            self.problems
                .push(format!("{}: extern crate {}", self.file, item.ident));
        }

        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            let mut introduced = Names::default();
            introduced.visit_item_use(item);
            self.introducing = introduced.0;
            self.check_use(item.leading_colon.is_some(), &mut Vec::new(), &item.tree);
            self.introducing.clear();
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            self.check_path(path);
            visit::visit_path(self, path);
        }

        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            let builtin = ATTRIBUTES.iter().any(|name| attr.path().is_ident(name));
            if !builtin {
                let name: Vec<String> = attr
                    .path()
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                self.problems
                    .push(format!("{}: #[{}]", self.file, name.join("::")));
            } else if attr.path().is_ident("derive") {
                let derives = attr
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    )
                    .expect("a derive list parses");
                for derive in derives {
                    let segments: Vec<String> = derive
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect();
                    if derive.leading_colon.is_none() && segments.len() == 1 {
                        if !PRELUDE.contains(&segments[0].as_str()) {
                            self.problems
                                .push(format!("{}: derive({})", self.file, segments[0]));
                        }
                    } else {
                        self.check(derive.leading_colon.is_some(), &segments);
                    }
                }
            }
            visit::visit_attribute(self, attr);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.check_tokens(mac.tokens.clone());
            visit::visit_macro(self, mac);
        }
    }

    fn problems(
        file: &str,
        source: &str,
        depth: usize,
        extra_names: &BTreeSet<String>,
    ) -> Vec<String> {
        let syntax = syn::parse_file(source).expect("plugin_api source parses");
        let mut names = Names::default();
        names.visit_file(&syntax);
        names.0.extend(extra_names.iter().cloned());
        let mut boundary = Boundary {
            file,
            depth,
            names: &names.0,
            introducing: BTreeSet::new(),
            problems: Vec::new(),
        };
        boundary.visit_file(&syntax);
        boundary.problems
    }

    /// Every `.rs` file under `dir`, at any depth, as its `/`-separated path
    /// relative to `dir` and its module depth below `dir/mod.rs`: `mod.rs` is
    /// 0, `a.rs` and `a/mod.rs` are 1, `a/b.rs` is 2.
    fn rust_files(dir: &Path) -> Vec<(String, usize)> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, usize)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    walk(&path, &format!("{prefix}{name}/"), out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let relative = format!("{prefix}{name}");
                    let components = relative.split('/').count();
                    out.push((relative, components - usize::from(name == "mod.rs")));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, "", &mut out);
        out.sort();
        out
    }

    /// The module's sources with their depths, from every file under
    /// `src/plugin_api`, so a submodule in a subdirectory is checked too.
    fn module_sources() -> Vec<(String, usize, String)> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugin_api");
        rust_files(&dir)
            .into_iter()
            .map(|(file, depth)| {
                let source = std::fs::read_to_string(dir.join(&file)).unwrap();
                (file, depth, source)
            })
            .collect()
    }

    #[test]
    fn every_file_under_the_module_is_found_with_its_depth() {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "mod.rs",
            "a.rs",
            "x/mod.rs",
            "x/y.rs",
            "x/z/w.rs",
            "x/notes.md",
        ] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        assert_eq!(
            rust_files(dir.path()),
            [
                ("a.rs", 1),
                ("mod.rs", 0),
                ("x/mod.rs", 1),
                ("x/y.rs", 2),
                ("x/z/w.rs", 3),
            ]
            .map(|(file, depth)| (file.to_string(), depth))
        );
    }

    /// Nothing in this module names anything outside it except the standard
    /// library, so no engine or third-party type can cross the contract. The
    /// module's sources are parsed and every path is visited; `#[cfg(test)]`
    /// items never ship and are skipped.
    #[test]
    fn imports_nothing_but_std_and_itself() {
        let sources = module_sources();
        assert!(sources.len() >= 3, "plugin_api sources not found");
        let mut names = Names::default();
        for (_, _, source) in &sources {
            names.visit_file(&syn::parse_file(source).unwrap());
        }
        let mut found = Vec::new();
        for (file, depth, source) in &sources {
            found.extend(problems(file, source, *depth, &names.0));
        }
        assert!(
            found.is_empty(),
            "plugin_api names items outside itself and std:\n{}",
            found.join("\n")
        );
    }

    /// The paths a line-based scan missed are caught, and the paths the
    /// module legitimately uses are not.
    #[test]
    fn the_boundary_check_sees_every_kind_of_path() {
        let none = BTreeSet::new();
        let escapes = [
            "fn f() -> super::super::engine::Row { todo!() }",
            "#[derive(Clone, serde::Serialize)] struct A;",
            "#[derive(Serialize)] struct A;",
            "#[serde(rename = \"a\")] struct A;",
            "use\n    crate::relation::Row;",
            "use crate::{plugin_api::Node, relation::Row};",
            "use serde;",
            "fn f() { let _ = crate::relation::Row::new; }",
            "fn f() { let _ = vec![serde_json::Value::Null]; }",
            "fn f() { assert!(matches!(x, ::regex::Regex { .. })); }",
            "fn f(x: ::serde::de::IgnoredAny) {}",
            "extern crate serde;",
            "impl serde::Serialize for A {}",
            "mod inner { fn f() -> super::super::super::X { todo!() } }",
        ];
        for source in escapes {
            assert!(
                !problems("child.rs", source, 1, &none).is_empty(),
                "not caught: {source}"
            );
        }
        assert!(
            !problems("mod.rs", "use super::relation::Row;", 0, &none).is_empty(),
            "`super` from plugin_api itself is the crate root"
        );
        let allowed = "
            use std::fmt;
            use fmt::Write as _;
            use super::FlatValue;
            use crate::plugin_api::Node;
            #[derive(Clone, Debug, PartialEq, Eq)]
            #[non_exhaustive]
            pub enum E { A(Box<str>), B }
            impl fmt::Display for E {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, \"{}\", Self::B as u8)?;
                    let _ = vec![E::B, std::mem::take(&mut E::B)];
                    let _: Option<FlatValue> = None;
                    let _ = u32::try_from(1usize);
                    Ok(())
                }
            }
            impl std::error::Error for E {}
            fn g<T: Iterator>(t: T) -> Option<T::Item> { let _ = Node::Null; None }
            mod inner { use super::super::FlatValue; }
            #[cfg(test)]
            mod tests { use serde::Serialize; }
        ";
        assert_eq!(
            problems("child.rs", allowed, 1, &none),
            Vec::<String>::new()
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
