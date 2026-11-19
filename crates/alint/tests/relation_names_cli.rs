//! A declared relation whose name starts with `_` or holds `_` and `-` is
//! a declared one on the command line: `explain` and `list` show it, and a
//! finding names it, as `relation NAME`, never as an inline `input:`
//! relation (design `v0.23/selectors-and-constraints.md` section 11).

use std::path::Path;
use std::process::Output;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn a_relation_named_with_underscores_and_hyphens_is_declared() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  _dep-ranges:\n    from: { paths: \"**/package.json\" }\n    \
         entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: entry.key \
         }\n  dep_ranges:\n    from: { paths: wanted.json }\n    entries: { format: json, at: \
         \"$\" }\n    fields: { name: entry.key }\nrules:\n  - id: wanted\n    kind: \
         constraint\n    input: _dep-ranges\n    assert: { set: { field: dependency, subset_of: \
         { relation: dep_ranges, field: name } } }\n    level: error\n",
    );
    write(
        root,
        "a/package.json",
        "{\"dependencies\": {\"left-pad\": \"1\"}}\n",
    );
    write(root, "wanted.json", "{\"react\": true}\n");

    let explain = run(root, &["explain", "wanted"]);
    assert!(explain.status.success(), "{explain:?}");
    let human = stdout(&explain);
    for line in [
        "    relation _dep-ranges  .alint.yml  (input)\n",
        "    relation dep_ranges  .alint.yml  (set target)\n",
        "    the inline selector of relation _dep-ranges  .alint.yml\n",
    ] {
        assert!(human.contains(line), "{line:?} in:\n{human}");
    }
    assert!(!human.contains("inline input"), "{human}");
    let json = run(root, &["explain", "wanted", "--format", "json"]);
    assert!(json.status.success(), "{json:?}");
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let relations: Vec<(&str, bool)> = json["plan"]["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relation| {
            (
                relation["name"].as_str().unwrap(),
                relation["inline"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(relations, [("_dep-ranges", false), ("dep_ranges", false)]);

    // `list` shows the declared relations, and an inline one never.
    let list = stdout(&run(root, &["list"]));
    for line in [
        "relation _dep-ranges  .alint.yml\n",
        "relation dep_ranges  .alint.yml\n",
    ] {
        assert!(list.contains(line), "{line:?} in:\n{list}");
    }

    let check = run(root, &["check"]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let human = stdout(&check);
    assert!(
        human.contains("relation `_dep-ranges` holds `dependency` \"left-pad\""),
        "{human}"
    );
}
