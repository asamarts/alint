use super::*;
use crate::located_fix::{LocatedEdit, LocatedOutcome, apply_file_edits};
use crate::rule::{Applicability, CollectedEdit, EditVerifier, ExpectedValue, FixEdit};
use crate::structured_fix::{
    document_remove, document_set, resolve_removal_span, resolve_removal_spans, resolve_value_span,
};
use crate::yaml_differential::generate::{self, Rng};
use serde_json::json;
use std::fmt::Write as _;

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.to_string())
}

fn idx(i: usize) -> PathSeg {
    PathSeg::Index(i)
}

/// `src` with the edits applied, or `None` when there is none or any is
/// missing (a fixer emits all of a removal's edits or the check declined
/// them all).
fn applied(src: &str, edits: &[Option<(Range<usize>, Vec<u8>)>]) -> Option<String> {
    let edits: Vec<_> = edits.iter().cloned().collect::<Option<_>>()?;
    if edits.is_empty() {
        return None;
    }
    String::from_utf8(splice(src.as_bytes(), &edits)?).ok()
}

fn removed(src: &str, paths: &[Vec<PathSeg>]) -> Option<String> {
    let before = Format::Yaml.parse(src).unwrap();
    applied(
        src,
        &checked_removal_edits(Format::Yaml, src.as_bytes(), &before, paths),
    )
}

fn set(src: &str, path: &[PathSeg], value: &Value) -> Option<String> {
    let before = Format::Yaml.parse(src).unwrap();
    applied(
        src,
        &checked_value_edits(
            Format::Yaml,
            src.as_bytes(),
            &before,
            &[(path.to_vec(), value.clone())],
        ),
    )
}

#[test]
fn intended_applies_sets_then_removals_from_the_last_path_back() {
    let before = json!({"a": [1, 2, 3, 4], "b": {"c": 1, "d": 2}, "e": null});
    let intents = [
        (vec![key("a"), idx(1)], Intent::Remove),
        (vec![key("a"), idx(3)], Intent::Remove),
        (vec![key("a"), idx(2)], Intent::Set(json!(9))),
        (vec![key("b"), key("c")], Intent::Remove),
        // A path selected twice is removed once.
        (vec![key("b"), key("c")], Intent::Remove),
        (vec![key("e")], Intent::Set(json!("x"))),
    ];
    assert_eq!(
        intended(&before, &intents),
        Some(json!({"a": [1, 9], "b": {"d": 2}, "e": "x"}))
    );
    // A set needs its node; a removal of a node that is not there changes
    // nothing.
    assert_eq!(
        intended(
            &before,
            &[(vec![key("z"), key("y")], Intent::Set(json!(1)))]
        ),
        None
    );
    assert_eq!(
        intended(&before, &[(vec![key("z")], Intent::Remove)]),
        Some(before.clone())
    );
}

/// YML-3, YML-4 and YML-5: a YAML line removal that changed a node it was not
/// aimed at, which the per-edit verify (of the target's path only) passed.
/// Each removal now declines, or leaves exactly the intended tree.
#[test]
fn a_yaml_removal_that_changes_another_node_declines() {
    // The blank line after the removed line joined the block scalar kept
    // with `|+`: `notes` read "Keep this.\n\n".
    let keep = "notes: |+\n  Keep this.\ndebug: true\n\nother: 1\n";
    assert_eq!(removed(keep, &[vec![key("debug")]]), None);
    // Without the blank line, the removal is exact.
    let out = removed(
        "notes: |+\n  Keep this.\ndebug: true\nother: 1\n",
        &[vec![key("debug")]],
    );
    assert_eq!(out.as_deref(), Some("notes: |+\n  Keep this.\nother: 1\n"));
    // An explicit key, or a key-less `:` entry, took a neighbour's value, or
    // a bare `?` was left behind.
    for src in [
        "? b\n<<: .nan\n: [a b]\n",
        "b: .nan\n? a\u{2028}\nk1: 's ''q'''\n: true\n",
        "\t\n  ?\n    \u{2028}!t nullyes",
        "? \n \"e\\n\\u0041\"",
    ] {
        let before = Format::Yaml.parse(src).unwrap();
        let Value::Object(members) = &before else {
            panic!("{src:?} reads as {before}");
        };
        for name in members.keys() {
            if let Some(out) = removed(src, &[vec![key(name)]]) {
                let mut want = before.clone();
                want.as_object_mut().unwrap().remove(name);
                assert_eq!(
                    Format::Yaml.parse(&out).ok(),
                    Some(want),
                    "{src:?} - {name}"
                );
            }
        }
    }
    // The two inputs the audit reported decline.
    assert_eq!(
        removed("? b\n<<: .nan\n: [a b]\n", &[vec![key("<<")]]),
        None
    );
    assert_eq!(
        removed(
            "b: .nan\n? a\u{2028}\nk1: 's ''q'''\n: true\n",
            &[vec![key("k1")]]
        ),
        None
    );
}

