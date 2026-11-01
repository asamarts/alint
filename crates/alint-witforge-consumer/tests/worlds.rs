//! Every world of the contract, accounted for.
//!
//! witforge 0.1 checks a world's imports and exports but generates nothing
//! from them, so the generated code and `inventory.rs` cannot see a world
//! that is added, removed, or made to import or export something else. This
//! test reads the worlds from the WIT itself, requires each to be listed below
//! with what it imports and exports, and requires the generated trait of each
//! interface it exports to be paired, in `inventory.rs`, with the
//! `plugin_api` trait a plugin of that kind implements.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::inventory::{GENERATED, Generated};

/// One world: the interfaces it imports and exports, and the `plugin_api`
/// trait its exports are implemented through.
struct World {
    name: &'static str,
    imports: &'static [&'static str],
    exports: &'static [&'static str],
    api: &'static str,
}

const WORLDS: &[World] = &[
    World {
        name: "adapter",
        imports: &["types"],
        exports: &["adapter-api"],
        api: "Adapter",
    },
    World {
        name: "analyzer",
        imports: &["types"],
        exports: &["analyzer-api"],
        api: "Analyzer",
    },
];

/// A world's imports and exports, by interface name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Items {
    imports: BTreeSet<String>,
    exports: BTreeSet<String>,
}

/// WIT source with its comments removed, as tokens: names (with the `-`,
/// `_`, `.`, and `%` they may contain) and single punctuation characters.
/// Block comments nest, as in WIT.
fn tokens(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let name_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%');
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '/' && chars.get(at + 1) == Some(&'/') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && chars.get(at + 1) == Some(&'*') {
            let mut depth = 0;
            while at < chars.len() {
                if chars[at] == '/' && chars.get(at + 1) == Some(&'*') {
                    depth += 1;
                    at += 2;
                } else if chars[at] == '*' && chars.get(at + 1) == Some(&'/') {
                    depth -= 1;
                    at += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    at += 1;
                }
            }
        } else if c.is_whitespace() {
            at += 1;
        } else if name_char(c) {
            let start = at;
            while at < chars.len() && name_char(chars[at]) {
                at += 1;
            }
            out.push(chars[start..at].iter().collect());
        } else {
            out.push(c.to_string());
            at += 1;
        }
    }
    out
}

/// The worlds declared in `source`. A world may hold only gated
/// `import <interface>;` and `export <interface>;` items; anything else (an
/// inline function or interface, `use`, `include`) is refused, so a new kind
/// of world item fails here until this test understands it.
fn worlds(source: &str) -> Result<BTreeMap<String, Items>, String> {
    let tokens = tokens(source);
    let mut out = BTreeMap::new();
    let mut depth = 0usize;
    let mut at = 0;
    while at < tokens.len() {
        match tokens[at].as_str() {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            "world" if depth == 0 && tokens.get(at + 2).is_some_and(|t| t == "{") => {
                let name = tokens[at + 1].trim_start_matches('%').to_string();
                let mut items = Items::default();
                at += 3;
                loop {
                    let Some(token) = tokens.get(at) else {
                        return Err(format!("world `{name}` is not closed"));
                    };
                    match token.as_str() {
                        "}" => break,
                        // A feature gate such as `@since(version = 0.1.0)`.
                        "@" => {
                            let close = tokens[at..]
                                .iter()
                                .position(|t| t == ")")
                                .ok_or_else(|| format!("world `{name}`: unclosed gate"))?;
                            at += close + 1;
                        }
                        "import" | "export" => {
                            let interface = tokens.get(at + 1).filter(|t| {
                                t.chars()
                                    .next()
                                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '%')
                            });
                            let (Some(interface), Some(";")) =
                                (interface, tokens.get(at + 2).map(String::as_str))
                            else {
                                return Err(format!(
                                    "world `{name}`: only `{token} <interface>;` is understood, \
                                     not `{}`",
                                    tokens[at..tokens.len().min(at + 4)].join(" ")
                                ));
                            };
                            let set = if token == "import" {
                                &mut items.imports
                            } else {
                                &mut items.exports
                            };
                            set.insert(interface.trim_start_matches('%').to_string());
                            at += 3;
                        }
                        other => {
                            return Err(format!(
                                "world `{name}`: unexpected `{other}`; only gated imports and \
                                 exports of named interfaces are understood"
                            ));
                        }
                    }
                }
                if out.insert(name.clone(), items).is_some() {
                    return Err(format!("world `{name}` is declared twice"));
                }
            }
            _ => {}
        }
        at += 1;
    }
    Ok(out)
}

