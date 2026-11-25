---
title: 'constraint'
description: 'alint constraint rule (cross-file): An assertion over the rows of a relation: equal values, unique keys, or a set of values per group compared...'
sidebar:
  order: 19
categories: ['cross-file']
---

An assertion over the rows of a relation: equal values, unique keys, or a set of values per group compared with a reference group or another relation's field. Version 2 only, and experimental as `version: 2` is: a config of `version: 1` that uses it is refused at load with an error that names version 2, and its syntax may change before a release declares version 2 stable (v0.24 at the earliest). See [Version 2](/docs/configuration/version-2/) for selectors and relations.

A rule takes `input:`, the relation it reads (a name, bare or qualified by an `extends:` alias as `deps.ranges`, or an inline relation written as a relation under `relations:` is, whose values may not read the environment), and `assert:`, which holds exactly one of `equal`, `unique` and `set`. An optional key written as null (`equal:` with no value) is refused, never read as left out. It takes the common keys `id`, `level`, `message`, `when`, `policy_url` and `sensitive`, but no `paths:` and no `scope_filter:`: the selectors of its relation pick what it reads. It cannot be nested under a `for_each_*` kind's `require:`, it has no fix, and under `--changed` it runs when a changed path is one its selectors read (see below), then judges the whole repository. Every field it names (and every `{{row.FIELD}}` in its `message:`) must be a field of the relation, and a literal must be a string, or a boolean for a field that can hold one; a number cannot be written as a literal (YAML does not keep the text it is written with), and null matches nothing.

Values compare by their type and their text: a number by its lexeme (`1.0` is not `1`), and a path or path token as a string. Null, and a missing value, match nothing. A message calls a value a `read:` matched nothing `missing`, and a null `null`, in every family. With a `union:` input, the message names the part that read nothing, by its place and its query (`` `relations.node-pins.union[2]`, `read.json: $.volta.node` ``); when that part's relation reads the environment or the field is `sensitive`, it names the place alone. A key, group or membership comparison between two types (a number against a string) is an analysis failure that names both rows, never a silent non-match, and so is a null `group_by:` value; in `equal`, values of two types differ, and the finding shows both types. A relation that fails to evaluate gives the rule its analysis failures, never a pass, and so does one that reaches a relation limit (see [Version 2](/docs/configuration/version-2/#limits-in-this-release) for the limits and their values).


#### `equal`

`assert: { equal: { field: F } }`: every row has the same value of `F`. With `group_by: G`, rows are compared within each group of rows that share `G`'s value. Where rows disagree, each distinct value gets one finding, at the first row with it, related to one row of each other value and then to the other rows with it: 20 rows at most are picked in that order, then listed by path, with a count of the rest. Rows at one place (rows of one file without a span) are listed once and the rest counted, so the locations listed and the count add up to the rows.

`to: { row: { source: .nvmrc } }` compares every row with one source row: the row whose `source` field holds `.nvmrc`. With several fields (`row: { source: package.json, key: engines }`), the source row is the one that matches every one of them. Exactly one such row must exist and hold a value of `F`; none, several, or a source row without a value is an analysis failure. Each row that differs (a missing value differs) gets a finding, related to the source row. This form takes no `group_by:`.

`to: { field: G }` compares two fields of each row, such as a join's two sides: each row whose `F` differs from its `G` gets a finding at `F`, related to `G`, which may sit in another file.

#### `unique`

`assert: { unique: { key: [K, ...] } }`: no two rows share the key. Each row that repeats a key gets a finding, related to the first row with it; a row whose key holds a null is never a repeat. With `determines: [D, ...]`, rows that share the key must agree on the determined fields instead (a functional dependency): each distinct determined value of a key whose rows disagree gets one finding, at the first row with it, so a dependency at `^18.2.0` in 498 manifests and at `^17.0.2` in 2 gives two findings.

#### `set`

`assert: { set: { field: F, group_by: G, equal_to: { group: en } } }`: the set of `F`'s values in each group of `G` equals the set of the reference group, the rows whose `G` is `en`. A value the reference group holds and a group lacks is a finding at the group's first file, related to the reference row with it; a value a group holds that the reference group lacks is a finding at that row. `subset_of:` reports only the second, and `disjoint_from:` reports each value a group shares with the reference group, at that row, related to the reference row. Null is in no set. A reference group with no rows is an analysis failure. Without `group_by:`, the relation is one group.