/// YML-4: removing a block mapping's last entry left the key with no value,
/// which reads as null, where JSON leaves `{}` and TOML the table. The entry
/// becomes `{}`; a removal of every entry at once leaves `{}` in the last
/// one's place.
#[test]
fn removing_a_block_mapping_s_last_entries_leaves_an_empty_mapping() {
    let cases: [(&str, &[&[&str]], &str); 5] = [
        (
            "deps:\n  lodash: 1.0\nname: x\n",
            &[&["deps", "lodash"]],
            "deps:\n  {}\nname: x\n",
        ),
        (
            "deps:\n  a: 1\n  b: \"2\"  # pinned\nname: x\n",
            &[&["deps", "a"], &["deps", "b"]],
            "deps:\n  {}  # pinned\nname: x\n",
        ),
        ("a: 1\n", &[&["a"]], "{}\n"),
        (
            "deps: &d\n  a: &v 1  # c\n",
            &[&["deps", "a"]],
            "deps: &d\n  {}  # c\n",
        ),
        // A flow mapping's member goes with its line, which leaves `{}`.
        ("x: {\n  a: 1\n}\n", &[&["x", "a"]], "x: {\n}\n"),
    ];
    for (src, paths, want) in cases {
        let paths: Vec<Vec<PathSeg>> = paths
            .iter()
            .map(|path| path.iter().map(|k| key(k)).collect())
            .collect();
        let out = removed(src, &paths);
        assert_eq!(out.as_deref(), Some(want), "{src:?}");
    }
    let out = removed("- x\n-\n  a: 1\n", &[vec![idx(1), key("a")]]).unwrap();
    assert_eq!(out, "- x\n-\n  {}\n");
    assert_eq!(Format::Yaml.parse(&out).unwrap(), json!(["x", {}]));
    // The range-only entry points resolve each path alone: the only entry of
    // a block mapping declines, and two entries delete their lines, which
    // together leave null (the fixers use `checked_removal_edits`).
    let src = "deps:\n  a: 1\n  b: 2\n";
    assert_eq!(
        resolve_removal_spans(
            Format::Yaml,
            src.as_bytes(),
            &[vec![key("deps"), key("a")], vec![key("deps"), key("b")]]
        ),
        [Some(6..13), Some(13..20)]
    );
    assert_eq!(
        resolve_removal_span(Format::Yaml, b"deps:\n  a: 1\n", &[key("deps"), key("a")]),
        None
    );
}

