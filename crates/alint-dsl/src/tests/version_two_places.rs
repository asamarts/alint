//! Where a `constraint` rule places its findings when the file it read has
//! changed since: the rows a run holds were read from the file as it was,
//! and placing reads it again. A finding or a related location is placed
//! only where the file still holds the cell's value (or key), and nowhere
//! else.

use alint_core::{Context, WalkOptions, walk};

use super::constraint::load_tree;
use super::constraint_families::shown;

/// The findings of `config`'s one rule over `a.json` written as `before`,
/// and then, its file rewritten as `after`, from the rows the run read
/// before (its decodes held in the run's cache), each shown with its place
/// and its related locations' places.
fn placed_again(config: &str, before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let (dir, loaded) = load_tree(config, &[("a.json", before)]);
    let config = loaded.unwrap();
    let index = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    index.enable_relation_caches_under(&alint_core::relation::Limits::default());
    let mut registry = alint_core::RuleRegistry::new();
    alint_rules::register_builtin(&mut registry);
    let rule = registry.build(&config.rules[0]).unwrap();
    let findings = || -> Vec<String> {
        let context = Context::new(dir.path(), &index);
        rule.evaluate(&context).unwrap().iter().map(shown).collect()
    };
    let first = findings();
    let decodes = index.decode_cache().unwrap().decodes();
    std::fs::write(dir.path().join("a.json"), after).unwrap();
    let second = findings();
    // The second run read no file: it holds the rows the first read.
    assert_eq!(index.decode_cache().unwrap().decodes(), decodes);
    (first, second)
}

/// `a.json`'s members, `{"a":"x","b":"y","c":"y"}`, of which `c` repeats
/// `b`'s value.
const BEFORE: &str = r#"{"a":"x","b":"y","c":"y"}"#;

/// The file rewritten: `b` holds "y" where it did, and where `c`'s "y" was
/// a key and its colon are now.
const MOVED: &str = r#"{"c":"q","b":"y","zz":"x","a":"w"}"#;

/// The file rewritten: each member where it was, `c`'s value another.
const CHANGED: &str = r#"{"a":"x","b":"y","c":"q"}"#;

/// An `entries:` rule's finding at `c`'s value and its related location at
/// `b`'s: placed where the file read again still holds the value, and
/// nowhere else.
#[test]
fn an_entry_is_placed_only_where_the_file_read_again_holds_its_value() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: a.json }\n    entries: { \
                  format: json, at: \"$\" }\n    fields: { name: entry.key, owner: entry.value }\nrules:\n  \
                  - id: c\n    kind: constraint\n    input: r\n    assert: { unique: { key: \
                  [owner] } }\n    level: error\n";
    let message = "`owner` \"y\" is not unique: the first row with it is `b` in a.json\n  ~ \
                   the first row with this key: a.json:1:14";
    let (first, moved) = placed_again(config, BEFORE, MOVED);
    assert_eq!(first, [format!("a.json:1:22: {message}")]);
    assert_eq!(moved, [format!("a.json: {message}")]);
    let (_, changed) = placed_again(config, BEFORE, CHANGED);
    assert_eq!(changed, moved);
}

/// A `read:` rule's finding at `c`'s value and its related location at
/// `b`'s, found when they are placed: the file read again numbers `zz`'s
/// value where `c`'s was, which is not the value, so the finding is placed
/// nowhere.
#[test]
fn a_read_value_is_placed_only_where_the_file_read_again_holds_it() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: a.json }\n    read: { json: \
                  \"$.*\" }\n    fields: { owner: value }\nrules:\n  - id: c\n    kind: \
                  constraint\n    input: r\n    assert: { unique: { key: [owner] } }\n    level: \
                  error\n";
    let message = "`owner` \"y\" is not unique: the first row with it is a.json\n  ~ the \
                   first row with this key: a.json:1:14";
    let (first, moved) = placed_again(config, BEFORE, MOVED);
    assert_eq!(first, [format!("a.json:1:22: {message}")]);
    assert_eq!(moved, [format!("a.json: {message}")]);
    let (_, changed) = placed_again(config, BEFORE, CHANGED);
    assert_eq!(changed, moved);
}
