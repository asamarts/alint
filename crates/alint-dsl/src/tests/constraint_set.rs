//! A `constraint` rule's `set` over trees, as phase 7 step 2 completes it
//! (design `v0.23/selectors-and-constraints.md` section 6): `plurals: cldr`,
//! grouping by `path` or a path token, `groups:`, and a `set` against
//! another relation's field, with their failures, redaction and bounds. The
//! builder's refusals are tested in `constraint.rs`, and a `set` against a
//! group of exact values in `constraint_families.rs`.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::constraint_families::{failures, findings, result, run};

/// Each finding's baseline key.
fn keys(results: &[alint_core::RuleResult], id: &str) -> Vec<String> {
    result(results, id)
        .violations
        .iter()
        .map(|violation| violation.baseline_key.as_deref().unwrap().to_string())
        .collect()
}

// --- `plurals: cldr` ---------------------------------------------------------

/// Relation `keys` of each `locales/*.yml` file's leaves, below its locale,
/// and rule `c` asserting `assert`.
fn locales(assert: &str) -> String {
    format!(
        "version: 2\nrelations:\n  keys:\n    from: {{ paths: \"locales/*.yml\" }}\n    entries: \
         {{ format: yaml, at: \"$\", leaves: true }}\n    fields: {{ locale: entry.root, key: \
         entry.path }}\n    transform: {{ key: [strip-root] }}\nrules:\n  - id: c\n    kind: \
         constraint\n    input: keys\n    assert: {assert}\n    level: error\n"
    )
}

const CLDR: &str =
    "{ set: { field: key, group_by: locale, equal_to: { group: en }, plurals: cldr } }";

const EN: &str = "en:\n  inbox:\n    one: \"1 message\"\n    other: \"n messages\"\n  items_one: \"1 \
                  item\"\n  items_other: \"n items\"\n  title: \"Title\"\n";

/// `pl` has a category `en` lacks in both kinds of group, and `other` in
/// neither; `xx`'s `inbox` holds a key no category names; `de` has a plural
/// group `en` lacks, and a `_one` key with no `_other` anywhere.
fn plural_tree() -> Vec<(&'static str, &'static str)> {
    vec![
        ("locales/en.yml", EN),
        (
            "locales/pl.yml",
            "pl:\n  inbox:\n    few: \"f\"\n  items_few: \"f\"\n  title: \"T\"\n",
        ),
        (
            "locales/xx.yml",
            "xx:\n  inbox:\n    one: \"1\"\n    other: \"n\"\n    label: \"l\"\n  items_one: \
             \"1\"\n  items_other: \"n\"\n  title: \"T\"\n",
        ),
        (
            "locales/de.yml",
            "de:\n  inbox:\n    one: \"1\"\n    other: \"n\"\n  items_one: \"1\"\n  items_other: \
             \"n\"\n  title: \"T\"\n  counter:\n    one: \"1\"\n    other: \"n\"\n  widgets_one: \
             \"w\"\n",
        ),
    ]
}

/// Plural groups are decided over each pair of groups, as `key_parity`
/// decides them over a reference and a peer: a group merges in both when
/// either has its `other`, and not when either holds a key no category
/// names. A merged value sits at its `other` member.
#[test]
fn plurals_cldr_merges_the_plural_groups_of_each_pair() {
    let results = run(&locales(CLDR), &plural_tree());
    assert_eq!(
        findings(&results, "c"),
        [
            "locales/de.yml:10:5: `locale` \"de\" holds `key` \"counter\", which `locale` \"en\" \
             lacks",
            "locales/de.yml:11:3: `locale` \"de\" holds `key` \"widgets_one\", which `locale` \
             \"en\" lacks",
            "locales/xx.yml:5:5: `locale` \"xx\" holds `key` \"inbox.label\", which `locale` \
             \"en\" lacks",
        ]
    );
    assert_eq!(
        keys(&results, "c")[0],
        "3:set6:string2:de6:string7:counter5:extra"
    );
    // A shared merged value sits at each group's `other` member.
    let results = run(
        &locales(&CLDR.replace("equal_to", "disjoint_from")),
        &plural_tree()[..2],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "locales/pl.yml:3:5: `locale` \"pl\" and `locale` \"en\" both hold `key` \"inbox\"\n  \
             ~ the reference group's row with it: locales/en.yml:4:5",
            "locales/pl.yml:4:3: `locale` \"pl\" and `locale` \"en\" both hold `key` \"items\"\n  \
             ~ the reference group's row with it: locales/en.yml:6:3",
            "locales/pl.yml:5:3: `locale` \"pl\" and `locale` \"en\" both hold `key` \"title\"\n  \
             ~ the reference group's row with it: locales/en.yml:7:3",
        ]
    );
    // Without `plurals: cldr`, every category is a value of its own.
    let results = run(
        &locales(&CLDR.replace(", plurals: cldr", "")),
        &plural_tree()[..2],
    );
    assert_eq!(findings(&results, "c").len(), 6);
}