/// A set or a removal at a node an alias or a merge names, or inside one,
/// changed every alias of it too, and the verify (of the target only) passed.
#[test]
fn an_edit_at_a_node_an_alias_names_declines() {
    let nine = json!(9);
    for (src, path) in [
        ("a: &x 1\nb: *x\n", vec![key("a")]),
        (
            "base: &b\n  x: 1\nc:\n  <<: *b\n",
            vec![key("base"), key("x")],
        ),
        ("base: &b {x: 1}\nc: [*b]\n", vec![key("base"), key("x")]),
        ("s: [&x 1, *x]\n", vec![key("s"), idx(0)]),
    ] {
        assert_eq!(set(src, &path, &nine), None, "set {src:?}");
        assert_eq!(
            removed(src, std::slice::from_ref(&path)),
            None,
            "remove {src:?}"
        );
        assert_eq!(
            resolve_value_span(Format::Yaml, src.as_bytes(), &path),
            None,
            "{src:?}"
        );
    }
    // A key an alias names declines its removal, which would take the
    // anchor; a set of its value leaves the key, and the alias, alone.
    let src = "&k a: 1\nb: {*k : 2}\n";
    assert_eq!(removed(src, &[vec![key("a")]]), None);
    assert_eq!(
        set(src, &[key("a")], &nine).as_deref(),
        Some("&k a: 9\nb: {*k : 2}\n")
    );
    // An alias itself goes with its line; the anchor stays.
    assert_eq!(
        removed("a: &x 1\nb: *x\nc: 2\n", &[vec![key("b")]]).as_deref(),
        Some("a: &x 1\nc: 2\n")
    );
    // An anchor no alias names keeps the set.
    assert_eq!(
        set("a: &x 1\nb: 2\n", &[key("a")], &nine).as_deref(),
        Some("a: &x 9\nb: 2\n")
    );
}

/// Every path of `value`, the root's included, up to `limit` paths.
fn paths(value: &Value, limit: usize) -> Vec<Vec<PathSeg>> {
    let mut out = Vec::new();
    let mut pending = vec![(value, Vec::new())];
    while let Some((node, path)) = pending.pop() {
        if out.len() >= limit {
            break;
        }
        match node {
            Value::Object(members) => {
                for (name, member) in members {
                    let mut next = path.clone();
                    next.push(key(name));
                    pending.push((member, next));
                }
            }
            Value::Array(elements) => {
                for (i, element) in elements.iter().enumerate() {
                    let mut next = path.clone();
                    next.push(idx(i));
                    pending.push((element, next));
                }
            }
            _ => {}
        }
        out.push(path);
    }
    out
}

/// `value` with the node at `path` set to `to`, or removed: the test's own
/// reading of an intent, beside [`intended`].
fn edited(value: &Value, path: &[PathSeg], to: Option<&Value>) -> Value {
    let mut out = value.clone();
    let (last, parents) = path.split_last().unwrap();
    let mut node = &mut out;
    for seg in parents {
        node = match seg {
            PathSeg::Key(k) => node.get_mut(k.as_str()).unwrap(),
            PathSeg::Index(i) => node.get_mut(*i).unwrap(),
        };
    }
    match (node, last, to) {
        (Value::Object(members), PathSeg::Key(k), Some(to)) => {
            members.insert(k.clone(), to.clone());
        }
        (Value::Object(members), PathSeg::Key(k), None) => {
            members.remove(k);
        }
        (Value::Array(elements), PathSeg::Index(i), Some(to)) => elements[*i] = to.clone(),
        (Value::Array(elements), PathSeg::Index(i), None) => {
            elements.remove(*i);
        }
        _ => unreachable!("paths come from the value"),
    }
    out
}

/// Apply `edits` through the engine's located-edit batch, with the verifier
/// the fixer gives them, and return the bytes when every edit applied.
fn through_the_engine(
    format: Format,
    src: &[u8],
    edits: Vec<(Range<usize>, Vec<u8>)>,
    query: &str,
    expect: &ExpectedValue,
) -> Option<Vec<u8>> {
    let batch = edits
        .into_iter()
        .enumerate()
        .map(|(i, (range, content))| LocatedEdit {
            rule_index: 0,
            violation_index: i,
            sensitive: false,
            collected: CollectedEdit {
                edit: FixEdit::ReplaceRange {
                    path: "f".into(),
                    range,
                    content,
                },
                applicability: Applicability::Unsafe,
                verify: EditVerifier::Structured {
                    format,
                    strict: false,
                    query: query.to_string(),
                    expect: expect.clone(),
                },
                isolation_group: None,
            },
        })
        .collect();
    let (out, outcomes) = apply_file_edits(src, batch, Applicability::Unsafe);
    outcomes
        .iter()
        .all(|(_, outcome)| *outcome == LocatedOutcome::Applied)
        .then_some(out)
}