A group is the rows that share a `G` value. When `G` is a `path` field or a path token (`stem`, `dir`, ...), every file the relation reads is a group too, rows or not, so a locale file that is `{}` lacks every value of the reference group, at that file, and so, under `strip-root`, does one whose top-level key holds nothing (`fr:`, `fr: {}`, `fr: []`); a file whose path alone the relation's `where:` rejects is none (its path-token fields are tested after their transforms, and a predicate on any other field cannot reject it). With `equal_to:`, `groups: [fr, de]` names groups that must exist: a group only `groups:` names lacks every value, in a finding with no path. Each literal is a string, or a boolean for a field that can hold one, of `G`'s type; it may not repeat or name the reference group, and `groups:` needs `group_by:`. Under `subset_of:` or `disjoint_from:` a group without rows passes, so `groups:` is refused there.

`equal_to: { relation: R, field: F2 }` (or `subset_of:`, `disjoint_from:`) compares each group with every value of field `F2` across the rows of relation `R`, a name, bare or qualified by an `extends:` alias (not an inline relation). `R` is read before the rule, and a failure of `R` withholds the rule's findings. A value only `R` holds is a finding at the group's first file, related to `R`'s row with it, and `{{row.FIELD}}` shows `<no row>` in its `message:`. `F2` and `F` must hold one type, and `R` may hold at most 100,000 distinct values of `F2`; past either, the rule has no verdict. A `sensitive` `F2` makes `F` sensitive at load, with every field of the input that can spell the same values: every `entry.*` field (`entry.key`, `entry.value`, `entry.parent`, `entry.root` and `entry.path`) when `F` is one, every `path` field and path token when `F` is one, every `value` field when `F` is one, every `bound:` field of the same JSONPath when `F` is one, and in a join every copy of the same field of a side, or of the field `on:` pairs it with; so no message, `{{row.FIELD}}` or baseline shows them, and `explain` marks them sensitive. What `R`'s `read:` skipped is reported once, with the input's own.

`plurals: cldr` collapses CLDR plural groups before comparing, as `key_parity` does: `F` must be an `entry.path` field (with no transform but `strip-root`), and each comparison of a group with the reference group merges nested CLDR objects (`inbox: { one, other }`), then i18next siblings (`items_one`, `items_other`), decided over the two groups together. A merged value is the group's key path, at its `other` member, or at its first member when it has no `other`. It takes a `{ group: }` target only.


#### Findings and baselines

A generated message quotes the keys and values a finding is about, each cut at 64 characters. A key path is cut at 64 characters per key and at 256 in all, and then ends with its count of segments (`` `a.b`... (5000 segments) ``). A key path, `{{row.FIELD}}` and a file a message names (a row's file, a note's, the file that declares a relation) show a control, bidi or zero-width character escaped (`\n`, `\u{202e}`), as a quoted value does, and so does the rest of the text of a note or an analysis failure, the relation's or the rule's own. In `message:`, `{{row.FIELD}}` is the reported row's value of a field (the reference group's row, for a missing value), cut the same way, and a `sensitive` field's value is shown as `<redacted>` there and in generated messages. A field that spells an entry's key (`entry.key`, `entry.path`, `entry.parent`, `entry.root`) is sensitive when any of them is, and then no message or baseline shows a key path. A finding about a field that a join copies from its `unique:` side, or about a `bound:` field, sits at the row's own entry (a join's row is the row of its other side), since many rows share that value. A finding or related location at a `sensitive` field's cell, or at any cell while key paths are hidden, shows only where it starts: the finding has a line and column but no range, and a related location ends where it starts, since the end would give away the value's length.