/// Under `plurals: cldr`, missing values are reported in the reference's
/// row order and extra ones in the group's, not in key order.
#[test]
fn plurals_cldr_reports_in_row_order() {
    let results = run(
        &locales(CLDR),
        &[
            (
                "locales/en.yml",
                "en:\n  zeta: \"z\"\n  alpha: \"a\"\n  inbox:\n    one: \"1\"\n    other: \"n\"\n",
            ),
            (
                "locales/fr.yml",
                "fr:\n  zz: \"z\"\n  inbox:\n    other: \"n\"\n  aa: \"a\"\n",
            ),
        ],
    );
    let found = findings(&results, "c");
    let values: Vec<&str> = found
        .iter()
        .map(|finding| finding.split('"').nth(3).unwrap())
        .collect();
    assert_eq!(values, ["zeta", "alpha", "zz", "aa"], "{found:?}");
}

/// A reference of 20,000 keys and 2,000 groups of one key each: each group
/// is compared by looking its keys up, never by merging the reference again,
/// which made the comparisons the groups times the reference.
#[test]
fn plurals_cldr_compares_a_small_group_without_the_whole_reference() {
    let mut en = String::from("{\"en\": {\n");
    for index in 0..5_000 {
        writeln!(
            en,
            "\"k{index}\": {{\"one\": \"1\", \"other\": \"n\"}}, \"m{index}_one\": \"1\", \
             \"m{index}_other\": \"n\","
        )
        .unwrap();
    }
    en.push_str("\"last\": \"x\"}}\n");
    let mut files = vec![("locales/en.json".to_string(), en)];
    for index in 0..2_000 {
        files.push((
            format!("locales/l{index}.json"),
            format!("{{\"l{index}\": {{\"k{index}\": {{\"few\": \"f\"}}}}}}\n"),
        ));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();
    let config = locales(&CLDR.replace("equal_to", "subset_of"))
        .replace("locales/*.yml", "locales/*.json")
        .replace("format: yaml", "format: json");
    let started = Instant::now();
    let results = run(&config, &files);
    let elapsed = started.elapsed();
    assert_eq!(findings(&results, "c"), [] as [String; 0]);
    assert!(
        elapsed < Duration::from_secs(20),
        "2,000 groups took {elapsed:?}"
    );
}

// --- Grouped by `path` or a path token --------------------------------------

/// Relation `m` of each `locales/*.json` file's leaves, grouped by its path.
const BY_PATH: &str = "version: 2\nrelations:\n  m:\n    from: { paths: \"locales/*.json\" }\n    \
                       entries: { format: json, at: \"$\", leaves: true }\n    fields: { file: \
                       path, key: entry.path }\nrules:\n  - id: c\n    kind: constraint\n    input: \
                       m\n    assert: { set: { field: key, group_by: file, equal_to: { group: \
                       locales/en.json } } }\n    level: error\n";

/// Every file the relation reads is a group, rows or not: a file with no
/// keys lacks each of the reference group's, at that file.
#[test]
fn a_set_grouped_by_path_gives_every_file_its_group() {
    let results = run(
        BY_PATH,
        &[
            ("locales/en.json", "{\"a\": 1,\n\"b\": 2}"),
            ("locales/fr.json", "{\"a\": 1}"),
            ("locales/de.json", "{}"),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "locales/fr.json: `file` \"locales/fr.json\" lacks `key` \"b\", which `file` \
             \"locales/en.json\" holds\n  ~ the reference group's row with it: \
             locales/en.json:2:1",
            "locales/de.json: `file` \"locales/de.json\" lacks `key` \"a\", which `file` \
             \"locales/en.json\" holds\n  ~ the reference group's row with it: \
             locales/en.json:1:2",
            "locales/de.json: `file` \"locales/de.json\" lacks `key` \"b\", which `file` \
             \"locales/en.json\" holds\n  ~ the reference group's row with it: \
             locales/en.json:2:1",
        ]
    );
    assert_eq!(
        keys(&results, "c")[1],
        "3:set6:string15:locales/de.json6:string1:a7:missing"
    );
    // A reference file without keys is a reference group without rows.
    let results = run(
        BY_PATH,
        &[("locales/en.json", "{}"), ("locales/fr.json", "{\"a\": 1}")],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "relation `m` (.alint.yml): `assert.set.equal_to.group`: the reference group \
             (`file` \"locales/en.json\") has no rows; a set is compared with it, so it must \
             have some"
        ]
    );
}