/// The query of exactly `path`: a JSON string is a name RFC 9535 reads.
fn query(path: &[PathSeg]) -> String {
    let mut out = "$".to_string();
    for seg in path {
        match seg {
            PathSeg::Key(k) => write!(out, "[{}]", serde_json::to_string(k).unwrap()),
            PathSeg::Index(i) => write!(out, "[{i}]"),
        }
        .unwrap();
    }
    out
}

/// What one format's edits did over the generated documents.
#[derive(Debug, Default)]
struct Tally {
    applied: usize,
    declined: usize,
}

/// The edit of one path as the fixer makes it, through the engine: `None`
/// when the fixer or the engine declines it.
fn fix(
    format: Format,
    src: &str,
    before: &Value,
    path: &[PathSeg],
    to: Option<&Value>,
) -> Option<Vec<u8>> {
    let bytes = src.as_bytes();
    let path = path.to_vec();
    let (edits, expect) = match (format, to) {
        (Format::Toml, Some(to)) => {
            let doc = document_set(format, bytes, &path, to)?;
            let intents = [(path.clone(), Intent::Set(to.clone()))];
            is_exact_edit(format, before, &doc, &intents).then(|| {
                (
                    vec![(0..bytes.len(), doc)],
                    ExpectedValue::Scalar(to.clone()),
                )
            })?
        }
        (Format::Toml | Format::Json, None) => {
            let doc = document_remove(format, bytes, std::slice::from_ref(&path))?;
            is_exact_edit(format, before, &doc, &[(path.clone(), Intent::Remove)])
                .then(|| (vec![(0..bytes.len(), doc)], ExpectedValue::Absent))?
        }
        (_, Some(to)) => {
            let edit = checked_value_edits(format, bytes, before, &[(path.clone(), to.clone())])
                .pop()??;
            (vec![edit], ExpectedValue::Scalar(to.clone()))
        }
        (_, None) => {
            let edit = checked_removal_edits(format, bytes, before, std::slice::from_ref(&path))
                .pop()??;
            (vec![edit], ExpectedValue::Absent)
        }
    };
    through_the_engine(format, bytes, edits, &query(&path), &expect)
}

/// A block mapping whose entries are the shapes a line removal or a span set
/// can disturb beside it: block scalars kept with `|+` and the blank lines
/// after them, explicit `?` keys and key-less `:` entries, mappings of one
/// entry, anchors and the aliases and merges that name them, multi-line flow
/// collections and scalars, comments. The harness's documents hold these
/// shapes too, but next to each other only once in thousands of documents.
fn neighbours(rng: &mut Rng) -> String {
    let mut out = String::new();
    let mut anchors = 0;
    for k in 0..2 + rng.below(5) {
        let entry = match rng.below(17) {
            0 => format!("k{k}: |+\n  kept\n"),
            1 => format!("k{k}: |-\n  stripped\n"),
            2 => format!("k{k}: >\n  folded\n\n"),
            3 => format!("? k{k}\n: v{k}\n"),
            4 => format!("? k{k}\n"),
            5 => format!("k{k}:\n  a: 1\n"),
            6 => format!("k{k}:\n  a: 1\n  b: [x]\n"),
            7 => {
                anchors += 1;
                format!("k{k}: &a{anchors} v{k}\n")
            }
            8 if anchors > 0 => format!("k{k}: *a{}\n", 1 + rng.below(anchors)),
            9 => format!("k{k}:\n  <<: {{m: 1}}\n  n: 2\n"),
            10 => format!("k{k}: [a,\n  b]\n"),
            11 => format!("k{k}: plain\n  continued\n"),
            12 => format!("k{k}: \"two\n  lines\"\n"),
            13 => format!("k{k}:\n- x\n- y: 1\n"),
            14 => format!("k{k}: 1  # note\n"),
            15 => format!("k{k}:\n"),
            _ => format!("k{k}: v{k}\n"),
        };
        out.push_str(&entry);
        match rng.below(6) {
            0 => out.push('\n'),
            1 => out.push_str("# between\n"),
            _ => {}
        }
    }
    out
}

