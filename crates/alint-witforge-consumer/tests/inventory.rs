//! Every public type and trait on both sides of the contract, accounted for.
//!
//! `plugin_api.rs` converts each paired type field by field, which the
//! compiler checks, but it cannot notice an item it never names: a new WIT
//! interface (its generated trait), a new `plugin_api` type, a method with a
//! default body, or a field on a type with no counterpart. This test parses
//! the generated `plain` module and `alint_core::plugin_api`, and requires
//! each item to be listed below with its counterpart, or with the reason it
//! has none, and each pair to have the same members (fields, variants, or
//! methods). A change on either side fails here until the table, and the
//! conversions, follow it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What a generated item corresponds to in `plugin_api`.
pub(crate) enum Generated {
    /// The `plugin_api` item of this name, with the same members.
    Pair(&'static str),
    /// No `plugin_api` item: why (completing "because ..."), and the members
    /// it is expected to have.
    Unpaired(&'static str, &'static [&'static str]),
}

use Generated::{Pair, Unpaired};

pub(crate) const GENERATED: &[(&str, Generated)] = &[
    ("types::RepoResource", Pair("Resource")),
    ("types::Node", Pair("Node")),
    (
        "types::RecordField",
        Unpaired(
            "plugin_api writes a `Node::Record` entry as a (name, child) tuple",
            &["name", "child"],
        ),
    ),
    ("types::FlatValue", Pair("FlatValue")),
    ("types::PointerSegment", Pair("PointerSegment")),
    ("types::RowState", Pair("RowState")),
    ("types::Row", Pair("Row")),
    ("types::AdapterOutput", Pair("AdapterOutput")),
    ("types::AdapterInput", Pair("AdapterInput")),
    ("types::AdapterError", Pair("AdapterError")),
    (
        "types::LimitError",
        Unpaired(
            "plugin_api inlines it in `AdapterError::Limit` and `AnalyzerError::Limit`",
            &["limit", "reached"],
        ),
    ),
    ("types::RelatedLocation", Pair("RelatedLocation")),
    ("types::Finding", Pair("Finding")),
    ("types::AnalyzerInput", Pair("AnalyzerInput")),
    ("types::AnalyzerError", Pair("AnalyzerError")),
    ("types::Capabilities", Pair("Capabilities")),
    (
        "types::PluginInfo",
        Unpaired(
            "the handshake that carries it arrives with a plugin host (P2)",
            &["name", "version", "contract", "capabilities"],
        ),
    ),
    (
        "types::Types",
        Unpaired("the `types` interface declares types only", &[]),
    ),
    ("adapter_api::AdapterApi", Pair("Adapter")),
    ("analyzer_api::AnalyzerApi", Pair("Analyzer")),
];

/// `plugin_api` items with no generated counterpart, and why.
const HOST_ONLY: &[(&str, &str)] = &[
    (
        "FlatValueLimits",
        "the host's validation bounds for a flat value",
    ),
    ("FlatValueError", "why the host rejected a flat value"),
];

/// The public items in one parsed file, keyed by their module path, with their
/// members: field, variant, or trait member names (none for a type alias,
/// constant, static, or function). Every field counts, public or not: a
/// private one, like `FlatValue::nodes`, is still part of what crosses the
/// contract. Re-exports are not items of their own: `pub use`, and a
/// `pub type` naming an item through `super::`, as each generated interface
/// module does for the types it uses.
fn items(file: &syn::File) -> BTreeMap<String, BTreeSet<String>> {
    fn walk(items: &[syn::Item], prefix: &str, out: &mut BTreeMap<String, BTreeSet<String>>) {
        let public = |vis: &syn::Visibility| matches!(vis, syn::Visibility::Public(_));
        for item in items {
            let (name, members): (String, BTreeSet<String>) = match item {
                syn::Item::Mod(module) => {
                    let skip = module.attrs.iter().any(|attr| {
                        attr.path().is_ident("cfg")
                            && attr
                                .parse_args::<syn::Ident>()
                                .is_ok_and(|ident| ident == "test")
                    });
                    if let (false, Some((_, content))) = (skip, &module.content) {
                        let nested = format!("{prefix}{}::", module.ident);
                        walk(content, &nested, out);
                    }
                    continue;
                }
                syn::Item::Type(item) if public(&item.vis) => {
                    let reexport = matches!(&*item.ty, syn::Type::Path(path)
                        if path.qself.is_none()
                            && path.path.segments.first().is_some_and(|s| s.ident == "super"));
                    if reexport {
                        continue;
                    }
                    (item.ident.to_string(), BTreeSet::new())
                }
                syn::Item::Const(item) if public(&item.vis) => {
                    (item.ident.to_string(), BTreeSet::new())
                }
                syn::Item::Static(item) if public(&item.vis) => {
                    (item.ident.to_string(), BTreeSet::new())
                }
                syn::Item::Fn(item) if public(&item.vis) => {
                    (item.sig.ident.to_string(), BTreeSet::new())
                }
                syn::Item::Union(item) if public(&item.vis) => (
                    item.ident.to_string(),
                    item.fields
                        .named
                        .iter()
                        .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                        .collect(),
                ),
                syn::Item::Struct(item) if public(&item.vis) => (
                    item.ident.to_string(),
                    item.fields
                        .iter()
                        .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                        .collect(),
                ),
                syn::Item::Enum(item) if public(&item.vis) => (
                    item.ident.to_string(),
                    item.variants.iter().map(|v| v.ident.to_string()).collect(),
                ),
                syn::Item::Trait(item) if public(&item.vis) => (
                    item.ident.to_string(),
                    item.items
                        .iter()
                        .filter_map(|member| match member {
                            syn::TraitItem::Fn(method) => Some(method.sig.ident.to_string()),
                            syn::TraitItem::Type(ty) => Some(ty.ident.to_string()),
                            syn::TraitItem::Const(constant) => Some(constant.ident.to_string()),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => continue,
            };
            out.insert(format!("{prefix}{name}"), members);
        }
    }
    let mut out = BTreeMap::new();
    walk(&file.items, "", &mut out);
    out
}

fn parse(path: &Path) -> syn::File {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    syn::parse_file(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn generated_items() -> BTreeMap<String, BTreeSet<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plain/v0_1.rs");
    items(&parse(&path))
}

/// Every `.rs` file under `dir`, at any depth, sorted.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// `plugin_api`'s items, from every file under it, so a submodule in a
/// subdirectory is inventoried too.
fn api_items() -> BTreeMap<String, BTreeSet<String>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../alint-core/src/plugin_api");
    let files = rust_files(&dir)
        .into_iter()
        .map(|path| (path.display().to_string(), items(&parse(&path))));
    merge(files).unwrap_or_else(|duplicates| panic!("{}", duplicates.join("\n")))
}

/// The items of several files in one map, keyed by name as the tables pair
/// them. A name two files both declare is refused: one would hide the
/// other, and the hidden item would go unchecked.
fn merge(files: impl IntoIterator<Item = (String, Items)>) -> Result<Items, Vec<String>> {
    let mut out = Items::new();
    let mut declared_in: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates = Vec::new();
    for (file, items) in files {
        for (name, members) in items {
            if let Some(first) = declared_in.insert(name.clone(), file.clone()) {
                duplicates.push(format!(
                    "plugin_api `{name}` is declared in both {first} and {file}; the inventory \
                     keys items by name, so give one another name"
                ));
            }
            out.insert(name, members);
        }
    }
    if duplicates.is_empty() {
        Ok(out)
    } else {
        Err(duplicates)
    }
}

fn names(set: &BTreeSet<String>) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}

type Items = BTreeMap<String, BTreeSet<String>>;

/// Everything about `generated` and `api` that the tables above do not
/// account for.
fn problems(
    generated: &Items,
    api: &Items,
    table: &[(&str, Generated)],
    host_only: &[(&str, &str)],
) -> Vec<String> {
    let mut problems = Vec::new();

    let listed: BTreeSet<&str> = table.iter().map(|(path, _)| *path).collect();
    for path in generated.keys() {
        if !listed.contains(path.as_str()) {
            problems.push(format!(
                "generated `{path}` is not in GENERATED: pair it with a plugin_api item (and \
                 convert it in plugin_api.rs) or record why it has none"
            ));
        }
    }
    let mut paired = BTreeSet::new();
    for (path, counterpart) in table {
        let Some(members) = generated.get(*path) else {
            problems.push(format!(
                "GENERATED lists `{path}`, which witforge no longer generates"
            ));
            continue;
        };
        match counterpart {
            Pair(name) => {
                paired.insert(*name);
                match api.get(*name) {
                    None => problems.push(format!(
                        "`{path}` is paired with plugin_api `{name}`, which does not exist"
                    )),
                    Some(api_members) if api_members != members => problems.push(format!(
                        "`{path}` has [{}] but plugin_api `{name}` has [{}]",
                        names(members),
                        names(api_members)
                    )),
                    Some(_) => {}
                }
            }
            Unpaired(reason, expected) => {
                let expected: BTreeSet<String> = expected.iter().map(ToString::to_string).collect();
                if &expected != members {
                    problems.push(format!(
                        "`{path}` now has [{}], not [{}]; it has no plugin_api counterpart \
                         because {reason}: decide whether that still holds, then update GENERATED",
                        names(members),
                        names(&expected)
                    ));
                }
            }
        }
    }

    let host_only: BTreeSet<&str> = host_only.iter().map(|(name, _)| *name).collect();
    for name in api.keys() {
        if !paired.contains(name.as_str()) && !host_only.contains(name.as_str()) {
            problems.push(format!(
                "plugin_api `{name}` has no generated counterpart: add it to the WIT, or to \
                 HOST_ONLY with the reason it stays on the host"
            ));
        }
    }
    for name in host_only {
        if !api.contains_key(name) {
            problems.push(format!(
                "HOST_ONLY lists `{name}`, which plugin_api no longer has"
            ));
        }
        if paired.contains(name) {
            problems.push(format!("`{name}` is both paired and HOST_ONLY"));
        }
    }
    problems
}

#[test]
fn every_contract_item_is_accounted_for() {
    let generated = generated_items();
    let api = api_items();
    assert!(generated.len() > 10 && api.len() > 10, "sources not found");
    let found = problems(&generated, &api, GENERATED, HOST_ONLY);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// Each check in `problems` fires on the input it exists for.
#[test]
fn every_kind_of_unaccounted_item_is_reported() {
    let set = |members: &[&str]| members.iter().map(ToString::to_string).collect();
    let generated: Items = [
        ("types::Pair".to_string(), set(&["a", "b"])),
        ("types::Unpaired".to_string(), set(&["x"])),
    ]
    .into();
    let api: Items = [
        ("Pair".to_string(), set(&["a", "b"])),
        ("Host".to_string(), set(&[])),
    ]
    .into();
    let table: &[(&str, Generated)] = &[
        ("types::Pair", Pair("Pair")),
        ("types::Unpaired", Unpaired("it has none", &["x"])),
    ];
    let host = &[("Host", "it stays on the host")];
    assert_eq!(
        problems(&generated, &api, table, host),
        Vec::<String>::new()
    );

    let report = |generated: &Items, api: &Items, table, host| {
        let found = problems(generated, api, table, host);
        assert_eq!(found.len(), 1, "{found:?}");
        found[0].clone()
    };
    let mut extra = generated.clone();
    extra.insert("fixer_api::FixerApi".to_string(), set(&["fix"]));
    assert!(report(&extra, &api, table, host).contains("`fixer_api::FixerApi` is not in"));
    let mut gone = generated.clone();
    gone.remove("types::Unpaired");
    assert!(report(&gone, &api, table, host).contains("no longer generates"));
    let mut grown = api.clone();
    grown.insert("Pair".to_string(), set(&["a", "b", "c"]));
    assert!(
        report(&generated, &grown, table, host).contains("but plugin_api `Pair` has [a, b, c]")
    );
    let mut missing = api.clone();
    missing.remove("Pair");
    assert!(report(&generated, &missing, table, host).contains("which does not exist"));
    let mut drifted = generated.clone();
    drifted.insert("types::Unpaired".to_string(), set(&["x", "y"]));
    assert!(report(&drifted, &api, table, host).contains("because it has none"));
    let mut new_api = api.clone();
    new_api.insert("Sneaky".to_string(), set(&[]));
    assert!(report(&generated, &new_api, table, host).contains("`Sneaky` has no generated"));
    let mut no_host = api.clone();
    no_host.remove("Host");
    assert!(report(&generated, &no_host, table, host).contains("HOST_ONLY lists `Host`"));
    let both = &[("Host", "stays"), ("Pair", "both")];
    assert!(report(&generated, &api, table, both).contains("both paired and HOST_ONLY"));
}

/// Files in a subdirectory are found as well as files beside `mod.rs`.
#[test]
fn every_file_under_plugin_api_is_inventoried() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../alint-core/src/plugin_api");
    let files = rust_files(&dir);
    assert!(files.iter().any(|path| path.ends_with("mod.rs")));
    let root = std::env::temp_dir().join(format!("alint-inventory-{}", std::process::id()));
    std::fs::create_dir_all(root.join("x/z")).unwrap();
    for file in ["mod.rs", "x/mod.rs", "x/y.rs", "x/z/w.rs", "x/notes.md"] {
        std::fs::write(root.join(file), "").unwrap();
    }
    let found: Vec<String> = rust_files(&root)
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(found, ["mod.rs", "x/mod.rs", "x/y.rs", "x/z/w.rs"]);
}

/// The inventory sees each kind of drift the conversions cannot.
#[test]
fn the_inventory_sees_unnamed_items_and_members() {
    let parsed = |source: &str| items(&syn::parse_file(source).unwrap());
    let generated = parsed(
        "pub mod types { pub struct PluginInfo { pub name: String, pub extra: u32 } }
         pub mod fixer_api { pub type T = (); pub trait FixerApi { fn fix(&self); } }",
    );
    assert_eq!(
        generated.get("types::PluginInfo").map(names).as_deref(),
        Some("extra, name")
    );
    assert_eq!(
        generated.get("fixer_api::FixerApi").map(names).as_deref(),
        Some("fix")
    );
    let api = parsed(
        "pub trait Adapter { fn id(&self) -> &str; fn added(&self) -> bool { true } }
         pub struct FlatValue { nodes: Vec<u32>, limits: u32 }
         #[cfg(test)] mod tests { pub struct Ignored; }",
    );
    assert_eq!(api.get("Adapter").map(names).as_deref(), Some("added, id"));
    assert_eq!(
        api.get("FlatValue").map(names).as_deref(),
        Some("limits, nodes")
    );
    assert!(!api.contains_key("tests::Ignored"));

    // Items other than structs, enums, and traits count too; re-exports do
    // not.
    let generated = parsed(
        "pub mod types { pub struct Row; pub type Alias = u32; }
         pub mod adapter_api {
             pub type Row = super::types::Row;
             pub trait AdapterApi { type Out; const LIMIT: u32; fn run(&self); }
         }
         pub use adapter_api::AdapterApi;
         pub const VERSION: &str = \"0.1\";
         pub static NAME: &str = \"alint\";
         pub fn helper() {}
         pub union Bits { a: u32, b: f32 }
         pub(crate) fn private() {}",
    );
    assert_eq!(
        generated.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "Bits",
            "NAME",
            "VERSION",
            "adapter_api::AdapterApi",
            "helper",
            "types::Alias",
            "types::Row",
        ]
    );
    assert_eq!(
        generated
            .get("adapter_api::AdapterApi")
            .map(names)
            .as_deref(),
        Some("LIMIT, Out, run")
    );
    assert_eq!(generated.get("Bits").map(names).as_deref(), Some("a, b"));
}

#[test]
fn a_name_two_plugin_api_files_declare_is_refused() {
    let file = |name: &str, members: &[&str]| -> Items {
        [(
            name.to_string(),
            members.iter().map(ToString::to_string).collect(),
        )]
        .into()
    };
    let merged = merge([
        ("contract.rs".to_string(), file("Finding", &["message"])),
        ("value.rs".to_string(), file("Node", &["Null"])),
    ])
    .unwrap();
    assert_eq!(merged.keys().collect::<Vec<_>>(), ["Finding", "Node"]);
    let duplicates = merge([
        ("contract.rs".to_string(), file("Finding", &["message"])),
        ("aaa.rs".to_string(), file("Finding", &["engine_only"])),
    ])
    .unwrap_err();
    assert_eq!(duplicates.len(), 1, "{duplicates:?}");
    assert!(
        duplicates[0].contains("contract.rs and aaa.rs"),
        "{duplicates:?}"
    );
}