/// A path token's group is its value after the field's transforms, as its
/// rows have it, and sits at the first of its files in path order, rows or
/// not.
#[test]
fn a_path_token_group_takes_its_transforms_and_sits_at_its_first_file() {
    let config = "version: 2\nrelations:\n  m:\n    from: { paths: \"locales/**/*.json\" }\n    \
                  entries: { format: json, at: \"$\", leaves: true }\n    fields: { locale: stem, \
                  key: entry.path }\n    transform: { locale: [lower] }\nrules:\n  - id: c\n    \
                  kind: constraint\n    input: m\n    assert: { set: { field: key, group_by: \
                  locale, equal_to: { group: en } } }\n    level: error\n";
    let results = run(
        config,
        &[
            ("locales/EN.json", "{\"a\": 1}"),
            ("locales/b/De.json", "{\"b\": 1}"),
            ("locales/a/de.json", "{}"),
            ("locales/Fr.json", "{\"a\": 2}"),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "locales/a/de.json: `locale` \"de\" lacks `key` \"a\", which `locale` \"en\" holds\n  \
             ~ the reference group's row with it: locales/EN.json:1:2",
            "locales/b/De.json:1:2: `locale` \"de\" holds `key` \"b\", which `locale` \"en\" \
             lacks",
        ]
    );
}