/// The one gate every item of contract 0.1 carries, as tokens.
const GATE: &str = "@ since ( version = 0.1.0 )";

/// What the contract's items and gates do that this test does not account
/// for: a nested `package … { }` block (witforge 0.1.0 checked it but neither
/// locked nor generated it; 0.1.1 refuses it, and this check does not depend
/// on the pin), and any interface, world, interface item, or
/// world item without exactly `@since(version = 0.1.0)`. Component tooling
/// hides an `@unstable` item unless its feature is enabled, and witforge
/// treats gates as metadata, so a changed gate would otherwise pass every
/// check. A `use` carries no gate.
fn item_problems(source: &str) -> Vec<String> {
    let tokens = tokens(source);
    let mut problems = Vec::new();
    let mut at = 0;
    items(&tokens, &mut at, None, &mut problems);
    problems
}

/// The items from `at` to the `}` that closes `container`, or to the end of
/// the file at the top level, each checked as `item_problems` describes.
fn items(tokens: &[String], at: &mut usize, container: Option<&str>, problems: &mut Vec<String>) {
    let end_of = |from: usize, stop: &[&str]| {
        tokens[from..]
            .iter()
            .position(|t| stop.contains(&t.as_str()))
            .map(|offset| from + offset)
    };
    // The index just past the `}` that closes the `{` at `open`.
    let closing = |open: usize| {
        let mut depth = 0usize;
        for (index, token) in tokens.iter().enumerate().skip(open) {
            match token.as_str() {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        tokens.len()
    };
    while *at < tokens.len() {
        if tokens[*at] == "}" {
            *at += 1;
            return;
        }
        let mut gates = Vec::new();
        while tokens.get(*at).is_some_and(|t| t == "@") {
            let Some(close) = end_of(*at, &[")"]) else {
                problems.push("an unclosed gate".to_string());
                *at = tokens.len();
                return;
            };
            gates.push(tokens[*at..=close].join(" "));
            *at = close + 1;
        }
        let Some(keyword) = tokens.get(*at).map(String::as_str) else {
            problems.push(format!("gates at the end of the file: {gates:?}"));
            return;
        };
        // A `use` takes no gate (a gate there would be dropped silently
        // otherwise), and its `{…}` list is not a body.
        if keyword == "use" {
            if !gates.is_empty() {
                let label = container.map_or_else(
                    || "use".to_string(),
                    |container| format!("{container}: use"),
                );
                problems.push(format!("{label} carries {gates:?}; a `use` takes no gate"));
            }
            *at = end_of(*at, &[";"]).map_or(tokens.len(), |end| end + 1);
            continue;
        }
        let Some(end) = end_of(*at, &[";", "{"]) else {
            problems.push(format!("`{keyword}` has no end"));
            return;
        };
        let opens = tokens[end] == "{";
        if container.is_none() && keyword == "package" {
            if opens {
                problems.push(format!(
                    "nested `package {}` block: a contract is one package",
                    tokens[*at + 1..end].concat()
                ));
                *at = closing(end);
            } else {
                *at = end + 1;
            }
            continue;
        }
        let name = if tokens.get(*at + 1).is_some_and(|t| t == ":") {
            keyword.to_string()
        } else {
            tokens[*at..(*at + 2).min(end)].join(" ")
        };
        let label =
            container.map_or_else(|| name.clone(), |container| format!("{container}: {name}"));
        if gates != [GATE] {
            problems.push(format!(
                "{label} carries {gates:?}, not exactly `@since(version = 0.1.0)`"
            ));
        }
        let body = container.is_none() && matches!(keyword, "interface" | "world");
        *at = if opens && body {
            let name = format!("{keyword} {}", tokens[*at + 1]);
            let mut inner = end + 1;
            items(tokens, &mut inner, Some(&name), problems);
            inner
        } else if opens {
            closing(end)
        } else {
            end + 1
        };
    }
}

/// The generated trait for interface `name`: `adapter-api` is
/// `adapter_api::AdapterApi`.
fn generated_trait(name: &str) -> String {
    let camel: String = name
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect();
    format!("{}::{camel}", name.replace('-', "_"))
}

/// Everything about `found` that `table` and `generated` do not account for.
fn problems(
    found: &BTreeMap<String, Items>,
    table: &[World],
    generated: &[(&str, Generated)],
) -> Vec<String> {
    let set =
        |names: &[&str]| -> BTreeSet<String> { names.iter().map(ToString::to_string).collect() };
    let mut problems = Vec::new();
    let listed: BTreeSet<&str> = table.iter().map(|world| world.name).collect();
    for name in found.keys() {
        if !listed.contains(name.as_str()) {
            problems.push(format!(
                "world `{name}` is not in WORLDS: list it with its imports, exports, and the \
                 plugin_api trait that implements it"
            ));
        }
    }
    for world in table {
        let Some(items) = found.get(world.name) else {
            problems.push(format!(
                "WORLDS lists `{}`, which the WIT no longer declares",
                world.name
            ));
            continue;
        };
        let expected = Items {
            imports: set(world.imports),
            exports: set(world.exports),
        };
        if items != &expected {
            problems.push(format!(
                "world `{}` imports [{}] and exports [{}], not [{}] and [{}]",
                world.name,
                items.imports.iter().cloned().collect::<Vec<_>>().join(", "),
                items.exports.iter().cloned().collect::<Vec<_>>().join(", "),
                world.imports.join(", "),
                world.exports.join(", "),
            ));
        }
        for export in world.exports {
            let path = generated_trait(export);
            let paired = generated.iter().any(|(generated, counterpart)| {
                *generated == path
                    && matches!(counterpart, Generated::Pair(api) if *api == world.api)
            });
            if !paired {
                problems.push(format!(
                    "world `{}` exports `{export}`, whose generated `{path}` is not paired with \
                     plugin_api `{}` in inventory.rs",
                    world.name, world.api
                ));
            }
        }
    }
    problems
}

/// The worlds in every `.wit` file of the contract version the consumer
/// crate is generated from.
fn contract_worlds() -> BTreeMap<String, Items> {
    let mut out = BTreeMap::new();
    for (path, source) in contract_sources() {
        let found = worlds(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (name, items) in found {
            assert!(
                out.insert(name.clone(), items).is_none(),
                "world `{name}` is declared in more than one file"
            );
        }
    }
    out
}

/// Every `.wit` file of the contract version, with its source.
fn contract_sources() -> Vec<(std::path::PathBuf, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wit/alint-plugin/v0.1");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wit"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no .wit files in {}", dir.display());
    paths
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).unwrap();
            (path, source)
        })
        .collect()
}