/// The property behind the YAML findings, over generated documents, the
/// differential harness's and [`neighbours`]: an edit that applies leaves the
/// original tree with exactly its own change, a set at a scalar or a removal
/// of a mapping key. The same for JSON and TOML renderings of the same trees,
/// whose fixes the whole-document check covers as well.
#[test]
fn an_applied_structured_edit_changes_only_its_target() {
    const DOCUMENTS: usize = 3_000;
    let mut rng = Rng::new(0x5EED);
    let mut tallies = [
        (Format::Yaml, Tally::default()),
        (Format::Json, Tally::default()),
        (Format::Toml, Tally::default()),
    ];
    let new_values = [json!("fixed value"), json!(7), json!(true), Value::Null];
    for n in 0..DOCUMENTS {
        let yaml = match n % 4 {
            0 => neighbours(&mut rng),
            1 => {
                let document = generate::document(&mut rng, false);
                generate::mutate(&mut rng, &document, false)
            }
            _ => generate::document(&mut rng, false),
        };
        let Ok(tree) = Format::Yaml.parse(&yaml) else {
            continue;
        };
        let json_text = serde_json::to_string_pretty(&tree).unwrap();
        let toml_text = toml::to_string(&tree).ok();
        for (format, tally) in &mut tallies {
            let text = match format {
                Format::Yaml => yaml.clone(),
                Format::Json => json_text.clone(),
                _ => match &toml_text {
                    Some(text) => text.clone(),
                    None => continue,
                },
            };
            let Ok(before) = format.parse(&text) else {
                continue;
            };
            for (i, path) in paths(&before, 40).into_iter().enumerate() {
                let Some((last, parents)) = path.split_last() else {
                    continue;
                };
                let mut parent = &before;
                for seg in parents {
                    parent = match seg {
                        PathSeg::Key(k) => &parent[k.as_str()],
                        PathSeg::Index(i) => &parent[*i],
                    };
                }
                let node = match last {
                    PathSeg::Key(k) => &parent[k.as_str()],
                    PathSeg::Index(i) => &parent[*i],
                };
                let to = &new_values[i % new_values.len()];
                let mut tries = Vec::new();
                // A set aims at a scalar; TOML has no null to set.
                let scalar = !node.is_object() && !node.is_array();
                if scalar && (*format != Format::Toml || !to.is_null()) {
                    tries.push(Some(to));
                }
                if matches!(last, PathSeg::Key(_)) {
                    tries.push(None);
                }
                for to in tries {
                    let Some(out) = fix(*format, &text, &before, &path, to) else {
                        tally.declined += 1;
                        continue;
                    };
                    tally.applied += 1;
                    let read = std::str::from_utf8(&out)
                        .ok()
                        .and_then(|out| format.parse(out).ok());
                    assert_eq!(
                        read,
                        Some(edited(&before, &path, to)),
                        "document {n}: {} {:?} {:?} in {text:?}: {:?}",
                        format.label(),
                        to,
                        path,
                        String::from_utf8_lossy(&out)
                    );
                }
            }
        }
    }
    eprintln!("{tallies:?}");
    for (format, tally) in &tallies {
        assert!(tally.applied > 1_000, "{}: {tally:?}", format.label());
    }
}

#[test]
fn intended_dedups_repeated_sequence_removals() {
    let before = serde_json::json!({ "items": [10, 20, 30] });
    let intents = vec![
        (vec![key("items"), idx(0)], Intent::Remove),
        (vec![key("items"), idx(0)], Intent::Remove),
    ];
    let after = intended(&before, &intents).expect("intended succeeds");
    assert_eq!(after, serde_json::json!({ "items": [20, 30] }));
}