/// 3,000 files, a third of them without keys: one group each, built in one
/// pass over the files.
#[test]
fn a_set_grouped_by_path_is_linear_in_the_files() {
    let mut files = vec![("locales/en.json".to_string(), "{\"a\": 1}".to_string())];
    for index in 0..3_000 {
        let body = if index % 3 == 0 { "{}" } else { "{\"a\": 2}" };
        files.push((format!("locales/f{index:04}.json"), body.to_string()));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();
    let started = Instant::now();
    let results = run(BY_PATH, &files);
    let elapsed = started.elapsed();
    assert_eq!(result(&results, "c").violations.len(), 1_000);
    assert!(
        elapsed < Duration::from_secs(20),
        "3,000 files took {elapsed:?}"
    );
}

/// Relation `m` of each `locales/*.json` file's leaves, grouped by its stem
/// after `transform`, with `where:` `filter`, and the files `en` and `fr`
/// with keys `a` and `b`, beside `extra`.
fn filtered(transform: &str, filter: &str, extra: &[(&str, &str)]) -> Vec<String> {
    let config = format!(
        "version: 2\nrelations:\n  drafts:\n    from: {{ paths: drafts.json }}\n    entries: {{ \
         format: json, at: \"$\" }}\n    fields: {{ name: entry.key }}\n  m:\n    from: {{ \
         paths: \"locales/*.json\" }}\n    entries: {{ format: json, at: \"$\", leaves: true \
         }}\n    fields: {{ locale: stem, key: entry.path }}\n{transform}    where: \
         {filter}\nrules:\n  - id: c\n    kind: constraint\n    input: m\n    assert: {{ set: {{ \
         field: key, group_by: locale, equal_to: {{ group: en }} }} }}\n    level: error\n"
    );
    let mut files = vec![
        ("drafts.json", "{\"draft\": true}"),
        ("locales/en.json", "{\"a\": 1, \"b\": 2}"),
        ("locales/fr.json", "{\"a\": 1, \"b\": 2}"),
    ];
    files.extend_from_slice(extra);
    findings(&run(&config, &files), "c")
}

/// A resource that `where:` drops by its path tokens alone, rows or not, is
/// no group: its values are known without rows, after the transforms, and
/// a list may name another relation's field.
#[test]
fn a_resource_where_drops_by_its_path_is_no_group() {
    let draft = [("locales/draft.json", "{}")];
    for filter in [
        "{ locale: { not_in: [draft] } }",
        "{ locale: { in: [en, fr] } }",
        "{ locale: { not_matches: \"^dr\" } }",
        "{ locale: { not_in: [{ relation: drafts, field: name }] } }",
    ] {
        assert_eq!(filtered("", filter, &draft), [] as [String; 0], "{filter}");
    }
    let lowered = "    transform: { locale: [lower] }\n";
    let draft = [("locales/Draft.json", "{}")];
    assert_eq!(
        filtered(lowered, "{ locale: { not_in: [draft] } }", &draft),
        [] as [String; 0]
    );
    // Without the transform, `Draft` is not `draft`, so it is a group.
    assert_eq!(
        filtered("", "{ locale: { not_in: [draft] } }", &draft).len(),
        2
    );
}

/// A predicate on a field the path does not give may keep a row of a
/// resource without rows, had it any: that resource is a group.
#[test]
fn a_predicate_on_another_field_keeps_a_resource_without_rows() {
    let empty = [("locales/de.json", "{}")];
    for filter in [
        "{ key: { not_matches: \"^z\" } }",
        "{ key: { in: [a, b] }, locale: { not_in: [draft] } }",
    ] {
        assert_eq!(
            filtered("", filter, &empty),
            [
                "locales/de.json: `locale` \"de\" lacks `key` \"a\", which `locale` \"en\" \
                 holds\n  ~ the reference group's row with it: locales/en.json:1:2",
                "locales/de.json: `locale` \"de\" lacks `key` \"b\", which `locale` \"en\" \
                 holds\n  ~ the reference group's row with it: locales/en.json:1:10",
            ],
            "{filter}"
        );
    }
}

/// A join's copy of a path token from the side whose resources it lists is
/// known from the path too, through both relations' `where:`.
#[test]
fn a_join_drops_a_resource_by_its_copy_of_a_path_token() {
    let join = |side: &str, joined: &str| {
        format!(
            "version: 2\nrelations:\n  known:\n    from: {{ paths: known.json }}\n    entries: \
             {{ format: json, at: \"$\" }}\n    fields: {{ name: entry.key }}\n  keys:\n    from: \
             {{ paths: \"locales/*.json\" }}\n    entries: {{ format: json, at: \"$\", leaves: \
             true }}\n    fields: {{ locale: stem, key: entry.path }}\n{side}  m:\n    join: {{ \
             left: keys, right: known, on: {{ key: name }}, unique: right }}\n    fields: {{ \
             locale: left.locale, key: left.key }}\n{joined}rules:\n  - id: c\n    kind: \
             constraint\n    input: m\n    assert: {{ set: {{ field: key, group_by: locale, \
             equal_to: {{ group: en }} }} }}\n    level: error\n"
        )
    };
    let files = [
        ("known.json", "{\"a\": 1}"),
        ("locales/en.json", "{\"a\": 1}"),
        ("locales/draft.json", "{}"),
    ];
    let drop = "{ locale: { not_in: [draft] } }";
    for (side, joined) in [
        (format!("    where: {drop}\n"), String::new()),
        (String::new(), format!("    where: {drop}\n")),
    ] {
        assert_eq!(
            findings(&run(&join(&side, &joined), &files), "c"),
            [] as [String; 0]
        );
    }
    assert_eq!(findings(&run(&join("", ""), &files), "c").len(), 1);
}

// --- `groups:` ----------------------------------------------------------------

/// Relation `m` of `messages.json`'s locales, and rule `c` asserting a set
/// against `en` with `groups:` `groups`.
fn named_groups(groups: &str) -> String {
    format!(
        "version: 2\nrelations:\n  m:\n    from: {{ paths: messages.json }}\n    entries: {{ \
         format: json, at: [\"$.en\", \"$.fr\"] }}\n    fields: {{ locale: entry.parent, key: \
         entry.key }}\nrules:\n  - id: c\n    kind: constraint\n    input: m\n    assert: {{ set: \
         {{ field: key, group_by: locale, equal_to: {{ group: en }}, groups: {groups} }} }}\n    \
         level: error\n"
    )
}

const MESSAGES: &str = "{\"en\": {\"home\": \"Home\", \"about\": \"About\"},\n\"fr\": {\"home\": \
                        \"Accueil\"}}";

/// A group only `groups:` names lacks each of the reference group's values,
/// without a path; a named group with rows is compared as any group is.
#[test]
fn groups_names_groups_that_must_exist() {
    let results = run(&named_groups("[fr, de]"), &[("messages.json", MESSAGES)]);
    assert_eq!(
        findings(&results, "c"),
        [
            "messages.json: `locale` \"fr\" lacks `key` \"about\", which `locale` \"en\" holds\n  \
             ~ the reference group's row with it: messages.json:1:25",
            "`locale` \"de\" lacks `key` \"home\", which `locale` \"en\" holds\n  ~ the reference \
             group's row with it: messages.json:1:9",
            "`locale` \"de\" lacks `key` \"about\", which `locale` \"en\" holds\n  ~ the reference \
             group's row with it: messages.json:1:25",
        ]
    );
    let violation = &result(&results, "c").violations[1];
    assert!(violation.path.is_none() && violation.path_free_identity);
    assert_eq!(
        violation.baseline_key.as_deref(),
        Some("3:set6:string2:de6:string4:home7:missing")
    );
}

/// A group the environment names is a literal it wrote: no message or key
/// shows it.
#[test]
fn a_group_the_environment_names_is_not_shown() {
    let results = run(
        &named_groups("[\"{{env.ALINT_TEST_UNSET_GROUP | default('es')}}\"]"),
        &[("messages.json", MESSAGES)],
    );
    let found = findings(&results, "c");
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found[1].starts_with("`locale` <redacted> lacks `key` \"home\""),
        "{found:?}"
    );
    assert_eq!(
        keys(&results, "c")[1],
        "3:set9:sensitive6:string4:home7:missing"
    );
}

