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
use std::path::Path;

/// What a generated item corresponds to in `plugin_api`.
enum Generated {
    /// The `plugin_api` item of this name, with the same members.
    Pair(&'static str),
    /// No `plugin_api` item: why (completing "because ..."), and the members
    /// it is expected to have.
    Unpaired(&'static str, &'static [&'static str]),
}

use Generated::{Pair, Unpaired};

const GENERATED: &[(&str, Generated)] = &[
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

/// The public structs, enums, and traits in one parsed file, keyed by their
/// module path, with their members: field, variant, or method names. Every
/// field counts, public or not: a private one, like `FlatValue::nodes`, is
/// still part of what crosses the contract.
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

fn api_items() -> BTreeMap<String, BTreeSet<String>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../alint-core/src/plugin_api");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            out.extend(items(&parse(&path)));
        }
    }
    out
}

fn names(set: &BTreeSet<String>) -> String {
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}

#[test]
fn every_contract_item_is_accounted_for() {
    let generated = generated_items();
    let api = api_items();
    assert!(generated.len() > 10 && api.len() > 10, "sources not found");
    let mut problems = Vec::new();

    let listed: BTreeSet<&str> = GENERATED.iter().map(|(path, _)| *path).collect();
    for path in generated.keys() {
        if !listed.contains(path.as_str()) {
            problems.push(format!(
                "generated `{path}` is not in GENERATED: pair it with a plugin_api item (and \
                 convert it in plugin_api.rs) or record why it has none"
            ));
        }
    }
    let mut paired = BTreeSet::new();
    for (path, counterpart) in GENERATED {
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

    let host_only: BTreeSet<&str> = HOST_ONLY.iter().map(|(name, _)| *name).collect();
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

    assert!(problems.is_empty(), "{}", problems.join("\n"));
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
}