#[test]
fn every_item_is_gated_at_0_1_0_in_one_package() {
    let mut problems = Vec::new();
    for (path, source) in contract_sources() {
        problems.extend(
            item_problems(&source)
                .into_iter()
                .map(|problem| format!("{}: {problem}", path.display())),
        );
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Each way an item can leave the 0.1 gate, or the file hold another
/// package, is reported; the gated contract shape is not.
#[test]
fn the_item_check_reports_every_changed_gate_and_a_nested_package() {
    let gated = "
        package a:b@0.1.0;
        /// Types.
        @since(version = 0.1.0)
        interface types {
            @since(version = 0.1.0)
            record r { x: u32, y: option<list<u8>> }
            @since(version = 0.1.0)
            variant v { a, b(string) }
            @since(version = 0.1.0)
            type t = list<r>;
            @since(version = 0.1.0)
            f: func(x: r) -> result<v, string>;
        }
        @since(version = 0.1.0)
        interface api {
            use types.{r};
            @since(version = 0.1.0)
            g: func() -> r;
        }
        @since(version = 0.1.0)
        world %w {
            @since(version = 0.1.0)
            import types;
            @since(version = 0.1.0)
            export api;
        }
    ";
    assert_eq!(item_problems(gated), Vec::<String>::new());
    for (from, to, expected) in [
        (
            "@since(version = 0.1.0)\n        world",
            "@unstable(feature = later)\n        world",
            "world %w carries",
        ),
        (
            "@since(version = 0.1.0)\n        interface api",
            "@unstable(feature = later)\n        interface api",
            "interface api carries",
        ),
        (
            "@since(version = 0.1.0)\n            record",
            "@unstable(feature = later)\n            record",
            "interface types: record r carries",
        ),
        (
            "@since(version = 0.1.0)\n            g:",
            "g:",
            "interface api: g carries []",
        ),
        (
            "@since(version = 0.1.0)\n            export",
            "@since(version = 0.2.0)\n            export",
            "world %w: export api carries",
        ),
        (
            "@since(version = 0.1.0)\n            type",
            "@since(version = 0.1.0)\n            @deprecated(version = 0.1.0)\n            type",
            "interface types: type t carries",
        ),
        (
            "export api;\n        }",
            "export api;\n        }\n        package other:pkg { interface extra { f: func(); } }",
            "nested `package other:pkg` block",
        ),
        (
            "use types.{r};",
            "@unstable(feature = later)\n            use types.{r};",
            "interface api: use carries",
        ),
    ] {
        let edited = gated.replacen(from, to, 1);
        assert_ne!(edited, gated, "{from}");
        let problems = item_problems(&edited);
        assert_eq!(problems.len(), 1, "{to}: {problems:?}");
        assert!(problems[0].contains(expected), "{to}: {problems:?}");
    }
}

#[test]
fn every_world_is_accounted_for() {
    let found = contract_worlds();
    assert!(!found.is_empty(), "no worlds found");
    let problems = problems(&found, WORLDS, GENERATED);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The parser reads worlds through comments and gates, ignores what is not
/// a world, and refuses world items it does not understand.
#[test]
fn the_world_parser_reads_worlds_and_refuses_the_rest() {
    let source = "
        package a:b@0.1.0;
        /* a /* nested */ comment: world hidden { export x; } */
        interface types { record world-info { world: string } }
        /// Doc comment: world fake { }
        @since(version = 0.1.0)
        world adapter {
            /// Shared types.
            @since(version = 0.1.0)
            import types;
            @unstable(feature = next)
            export %adapter-api;
        }
        world empty {}
        world %escaped { import types; }
    ";
    let found = worlds(source).unwrap();
    assert_eq!(
        found.keys().collect::<Vec<_>>(),
        ["adapter", "empty", "escaped"]
    );
    assert_eq!(
        found["adapter"],
        Items {
            imports: ["types".to_string()].into(),
            exports: ["adapter-api".to_string()].into(),
        }
    );
    assert_eq!(found["empty"], Items::default());

    for (refused, expected) in [
        (
            "world w { import f: func(); }",
            "only `import <interface>;`",
        ),
        (
            "world w { export i: interface { f: func(); } }",
            "only `export <interface>;`",
        ),
        ("world w { use types.{row}; }", "unexpected `use`"),
        ("world w { include other; }", "unexpected `include`"),
        (
            "world w { import a:b/types@0.1.0; }",
            "only `import <interface>;`",
        ),
        ("world w { import types;", "is not closed"),
        ("world w {} world w {}", "declared twice"),
    ] {
        let error = worlds(refused).expect_err(refused);
        assert!(error.contains(expected), "{refused}: {error}");
    }
}

/// Each check in `problems` fires on the input it exists for.
#[test]
fn every_kind_of_unaccounted_world_is_reported() {
    let items = |imports: &[&str], exports: &[&str]| Items {
        imports: imports.iter().map(ToString::to_string).collect(),
        exports: exports.iter().map(ToString::to_string).collect(),
    };
    let table = &[World {
        name: "adapter",
        imports: &["types"],
        exports: &["adapter-api"],
        api: "Adapter",
    }];
    let generated: &[(&str, Generated)] =
        &[("adapter_api::AdapterApi", Generated::Pair("Adapter"))];
    let found: BTreeMap<String, Items> =
        [("adapter".to_string(), items(&["types"], &["adapter-api"]))].into();
    assert_eq!(problems(&found, table, generated), Vec::<String>::new());

    let report = |found: &BTreeMap<String, Items>, generated| {
        let problems = problems(found, table, generated);
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems[0].clone()
    };
    let mut extra = found.clone();
    extra.insert("fixer".to_string(), items(&["types"], &["fixer-api"]));
    assert!(report(&extra, generated).contains("world `fixer` is not in WORLDS"));
    assert!(report(&BTreeMap::new(), generated).contains("no longer declares"));
    let swapped = [("adapter".to_string(), items(&["types"], &["analyzer-api"]))].into();
    assert!(report(&swapped, generated).contains("exports [analyzer-api], not"));
    let host = [(
        "adapter".to_string(),
        items(&["types", "host-api"], &["adapter-api"]),
    )]
    .into();
    assert!(report(&host, generated).contains("imports [host-api, types]"));
    let unpaired: &[(&str, Generated)] =
        &[("adapter_api::AdapterApi", Generated::Pair("Analyzer"))];
    assert!(report(&found, unpaired).contains("is not paired with plugin_api `Adapter`"));
}

#[test]
fn interface_names_map_to_their_generated_traits() {
    assert_eq!(generated_trait("adapter-api"), "adapter_api::AdapterApi");
    assert_eq!(generated_trait("types"), "types::Types");
    assert_eq!(generated_trait("a-b-c"), "a_b_c::ABC");
}