/// A reference group the environment names is a literal it wrote: no
/// message or key shows it.
#[test]
fn a_reference_group_the_environment_names_is_not_shown() {
    let config = named_groups("[]").replace(
        "equal_to: { group: en }",
        "equal_to: { group: \"{{env.ALINT_TEST_UNSET_REFERENCE | default('en')}}\" }",
    );
    let results = run(&config, &[("messages.json", MESSAGES)]);
    assert_eq!(
        findings(&results, "c"),
        [
            "messages.json: `locale` \"fr\" lacks `key` \"about\", which `locale` <redacted> \
             holds\n  ~ the reference group's row with it: messages.json:1:25"
        ]
    );
}

#[test]
fn a_group_literal_of_another_type_than_the_groups_has_no_verdict() {
    let config = named_groups("[\"true\"]")
        .replace("locale: entry.parent", "locale: entry.value")
        .replace("key: entry.key", "key: entry.parent")
        .replace("group: en", "group: true");
    let results = run(&config, &[("messages.json", "{\"en\": {\"a\": true}}")]);
    assert_eq!(
        failures(&results, "c"),
        [
            "messages.json: relation `m` (.alint.yml): `assert.set.groups[0]`: the literal is a \
             string and `locale` holds a boolean at `en.a` in messages.json; the comparison \
             would cross types, so the assertion has no verdict"
        ]
    );
}

/// 2,000 groups only `groups:` names, each lacking the reference's one
/// value: one finding each.
#[test]
fn groups_is_linear_in_the_groups() {
    let names: Vec<String> = (0..2_000).map(|index| format!("g{index}")).collect();
    let config = named_groups(&format!("[{}]", names.join(", ")));
    let started = Instant::now();
    let results = run(
        &config,
        &[("messages.json", "{\"en\": {\"a\": 1}, \"fr\": {\"a\": 1}}")],
    );
    let elapsed = started.elapsed();
    assert_eq!(result(&results, "c").violations.len(), 2_000);
    assert!(
        elapsed < Duration::from_secs(20),
        "2,000 groups took {elapsed:?}"
    );
}

// --- A `set` against another relation's field ----------------------------------

/// Relation `deps` of each package's dependencies, relation `allowed` of
/// `allowed.json`'s keys, and rule `c` asserting `assert` over `deps`.
fn against(assert: &str, extra: &str) -> String {
    format!(
        "version: 2\nrelations:\n  deps:\n    from: {{ paths: \"packages/*/package.json\" }}\n    \
         entries: {{ format: json, at: \"$.dependencies\" }}\n    fields: {{ manifest: path, \
         dependency: entry.key, range: entry.value }}\n  allowed:\n    from: {{ paths: \
         allowed.json }}\n    entries: {{ format: json, at: \"$\" }}\n    fields: {{ name: \
         entry.key, flag: entry.value }}\nrules:\n  - id: c\n    kind: constraint\n    input: \
         deps\n    assert: {assert}\n{extra}    level: error\n"
    )
}

fn packages() -> Vec<(&'static str, &'static str)> {
    vec![
        ("allowed.json", "{\"react\": true,\n\"vue\": true}\n"),
        (
            "packages/a/package.json",
            "{\"dependencies\": {\"react\": \"1\", \"left-pad\": \"1\"}}\n",
        ),
        ("packages/b/package.json", "{\"dependencies\": {}}\n"),
    ]
}