A finding sits at the line and column of its cell: an entry's key, or its value for `entry.value`; a `lines:` value's line; a `regex:` value's capture; and a JSON, YAML or TOML `read:` value that an object member holds. A `whole_file` value, a `read:` value in an array, a path token and a value of a file that is not UTF-8 have no line or column. A `read:` value's place is found only in the files a finding or related location sits in, when the rule places its findings, so reading the values costs the same whether or not they need a place. Placing reads each such file again, and a cell is placed only where the file still holds what was read there (the value, or the key for an entry's key): a file that changed during the run leaves a finding or related location without a line and column rather than at other text.

When a relation the rule reads takes a value from the environment (`{{env.X}}`) anywhere in its definition, a field's `where:` and `transform:` included, or reads a selector or relation that does (a join side, an `in:` list's relation, a `set`'s target), the environment may have chosen its rows: every finding's path is `<redacted>`, its message is `sensitive evidence` with a redaction marker, and it has no related locations, as for a version 1 rule whose `paths:` the environment writes. Even a filter on a field that holds no path says, by the rows it keeps, which files hold the value it was given. `explain` still shows the rest of the definition, with the values those entries were given as `<redacted>`. A value from the environment anywhere in `assert:` (a `{ group: }` target, a `groups:` item, a `to: { row: }` literal) redacts every message of the rule in the same way.

A finding's baseline fingerprint is its rule and its key alone, never its path: the first row of a key or the first file of a group moves when rows are added, removed or renamed, so adding a row that sorts first, or renaming the first file, keeps a baselined finding baselined. The key is made of the values the finding is about; a value longer than 128 bytes is held as its SHA-256 digest. Rows that repeat a `unique` key share one key: n rows with a key give n - 1 findings, which a baseline tells apart by their count, so renaming any of their files changes none. A finding about one row (`equal` with `to: { row: ... }` or `to: { field: ... }`) is keyed by that row's file and key path (a `read:` row's place among the values read), so renaming its file makes it a new finding. A sensitive field's value is left out of the key: every sensitive value is the one word `sensitive`, never a digest, which anyone who reads the baseline could test guesses against. Findings that differ only in a sensitive value share a key and are told apart by their count: three such findings against a baseline that counted two are one new finding. SARIF and GitLab output tell the findings of a run that share a fingerprint apart: they are numbered by the path, line and column they show, then rule and message (in SARIF, baselined findings first), and the first carries the fingerprint and each other one a fingerprint derived from it and its number, so code scanning keeps each as its own alert.

`alint explain <rule>` shows a `constraint` rule's resolved plan after its options: the assertion, every relation it reads in the order a run evaluates them, each with its source, reader, fields and `where:` and the file that declares it, and the selectors that run it under `--changed`; it shows no value read from a file, except the paths a manifest-derived `scope_filter:` resolves to, as for any rule (ADR-0010), and what the environment wrote or a sensitive field compares with shows as `<redacted>` (see [Version 2](/docs/configuration/version-2/)). `explain` names a `union:` relation's parts by their place from 0 (`union[0]`), as a message that names a part does.

#### Under `--changed`

`alint check --changed` (and `--base`) runs a `constraint` rule when a changed path, added, modified or deleted, is one its trigger scope covers: the selectors of every relation the rule reads, which are its `input:`, both sides of each join, the relation each `in:` or `not_in:` list probes, and the relation a `set` compares against, followed through every relation they read. A renamed file is a change to both of its paths, so one renamed out of a selector runs the rule. A selector's `scope_filter:` narrows the trigger scope as it narrows what the selector reads, and a change to what it consults also runs the rule: a file named as a `has_ancestor` marker in a directory where the selector's `paths:` can match (at or below it), or the manifest an `include_manifest_paths` or `exclude_manifest_paths` predicate reads. So does a `.gitignore` or `.ignore` in such a directory, since it decides which files there the walk reads, unless the walk reads no ignore file (`respect_gitignore: false` or `--no-gitignore`). So does a path Git lists as a directory where the selector's `paths:` can match below it: a submodule added, moved or removed, checked out or not, or an untracked repository, whose files Git does not list one by one. A pattern without `**` matches paths with as many components as it has, so it can match below a directory only when it has more components and its leading ones can match the directory's: `a/[xy]*.txt` cannot match below `a/sm`, while `a/*/c.json` and `a/**` can. A class that is negated or holds a range can match `/`, though (`a[!x]sm/*.json` matches `a/sm/c.json`), so a pattern with such a class, an alternation or an escape can match below every directory its leading literal directories allow. Without `--base`, Git lists an untracked repository on every run, so until it is committed or ignored, every such run runs the rules whose selectors' `paths:` can match below it, which every pattern that starts with `**` can. The rule then judges every file its relations read, changed or not, and reports every finding, so adding a key to `locales/en.json` reports the keys `locales/fr.json` lacks though `fr.json` did not change. When no changed path is in its trigger scope the rule does not run and reports nothing, not even an analysis failure. A change to `.alint.yml` or another configuration file alone runs no rule, as in version 1.

The changed paths are the ones Git reports, so several kinds of change that can alter a verdict run no rule. An edit to the target of a symlink a selector reads through is a change to the target's path alone: with `locales/de.json` linked to `shared/de.json`, editing `shared/de.json` does not run a rule whose selector reads `locales/*.json`. With `respect_gitignore: false`, a file Git ignores is read by the walk but never listed by Git, so adding or editing one (say, under a gitignored directory) runs nothing. And Git does not report a change to `.git/info/exclude`, to the global excludes file, or to an ignore file above the directory alint lints. Nor does Git list an edit to a file marked `git update-index --assume-unchanged`, or to a `.ignore` file that a `.gitignore` ignores. Nor does it list a staged or committed change to a submodule it is told to ignore (`submodule.<name>.ignore` or `diff.ignoreSubmodules`). Nor does it list `git submodule deinit`, which empties a submodule's directory, or `git submodule update --init` of one that was not checked out, or an edit inside a submodule's work tree that leaves its checked-out commit where it was, though each changes the files the walk reads there. A full `alint check` covers them all.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `assert` | object { `compare`, `count`, `equal`, `reference`, `set`, `unique` } | yes |  | The assertion: exactly one of `equal`, `unique`, `set`, `reference`, `count`, and `compare`. |
| `input` | v2 reference or v2 relation | yes |  | The rows the assertion reads: a relation's name, bare or qualified by an `extends:` alias (`deps.ranges`), or an inline relation (a map, as a relation under `relations:` is written, whose values may not read the environment). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths` or `expect_matches`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A dependency at two ranges across workspace manifests

The rule fires on this repository:

```text
packages/
packages/app/
packages/app/package.json
packages/legacy/
packages/legacy/package.json
packages/web/
packages/web/package.json
```

```json title="packages/app/package.json"
{
  "name": "app",
  "dependencies": {
    "react": "^18.2.0",
    "lodash": "^4.17.21"
  }
}
```

```json title="packages/legacy/package.json"
{
  "name": "legacy",
  "dependencies": {
    "react": "^17.0.2"
  }
}
```

```json title="packages/web/package.json"
{
  "name": "web",
  "dependencies": {
    "react": "^18.2.0"
  }
}
```

With this `.alint.yml`:

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  dependency-ranges:
    from: manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies"]
    fields: { dependency: entry.key, range: entry.value }
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert:
      unique: { key: [dependency], determines: [range] }
    level: error
```

`alint check` reports:

```ansi
[2m--- packages/app/package.json --------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mone-range-per-dependency[0m
              [2m4:14[0m  rows with `dependency` "react" disagree on `range`: "^18.2.0" here
              and in 1 other row; "^17.0.2" in 1 row
              [2mrelated:[0m a row with another value: packages/legacy/package.json:4:14-4:23
              [2mrelated:[0m another row with this value: packages/web/package.json:4:14-4:23

[2m--- packages/legacy/package.json -----------------------------------------------[0m
  [1m[31mx  error  [0m  [2mone-range-per-dependency[0m
              [2m4:14[0m  rows with `dependency` "react" disagree on `range`: "^17.0.2"
              here; "^18.2.0" in 2 rows
              [2mrelated:[0m a row with another value: packages/app/package.json:4:14-4:23

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Each dependency at one range across workspace manifests

This repository is compliant:

```text
packages/
packages/app/
packages/app/package.json
packages/web/
packages/web/package.json
```

```json title="packages/app/package.json"
{
  "name": "app",
  "dependencies": {
    "react": "^18.2.0",
    "lodash": "^4.17.21"
  }
}
```

```json title="packages/web/package.json"
{
  "name": "web",
  "dependencies": {
    "react": "^18.2.0"
  },
  "devDependencies": {
    "lodash": "^4.17.21"
  }
}
```

With this `.alint.yml`:

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  dependency-ranges:
    from: manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies"]
    fields: { dependency: entry.key, range: entry.value }
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert:
      unique: { key: [dependency], determines: [range] }
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

