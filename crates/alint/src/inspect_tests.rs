//! Unit tests of `list`'s named-object listing (`inspect.rs`).

use std::path::PathBuf;

use alint_core::ConfigSourceClass;
use alint_core::relation::plan::{
    Capabilities, NamedSelector, Plan, PlanAlias, PlanSource, SelectorDef, SourceIndex,
    SourceLocator,
};

use super::*;

/// A plan whose `p/a.yml` declares `objects` selectors, which `.alint.yml`
/// reaches through `aliases` aliases.
fn aliased_plan(objects: usize, aliases: usize) -> Plan {
    let source = |display: &str, aliases: Vec<PlanAlias>| PlanSource {
        display: display.into(),
        locator: SourceLocator::Local(PathBuf::from("/repo").join(display)),
        class: ConfigSourceClass::LocalExtend,
        aliases,
    };
    let aliases = (0..aliases)
        .map(|index| PlanAlias {
            name: format!("a{index}"),
            url: "./p/a.yml".into(),
            target: Some(SourceIndex(0)),
        })
        .collect();
    Plan {
        sources: vec![source("p/a.yml", Vec::new()), source(".alint.yml", aliases)],
        selectors: (0..objects)
            .map(|index| NamedSelector {
                name: format!("s{index}"),
                source: SourceIndex(0),
                selector: SelectorDef {
                    paths: alint_core::PathsSpec::Single("x".into()),
                    scope_filter: None,
                },
                capabilities: Capabilities::default(),
                reads_environment: false,
            })
            .collect(),
        ..Plan::default()
    }
}

/// `text` without its SGR escape sequences.
fn without_styles(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('\u{1b}') {
        plain.push_str(&rest[..start]);
        rest = rest[start..].split_once('m').map_or("", |(_, after)| after);
    }
    plain.push_str(rest);
    plain
}

fn no_redaction(text: &str) -> std::borrow::Cow<'_, str> {
    text.into()
}

/// The text `list` (human and JSON) and `export-agents-md` (Markdown and
/// JSON) print for `plan`'s named objects.
fn listings(plan: &Plan) -> [String; 3] {
    let mut human = Vec::new();
    write_named_objects(&mut human, plan).expect("writes");
    let json = serde_json::to_string(&named_objects_json(plan, &no_redaction)).expect("JSON");
    let named = NamedObjects::of(plan, &no_redaction);
    let markdown = crate::export_agents_md::markdown::render(&[], Some(&named), "Lint rules");
    [String::from_utf8(human).expect("UTF-8"), json, markdown]
}

/// Each source's aliases are shown once, not once per object it declares:
/// 300 selectors of a file 300 aliases reach print some 600 lines' worth,
/// not 90,000 alias mentions (3,000 of each once printed 213 MB).
#[test]
fn objects_and_aliases_print_in_their_sum_not_their_product() {
    for (objects, aliases) in [(300, 300), (1, 300), (300, 1)] {
        let plan = aliased_plan(objects, aliases);
        let [human, json, markdown] = listings(&plan);
        let bound = 64 * (objects + aliases);
        for (what, text) in [("human", &human), ("json", &json), ("markdown", &markdown)] {
            assert!(
                text.len() <= bound,
                "{what}: {} bytes for {objects} objects and {aliases} aliases",
                text.len()
            );
        }
    }
}

#[test]
fn the_human_listing_names_each_source_once_with_its_aliases() {
    let [human, json, _] = listings(&aliased_plan(2, 2));
    assert_eq!(
        without_styles(&human),
        "\nselector s0  p/a.yml\nselector s1  p/a.yml\naliases p/a.yml  as a0 in .alint.yml, \
         as a1 in .alint.yml\n"
    );
    assert_eq!(
        json,
        r#"{"sources":[{"aliases":[{"alias":"a0","in":1},{"alias":"a1","in":1}],"path":"p/a.yml"},{"aliases":[],"path":".alint.yml"}],"selectors":[{"name":"s0","source":0},{"name":"s1","source":0}],"relations":[]}"#
    );
}

#[test]
fn env_chosen_aliases_do_not_correlate_with_sibling_aliases() {
    let make_plan = |aliases: Vec<PlanAlias>| Plan {
        sources: vec![
            PlanSource {
                display: alint_core::CONFIGURATION_MASK.into(),
                locator: SourceLocator::Bundled("alint://bundled/js/workspace-deps@v2".into()),
                class: ConfigSourceClass::Bundled,
                aliases: Vec::new(),
            },
            PlanSource {
                display: ".alint.yml".into(),
                locator: SourceLocator::Local(PathBuf::from("/repo/.alint.yml")),
                class: ConfigSourceClass::Root,
                aliases,
            },
        ],
        selectors: vec![NamedSelector {
            name: "s0".into(),
            source: SourceIndex(0),
            selector: SelectorDef {
                paths: alint_core::PathsSpec::Single("x".into()),
                scope_filter: None,
            },
            capabilities: Capabilities::default(),
            reads_environment: false,
        }],
        ..Plan::default()
    };

    // Public + env-chosen diamond: env_ws is omitted so listings never fuse it
    // onto pub_ws's target source.
    let mixed = make_plan(vec![
        PlanAlias {
            name: "pub_ws".into(),
            url: "alint://bundled/js/workspace-deps@v2".into(),
            target: Some(SourceIndex(0)),
        },
        PlanAlias {
            name: "env_ws".into(),
            url: alint_core::CONFIGURATION_MASK.into(),
            target: Some(SourceIndex(0)),
        },
    ]);
    let [human, json, markdown] = listings(&mixed);
    for (what, out) in [
        ("human", &without_styles(&human)),
        ("json", &json),
        ("markdown", &markdown),
    ] {
        assert!(out.contains("pub_ws"), "{what}: {out}");
        assert!(
            !out.contains("env_ws"),
            "{what} must not fuse env_ws with pub_ws: {out}"
        );
    }

    // Two env-chosen aliases targeting the same source: neither is fused, so
    // listings never reveal `env.A == env.B`.
    let both_env = make_plan(vec![
        PlanAlias {
            name: "env_a".into(),
            url: alint_core::CONFIGURATION_MASK.into(),
            target: Some(SourceIndex(0)),
        },
        PlanAlias {
            name: "env_b".into(),
            url: alint_core::CONFIGURATION_MASK.into(),
            target: Some(SourceIndex(0)),
        },
    ]);
    let [human, json, markdown] = listings(&both_env);
    for (what, out) in [
        ("human", &without_styles(&human)),
        ("json", &json),
        ("markdown", &markdown),
    ] {
        assert!(
            !out.contains("env_a") && !out.contains("env_b"),
            "{what} must not correlate env_a and env_b: {out}"
        );
    }
}