/// A missing value sits at the group's first file, related to the other
/// relation's row with it; an extra one at its row; a shared one at its
/// row, related to the other relation's.
#[test]
fn a_set_against_another_relations_field_reports_missing_extra_and_shared_values() {
    let set = |mode: &str| {
        format!(
            "{{ set: {{ field: dependency, group_by: manifest, {mode}: {{ relation: allowed, \
             field: name }} }} }}"
        )
    };
    let results = run(&against(&set("equal_to"), ""), &packages());
    assert_eq!(
        findings(&results, "c"),
        [
            "packages/a/package.json: `manifest` \"packages/a/package.json\" lacks `dependency` \
             \"vue\", which `name` of relation `allowed` holds\n  ~ the row of relation `allowed` \
             with it: allowed.json:2:1",
            "packages/a/package.json:1:33: `manifest` \"packages/a/package.json\" holds \
             `dependency` \"left-pad\", which `name` of relation `allowed` lacks",
            "packages/b/package.json: `manifest` \"packages/b/package.json\" lacks `dependency` \
             \"react\", which `name` of relation `allowed` holds\n  ~ the row of relation \
             `allowed` with it: allowed.json:1:2",
            "packages/b/package.json: `manifest` \"packages/b/package.json\" lacks `dependency` \
             \"vue\", which `name` of relation `allowed` holds\n  ~ the row of relation `allowed` \
             with it: allowed.json:2:1",
        ]
    );
    assert_eq!(
        keys(&results, "c")[0],
        "3:set6:string23:packages/a/package.json6:string3:vue7:missing"
    );
    let results = run(&against(&set("subset_of"), ""), &packages());
    assert_eq!(findings(&results, "c").len(), 1);
    let results = run(&against(&set("disjoint_from"), ""), &packages());
    assert_eq!(
        findings(&results, "c"),
        [
            "packages/a/package.json:1:19: `manifest` \"packages/a/package.json\" and `name` of \
             relation `allowed` both hold `dependency` \"react\"\n  ~ the row of relation \
             `allowed` with it: allowed.json:1:2"
        ]
    );
}

/// Without `group_by:`, the relation is one group, keyed `all`, whose first
/// file is the first it reads; `{{row.FIELD}}` has no row for a value only
/// the other relation holds.
#[test]
fn without_group_by_the_relation_is_one_group() {
    let assert = "{ set: { field: dependency, equal_to: { relation: allowed, field: name } } }";
    let message = "    message: \"{{row.dependency}} in {{row.manifest}}\"\n";
    let results = run(&against(assert, message), &packages());
    assert_eq!(
        findings(&results, "c"),
        [
            "packages/a/package.json: <no row> in <no row>\n  ~ the row of relation `allowed` \
             with it: allowed.json:2:1",
            "packages/a/package.json:1:33: left-pad in packages/a/package.json",
        ]
    );
    assert_eq!(
        keys(&results, "c"),
        [
            "3:set3:all6:string3:vue7:missing",
            "3:set3:all6:string8:left-pad5:extra"
        ]
    );
    // A relation that reads nothing lacks every value, without a path.
    let results = run(&against(assert, ""), &packages()[..1]);
    assert_eq!(
        findings(&results, "c"),
        [
            "relation `deps` lacks `dependency` \"react\", which `name` of relation `allowed` \
             holds\n  ~ the row of relation `allowed` with it: allowed.json:1:2",
            "relation `deps` lacks `dependency` \"vue\", which `name` of relation `allowed` \
             holds\n  ~ the row of relation `allowed` with it: allowed.json:2:1",
        ]
    );
}

/// The other relation's values are of one type, and of the compared
/// field's; a comparison across types has no verdict.
#[test]
fn a_set_against_values_of_another_type_has_no_verdict() {
    let assert = "{ set: { field: dependency, subset_of: { relation: allowed, field: flag } } }";
    let results = run(&against(assert, ""), &packages());
    assert_eq!(
        failures(&results, "c"),
        ["packages/a/package.json: relation `deps` (.alint.yml): \
             `assert.set.subset_of.field`: `dependency` holds a string at \
             `dependencies.react` in packages/a/package.json and `flag` of relation `allowed` \
             holds a boolean at `react` in allowed.json; the comparison would cross types, so \
             the assertion has no verdict"]
    );
    let mut files = packages();
    files[0].1 = "{\"react\": \"1\",\n\"vue\": 2}\n";
    let results = run(&against(assert, ""), &files);
    assert_eq!(
        failures(&results, "c"),
        [
            "allowed.json: relation `deps` (.alint.yml): `assert.set.subset_of.field`: `flag` of \
             relation `allowed` holds a string at `react` in allowed.json and `flag` of relation \
             `allowed` holds a number at `vue` in allowed.json; the comparison would cross \
             types, so the assertion has no verdict"
        ]
    );
}

/// The other relation is evaluated before the rule; when it fails, the rule
/// has its failures and no finding.
#[test]
fn a_failure_of_the_other_relation_withholds_every_finding() {
    let assert = "{ set: { field: dependency, subset_of: { relation: allowed, field: name } } }";
    let mut files = packages();
    files[0].1 = "{\"react\": ";
    let results = run(&against(assert, ""), &files);
    let failures = failures(&results, "c");
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("allowed.json: relation `allowed` (.alint.yml): could not parse"),
        "{failures:?}"
    );
}

/// A sensitive field of the other relation hides its values in messages,
/// keys and `{{row.FIELD}}` of the field compared with it.
#[test]
fn a_sensitive_field_of_the_other_relation_hides_its_values() {
    let config = against(
        "{ set: { field: dependency, group_by: manifest, equal_to: { relation: allowed, field: \
         name } } }",
        "",
    )
    .replace(
        "fields: { name: entry.key, flag: entry.value }",
        "fields: { name: { source: entry.key, sensitive: true } }",
    );
    let results = run(&config, &packages());
    for finding in findings(&results, "c") {
        for secret in ["react", "vue", "left-pad"] {
            assert!(!finding.contains(secret), "{finding}");
        }
    }
    for key in keys(&results, "c") {
        assert!(key.contains("9:sensitive"), "{key}");
        assert!(!key.contains("react") && !key.contains("vue"), "{key}");
    }
    let message = config.replace(
        "    level: error\n",
        "    message: \"{{row.dependency}} in {{row.manifest}}\"\n    level: error\n",
    );
    let results = run(&message, &packages());
    assert!(
        findings(&results, "c")
            .iter()
            .all(|finding| !finding.contains("react") && !finding.contains("left-pad")),
        "{:?}",
        findings(&results, "c")
    );
}

/// What the other relation's `read:` skipped (a value that is not a string,
/// a `${VAR}`) is reported by the rule, once, whether or not the input
/// relation took the same notes through a `where:` list that reads it.
#[test]
fn the_other_relations_skipped_values_are_reported_once() {
    let config = |filter: &str| {
        format!(
            "version: 2\nrelations:\n  allowed:\n    from: {{ paths: allowed.json }}\n    read: \
             {{ json: \"$.names[*]\" }}\n    fields: {{ name: value }}\n  deps:\n    from: {{ \
             paths: \"packages/*/package.json\" }}\n    entries: {{ format: json, at: \
             \"$.dependencies\" }}\n    fields: {{ dependency: entry.key }}\n{filter}rules:\n  - \
             id: c\n    kind: constraint\n    input: deps\n    assert: {{ set: {{ field: \
             dependency, equal_to: {{ relation: allowed, field: name }} }} }}\n    level: error\n"
        )
    };
    let files = [
        (
            "allowed.json",
            "{\"names\": [\"react\", 3, \"vue\", \"${PKG}\"]}",
        ),
        (
            "packages/a/package.json",
            "{\"dependencies\": {\"react\": \"1\", \"vue\": \"1\"}}",
        ),
    ];
    for filter in [
        "",
        "    where: { dependency: { in: [{ relation: allowed, field: name }] } }\n",
    ] {
        let results = run(&config(filter), &files);
        let result = result(&results, "c");
        let found: Vec<String> = result
            .violations
            .iter()
            .map(|violation| violation.message.to_string())
            .collect();
        assert_eq!(
            found,
            [
                "allowed.json: `read.json: $.names[*]` matched non-string value(s) (number: 1); \
                 `read:` rows hold strings, so select string values, or read typed values with \
                 `entries:`"
            ],
            "{filter}"
        );
        let notes: Vec<String> = result
            .notes
            .iter()
            .map(|note| note.message.to_string())
            .collect();
        assert_eq!(
            notes,
            ["allowed.json: skipped non-literal value \"${PKG}\""],
            "{filter}"
        );
    }
}

/// The other relation may be the input itself: each manifest holds every
/// dependency any manifest holds. Groups with rows come first, in the order
/// of their first rows, then files without rows.
#[test]
fn a_set_against_a_field_of_its_own_input() {
    let assert = "{ set: { field: dependency, group_by: manifest, equal_to: { relation: deps, \
                  field: dependency } } }";
    let mut files = packages();
    files.push((
        "packages/c/package.json",
        "{\"dependencies\": {\"react\": \"1\"}}\n",
    ));
    let results = run(&against(assert, ""), &files);
    assert_eq!(
        findings(&results, "c"),
        [
            "packages/c/package.json: `manifest` \"packages/c/package.json\" lacks \
             `dependency` \"left-pad\", which `dependency` of relation `deps` holds\n  ~ the row \
             of relation `deps` with it: packages/a/package.json:1:33",
            "packages/b/package.json: `manifest` \"packages/b/package.json\" lacks \
             `dependency` \"react\", which `dependency` of relation `deps` holds\n  ~ the row of \
             relation `deps` with it: packages/a/package.json:1:19",
            "packages/b/package.json: `manifest` \"packages/b/package.json\" lacks \
             `dependency` \"left-pad\", which `dependency` of relation `deps` holds\n  ~ the row \
             of relation `deps` with it: packages/a/package.json:1:33",
        ]
    );
}

/// An extra value held by many rows of its group is related to the first 20
/// of the others in row order, with a count of the rest. `packages/q` is
/// walked before `packages/q-00`, but sorts after it as a path, so the 20
/// sorted locations of all 25 others would leave it out.
#[test]
fn an_extra_value_is_related_to_its_first_20_other_rows() {
    let mut files = packages();
    let paths: Vec<String> = std::iter::once("packages/q/package.json".to_string())
        .chain((0..24).map(|index| format!("packages/q-{index:02}/package.json")))
        .collect();
    let body = "{\"dependencies\": {\"left-pad\": \"1\"}}";
    files.extend(paths.iter().map(|path| (path.as_str(), body)));
    let assert = "{ set: { field: dependency, subset_of: { relation: allowed, field: name } } }";
    let results = run(&against(assert, ""), &files);
    let violations = &result(&results, "c").violations;
    assert_eq!(violations.len(), 1);
    let related: Vec<&std::path::Path> = violations[0]
        .related_locations
        .iter()
        .map(|related| related.path.as_path())
        .collect();
    let mut expected: Vec<&std::path::Path> =
        paths[1..20].iter().map(std::path::Path::new).collect();
    expected.push(std::path::Path::new(&paths[0]));
    assert_eq!(related, expected);
    assert_eq!(violations[0].related_locations_omitted, 5);
    assert!(
        violations[0]
            .related_locations
            .iter()
            .all(|related| related.label == "another row of the group with it")
    );
}

/// A sensitive key of the other relation hides every key path in the
/// rule's messages, its failures' included.
#[test]
fn a_sensitive_key_of_the_other_relation_hides_key_paths() {
    let config = against(
        "{ set: { field: dependency, subset_of: { relation: allowed, field: flag } } }",
        "",
    )
    .replace(
        "fields: { name: entry.key, flag: entry.value }",
        "fields: { name: { source: entry.key, sensitive: true }, flag: entry.value }",
    );
    let failures = failures(&run(&config, &packages()), "c");
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("`flag` of relation `allowed` holds a boolean")
            && !failures[0].contains("react"),
        "{failures:?}"
    );
}

/// The lookup set of the other relation's field is bounded by the
/// distinct-value limit, as a `where:` list's is: past it, no verdict.
#[test]
fn the_other_set_is_bounded_by_the_distinct_value_limit() {
    let mut body = String::from("{");
    for index in 0..100_001 {
        if index > 0 {
            body.push(',');
        }
        write!(body, "\"n{index}\": true").unwrap();
    }
    body.push('}');
    let mut files = packages();
    files[0].1 = &body;
    let assert = "{ set: { field: dependency, subset_of: { relation: allowed, field: name } } }";
    let results = run(&against(assert, ""), &files);
    let failures = failures(&results, "c");
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains(
            "`assert.set.subset_of.field`: reads `allowed.name`: relation limit `distinct_values` \
             (100000) reached: 100001 rows or values, so the assertion has no verdict rather than \
             one over part of the set; narrow `allowed`"
        ),
        "{failures:?}"
    );
}

/// 4,000 groups of two values each against 50,000 values, one value of each
/// group outside them: each value is looked up in the other set, never
/// compared with each of its values, which took 4,000 times 50,000
/// comparisons.
#[test]
fn a_set_against_another_relation_is_linear_in_the_rows() {
    let mut allowed = String::from("{");
    for index in 0..50_000 {
        if index > 0 {
            allowed.push(',');
        }
        write!(allowed, "\"d{index}\": true").unwrap();
    }
    allowed.push('}');
    let mut deps = String::from("{");
    for index in 0..4_000 {
        if index > 0 {
            deps.push(',');
        }
        write!(
            deps,
            "\"p{index}\": {{\"d{index}\": \"1\", \"x{index}\": \"1\"}}"
        )
        .unwrap();
    }
    deps.push('}');
    let config = "version: 2\nrelations:\n  deps:\n    from: { paths: deps.json }\n    entries: { \
                  format: json, at: \"$.*\" }\n    fields: { package: entry.parent, dependency: \
                  entry.key }\n  allowed:\n    from: { paths: allowed.json }\n    entries: { \
                  format: json, at: \"$\" }\n    fields: { name: entry.key }\nrules:\n  - id: \
                  c\n    kind: constraint\n    input: deps\n    assert: { set: { field: \
                  dependency, group_by: package, disjoint_from: { relation: allowed, field: name \
                  } } }\n    \
                  level: error\n";
    let started = Instant::now();
    let results = run(config, &[("allowed.json", &allowed), ("deps.json", &deps)]);
    let elapsed = started.elapsed();
    let result = result(&results, "c");
    assert_eq!(result.violations.len(), 4_000);
    for violation in &result.violations {
        assert_eq!(violation.related_locations.len(), 1);
    }
    assert!(
        elapsed < Duration::from_secs(20),
        "4,000 groups took {elapsed:?}"
    );
}
