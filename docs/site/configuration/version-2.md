---
title: 'Version 2 (experimental)'
description: 'Named selectors and relations in an experimental version 2 config, and their limits.'
sidebar:
  order: 10
---

A config whose top-level `version:` is `2` can name the files a policy reads (**selectors**) and
the rows of data it reads from them (**relations**), and refer to both by name, also from another
config file. Version 2 is everything version 1 is, plus `selectors:`, `relations:` and an `as:`
alias on an `extends:` entry.

The [`constraint`](/docs/rules/cross-file/constraint/) rule kind reads a relation and checks its
rows: equal values, unique keys, or the same set of values in every group (one range per
dependency, the same keys in every locale, toolchain pins that agree). It exists only in version
2, and is experimental as version 2 is. Every other rule kind runs exactly as in version 1, and a
relation no `constraint` rule reads is loaded and checked when the config loads, and never read.

> **Experimental.** Version 2 may change in any release until one declares it stable, v0.24 at
> the earliest, and the [CHANGELOG](https://github.com/asamarts/alint/blob/main/CHANGELOG.md)
> lists each change. Keep production policies on `version: 1` until then. Loading a version 2
> config prints a warning once per run, naming the file:
>
> ```text
> alint: warning: `version: 2` is experimental; its syntax may change before a release declares it stable, v0.24 at the earliest (/repo/.alint.yml)
> ```
>
> `alint validate-config` names the version on its success line and, with `--format json`, puts
> the warning in `warnings` and the version in `config_version`. `alint test` prints it once per
> run, naming the first policy test whose config is version 2. The language server loads a
> version 2 config without the warning.

## The version boundary

alint reads a config's `version:` before anything else in it and decodes the rest as that
version.

- Only the plain integer `2` selects version 2. A 2 written any other way (quoted as `'2'`,
  tagged, through a YAML alias, or as `+2`, `0x2`, `0o2` or `2.0`) is an error that says to write
  `version: 2`. A version read from the environment is version 1 and must equal 1, as before.
- A version 2 config accepts every version 1 key and rule kind with its version 1 meaning. A
  rule's `paths:` keeps its version 1 form; only relations read selectors.
- The `constraint` kind needs version 2: a version 1 config that uses it, or extends a file that
  does, is an error that names version 2, and so is a `constraint` rule nested under a
  `for_each_*` kind's `require:`. `alint rules list` and `alint rules show` mark the kind
  `version 2 only, experimental`.
- `selectors:`, `relations:` and `as:` in a version 1 config are errors that name the version
  they need (`` `selectors:` needs `version: 2`; this config is version 1 ``). An unknown key in
  a version 2 config is reported with the version 2 keys.

Only local files may be version 2: the top-level config, a local `extends:` target and an
`.alint.d/` fragment. A fragment without `version:` is read as its root's version. A remote
`extends:` source, a bundled ruleset, a nested config (`nested_configs:`) and the string
`alint_dsl::parse` reads must be `version: 1`, and a version 1 config cannot extend a version 2
one; each is an error that says so.

## Named selectors

A selector names a set of files. It takes the `paths:` and `scope_filter:` a rule takes and picks
files as a rule's scope does, inside the repository and never in `.git`.

```yaml
version: 2
selectors:
  manifests:
    paths: ["packages/*/package.json", "!packages/legacy/package.json"]
  crate-sources:
    paths: { include: "crates/*/src/**/*.rs", exclude: "**/generated/**" }
    scope_filter: { has_ancestor: Cargo.toml }
rules: []
```

| Key | Value |
|---|---|
| `paths` | required: a glob, a list of globs with `!` excludes, or `{ include, exclude }` |
| `scope_filter` | optional: `has_ancestor`, `include_manifest_paths` or `exclude_manifest_paths` |

A selector cannot use `changed_since`: the rules that read selectors judge the whole repository,
under `--changed` too, so what a selector picks must not depend on the diff. Under `--changed`, a
[`constraint`](/docs/rules/cross-file/constraint/) rule runs when a changed path is one the
selectors of its relations read; an ignore file (`.gitignore`, `.ignore`) the walk reads, in a
directory below which their `paths:` can match; a submodule added, moved or removed, or an
untracked repository (a directory Git lists whole), below which they can match; or a
`has_ancestor` marker or a manifest their `scope_filter:` consults. A manifest `source:` stays
inside the repository, as a rule's does.

Wherever a relation names a selector (`from:`), it may write one inline instead, as a map:
`from: { paths: package.json }`. A string there is always a selector's name, never a glob.

## Relations

A relation reads the files a selector picks and emits rows with named fields. It has `fields:`
and exactly one of `entries:`, `read:`, `union:` and `join:`. `transform:` and `where:` are
optional.

### `entries:` rows

`entries:` emits one row per member of each object an `at:` path selects in a JSON, YAML or TOML
file, or one per leaf below it with `leaves: true` (a key with no key below it; an array is a
value, so its elements are never keys). `at:` is a JSONPath or a list of them, read in order.
A file that lacks an `at:` path gives no rows from that path.

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
    fields:
      manifest: path
      field: entry.parent
      dependency: entry.key
      range: entry.value
rules: []
```

| Key | Value | Default |
|---|---|---|
| `format` | `json`, `yaml` or `toml` | required |
| `at` | a JSONPath, or a non-empty list of them | required |
| `leaves` | `true` or `false` | `false` |

### `read:` rows

`read:` extracts values as `cross_file` does, one row per value. It takes exactly one of:

- `json:`, `yaml:` or `toml:`, a JSONPath;
- `regex:`, a regex whose first capture group is the value. It runs over the whole file, so `^`
  is the file's start unless the regex sets `(?m)`;
- `lines: {}` or `lines: { comment: "//" }`, one value per non-blank line, trimmed, skipping
  lines that start with the comment prefix (`#` by default);
- `whole_file: {}`, the file's content as one value.

A `read:` row's value is a string. A file where the read matches nothing gives one row without a
value, unless the relation sets `optional: true`, which gives none. As in `cross_file`, a value
written as `${VAR}` is skipped with a note, and a match that is not a string (a table, a number)
gives one note for the file naming its kinds, while the file's strings are still rows. A join, or
a `where:` list that reads the relation, keeps these notes. `read:` cannot take `xml`,
`ini`, `hcl`, `dotenv` or `properties` in this build: dotenv and properties values stay redacted,
and the others have no value rows yet.

```yaml
version: 2
relations:
  package-names:
    from: { paths: "packages/*/package.json" }
    read: { json: "$.name" }
    fields: { name: value, dir: dir }
  node-version:
    from: { paths: .nvmrc }
    read: { regex: "^v?(\\S+)" }
    optional: true
    fields: { version: value }
rules: []
```

### `union:` rows

`union:` emits the rows of two or more parts, in the order written. Each part has its own `from:`,
its own `entries:` or `read:`, and its own `optional:`; the relation's `fields:` apply to every
part, so each part must be able to bind every field.

### `join:` rows

`join:` combines the rows of two relations whose `on:` fields are equal: `on:` maps a field of
`left:` to a field of `right:`. The join is inner: a row without a match on the other side is
dropped. `unique:` is required and names the side, `left` or `right`, that holds at most one row
per join key; a repeated key there is an analysis failure naming both rows. Output rows follow
the order of the other side. `fields:` names every output field as `left.<field>` or
`right.<field>`; nothing is copied implicitly. A join may read another join.

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  dependency-ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { manifest: path, dependency: entry.key, range: entry.value }
  workspace-versions:
    from: manifests
    read: { json: "$.version" }
    fields: { name: "bound: $.name", version: value }
  ranges-of-workspace-packages:
    join:
      left: dependency-ranges
      right: workspace-versions
      on: { dependency: name }
      unique: right
    fields:
      manifest: left.manifest
      dependency: left.dependency
      range: left.range
      version: right.version
rules: []
```

### Fields

`fields:` maps each field's name to its source, or to `{ source, sensitive }`:

| Source | Rows | Value |
|---|---|---|
| `path` | any but a join | the file's path |
| `dir`, `basename`, `stem`, `ext`, `parent_name` | any but a join | a part of the file's path |
| `entry.key` | `entries:` | the entry's own key |
| `entry.value` | `entries:` | its value, a scalar |
| `entry.parent` | `entries:` | the key of the object that holds it (null at the top level) |
| `entry.root` | `entries:` | the first key of its key path (`en` for `en.nav.home`) |
| `entry.path` | `entries:` | its key path from the top (`nav.home`) |
| `value` | `read:` | the extracted value |
| `bound: <JSONPath>` | `entries:`, a document `read:` | a value read once per file (`$.name`) |
| `left.<field>`, `right.<field>` | `join:` | a field of the matched row on that side |

A document `read:` is one with `json:`, `yaml:` or `toml:`; one with `regex:`, `lines:` or
`whole_file:` reads the file as text and binds no `bound:` field. In a `union:`, a part binds
`entry.*` only with `entries:`, `value` only with `read:`, and `bound:` as a relation of its one
part would. A `bound:` path must match exactly once in each file. `sensitive: true` keeps a
field's values out of messages, the JSON report and baselines. A field that shows a sensitive
field's value is sensitive too: a join's copy of it, a join's copy of the field an `on:` pair joins
with it, and a field that an `in:` or `not_in:` list of it filters, with the fields that show that
field's value (an entry's key, path and value).

### `transform:`

`transform:` maps a field to a list of transforms, applied in order before `where:`:

- `trim` and `lower`, on strings. `lower` trims too, and lowercases character by character, which
  is not Unicode case folding (`STRASSE` and `straße` stay different);
- `semver-major` and `semver-minor`, which keep the major, or the major and minor, part of a
  version (`^18.2.0` becomes `18` or `18.2`), as `cross_file`'s `normalize` does. They do not
  apply to a path, a path token or an `entry.path`, which name where a value is, nor to a join's
  copy of one;
- `strip-root`, which drops the first segment of an `entry.path` field (`en.nav.home` becomes
  `nav.home`), or of a join's copy of one that no transform has changed. It comes first in its
  list, and each file must have one top-level key. A file whose top-level key holds nothing
  (`fr:`, `fr: {}`, `fr: []`) has no keys, as an empty file has none; one whose top-level key
  holds a value (`fr: hello`) is an analysis failure.

### `where:`

`where:` maps a field to predicates that every row must meet:

- `in` and `not_in`: a list whose items are literals or `{ relation, field }` references, the
  values another relation's field holds;
- `matches` and `not_matches`: a regex, on strings.

A literal in `in` or `not_in` is a string or, for a field that may hold one, a boolean. Null and
numbers are refused: null is in no set, and a number compares by the text it is written with
(`1.10` is not `1.1`), which the config's YAML does not keep.

Every predicate tests every row, so the order they are written in does not matter. A row that any
predicate is false for is dropped. A row that none is false for, but that one cannot test (a
pattern on a number, a list of another type), is an analysis failure.

`in`, `not_in` and a join's `on:` compare the exact text of the values after their transforms.
Nothing normalizes Unicode, so an `é` written as one code point is not one written as `e` and a
combining accent, and only `lower` changes case.

```yaml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  workspace-names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  external-dependencies:
    from: manifests
    entries: { format: json, at: ["$.dependencies", "$.devDependencies"] }
    fields: { dependency: entry.key, range: entry.value }
    transform: { range: [trim] }
    where:
      range: { not_matches: "^(workspace|file|link):" }
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
rules: []
```

## Names, aliases and namespaces

A name, an alias and a field name start with an ASCII letter or `_` and hold only ASCII letters,
digits, `_` and `-`. A config file declares each name once, across its selectors, relations and
aliases. Names, aliases and references are literal: `{{...}}` in one is an error, since it must
name the same thing in every environment. Other values interpolate `{{env.X}}` as in version 1,
except that a value substituted in a selector or relation stays text: with `NODE_MAJOR=18`,
`in: ["{{env.NODE_MAJOR}}"]` compares with the text `18`, where a rule's value re-types as a
number. Only the fields that take a boolean (`optional:`, `leaves:` and a field's `sensitive:`)
read a substituted `true` or `false` as one; in an `in:` list it is the text `true`, which does
not match the boolean. An error that could quote a value read from the
environment is redacted to the selector or relation it is about, as a version 1 rule's is.

Each config file has its own namespace:

- A bare name (`from:`, a join's `left:` and `right:`, a `{ relation, field }` reference) names an
  object the same file declares.
- `alias.name` names an object declared by the file an `extends:` entry gives `as: alias`. It
  reaches only what that file declares itself: there is no re-export and no `a.b.c` chain.
- Nothing shadows: an extended file's names never mix with the extending file's.
- An `.alint.d/` fragment has a namespace of its own too, so it may declare a name the top-level
  config also declares.

`as:` takes a local `extends:` entry only; bundled and remote sources are version 1 and declare
nothing to reach.

Here `policies/deps.yml` declares a selector and a relation, the top-level config reaches them
through the alias `deps`, and an `.alint.d/` fragment declares a `manifests` selector of its own:

```yaml
# file: policies/deps.yml
version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
rules: []
```

```yaml
version: 2
extends:
  - url: ./policies/deps.yml
    as: deps
relations:
  workspace-versions:
    from: deps.manifests
    read: { json: "$.version" }
    fields: { name: "bound: $.name", version: value }
  ranges-of-workspace-packages:
    join: { left: deps.ranges, right: workspace-versions, on: { dependency: name }, unique: right }
    fields: { dependency: left.dependency, range: left.range, version: right.version }
rules: []
```

```yaml
# file: .alint.d/10-docs.yml
# No `version:`: read as the top-level config's version. Its namespace is its own.
selectors:
  manifests:
    paths: "docs/**/*.md"
rules:
  - id: docs-have-an-index
    kind: file_exists
    paths: docs/index.md
    level: warning
```

A name or alias that does not resolve is an error naming the file and the key, with the names the
file can reach or the nearest one:

```text
.alint.yml: `relations.titles.from`: no selector named `doc` in this config; the selectors it can reach are `docs`
```

`alint list` shows each named selector and relation after the rules, with the file that declares
it, then one line per file an alias reaches, with every alias that reaches it;
`alint export-agents-md` lists them the same way. A file is named by its path from the top-level
config's directory, `../`-relative when `allow_out_of_root: true` lets an `extends:` chain leave
it. A file an `extends:` entry read from the environment chooses, and every file it extends in
turn, is named `<redacted>`, here and in errors found in it, however short its name.

`alint explain <rule>` on a `constraint` rule shows its resolved plan after its options: the
assertion, then every relation the rule reads in the order a run evaluates them (its input, both
sides of each join, the relations its `in:` and `not_in:` lists probe, and a `set`'s other
relation), each named as the config writes each reference that reaches it (with the file that
declares it when two relations would read alike, so a reference names one), with its source, reader,
fields, transforms, `where:` and the file that declares it; then the selectors that run the rule
under `--changed`. For example, for a rule whose `input:` joins two relations:

```text
plan:
  assert: unique: key [dependency], determines [range]
  relations, in evaluation order:
    relation ranges  .alint.yml
      from: selector manifests  .alint.yml
        paths: packages/*/package.json
      entries: json at $.dependencies
      fields:
        dependency: entry.key
        range: entry.value
    relation names  .alint.yml
      from: selector manifests  .alint.yml
        paths: packages/*/package.json
      read: json $.name
      fields:
        name: value
        manifest: path
    relation joined  .alint.yml  (input)
      join: left ranges, right names, on dependency = name, unique right
      fields:
        dependency: left.dependency
        range: left.range
        manifest: right.manifest
  trigger: under --changed, the rule runs on a change to a path these selectors pick, to an ignore file, submodule or nested repository where they can match, or to a marker or manifest their scope_filter: consults
    selector manifests  .alint.yml
      paths: packages/*/package.json
  cost: linear in the rows of each relation it reads
```

With `respect_gitignore: false` or `--no-gitignore`, the walk reads no ignore file, and the
`trigger:` line names none. A `union:` relation's parts are named by their place from 0
(`union[0]:`), as a message that names a part names it.

`--format json` gives the same as a `plan` object, where each relation has an `id` (its place in the
list) by which a join's sides, an `in:` list, the trigger, `input` and `set_target` name it. The
plan is configuration and shows no value read from a file, except the paths a manifest-derived
`scope_filter:` resolves to, as for any rule (ADR-0010). What the environment wrote shows as
`<redacted>`, as do a sensitive field's `where:` literals, patterns and transforms and the literals
the assertion compares with it.

## What the loader checks

The whole relation plan is checked when the config loads, before any file is read:

- every key, value type, field source, transform and predicate;
- every name and alias resolves, and relations that read each other are refused, naming the cycle
  (`` relation `a` reads itself: `a` -> `b` -> `a` ``);
- every field a join's `on:`, a `left.<field>` or `right.<field>`, a `transform:` or a `where:`
  names exists, a relation's rows bind each of its fields (no `bound:` field on a text `read:`),
  and every part of a `union:` binds every field;
- `strip-root` comes first on an `entry.path` field or a join's copy of one, and the version
  transforms are not applied to a path, a path token or an `entry.path`;
- each regex compiles, a `read:` regex has a capture group, and each JSONPath parses;
- the literals in `in` and `not_in` can match.

When a `constraint` rule reads a relation, what a relation cannot hold (an `entry.value` that
is an object, an array or a TOML date, a `bound:` path that does not match exactly once, a
repeated key on a join's `unique:` side, a comparison across types) is an analysis failure naming
the relation, the file and the key path, never a dropped row. So is reaching a row, join or
distinct-value limit, which is never truncation.

## Limits in this release

- Only the `constraint` kind reads a relation, and a rule's `paths:` cannot name a selector.
- A `set`'s `plurals: cldr` takes an `entry.path` field and a `{ group: }` target only, and a
  `set` against another relation's field names it (bare or qualified), never inline. That
  relation may hold at most 100,000 distinct values of the field.
- Only local files may be version 2 (see [the version boundary](#the-version-boundary)).
- A selector cannot use `changed_since`.
- Values are scalars: null, booleans, numbers (compared by the text they are written with) and
  strings. An `entry.value` that is an object, an array or a TOML date is an analysis failure,
  and it fails before `where:` runs, so no predicate can drop its row: narrow `at:` to the
  objects of scalars, or read the leaves with `leaves: true`.
- `entries:` reads JSON, YAML and TOML. `read:` reads JSON, YAML and TOML values, a regex, lines
  or a whole file; XML, INI, HCL, dotenv and properties are not readable.
- A join is inner, and one side must be unique on the join key: there is no many-to-many join.
- The transforms are the five listed; there are no user-defined ones.
- `in` and `not_in` cannot list null or a number.
- Reading is bounded by these limits, which a config cannot change in this release. Reaching one
  is an analysis failure that names it, never truncation:
  - 1,000,000 rows from one reader of one file (`rows_per_extractor`);
  - 5,000,000 rows from every reader in a run (`rows_per_run`);
  - 5,000,000 cells in the relations a run builds, a row of n fields holding n cells and each file
    a part reads one more (`cells_per_run`);
  - 1 GiB that a run's relations hold beyond their cells: the files they decode, and the key
    paths, path tokens and transformed values they build (`bytes_per_run`);
  - 1,000,000 rows into each side of a join (`join_inputs`), and 1,000,000 out of it
    (`join_outputs`);
  - 100,000 distinct values of the field a `set` compares against in another relation
    (`distinct_values`).
- Placing a `constraint` rule's findings reads each file they sit in again, one at a time. Each
  file must fit in what the run's relations left of `bytes_per_run`, the limit less what their
  decodes and their rows took: alone, before its `read:` values' places are looked for, and then
  with its offsets and what finding those places held (the document parsed again, with what its
  parser held beside the tree, such as a JSONC file's copy without its comments or the TOML lexer's
  tokens, and what was read from the parse). The relations are evaluated, and what they took
  counted, before a rule that reads one runs: in `alint check` before every rule, and in each pass
  of `alint fix` before the first such rule. When a fix changes the tree, the rules after it read
  their relations again, but are checked against what the pass's relations took, and `fix
  --dry-run` and `fix --diff`, which change nothing, place findings as `check` does. Placing takes
  nothing from it, so every rule's files are checked against the same amount, whatever other rules
  hold, and a run holds at most one such file per thread. When a file does not fit, the rule
  reports an analysis failure, not findings without places. A cell is placed only where the file
  read again still holds what was read there, its value or its key, so a file changed during the
  run leaves a finding or related location without a place.
- A `set` with `equal_to:` gives one finding per value the reference group holds for each group
  that lacks it, so G empty groups against V reference values give G × V findings, as
  `key_parity` does for files without keys
  ([#596](https://github.com/asamarts/alint/issues/596)).

## Trust

Version 2 is local only in this release: every file that declares a selector or a relation is the
top-level config, a local `extends:` target or an `.alint.d/` fragment. Each selector and relation
passes the trust gates of the file that declares it, with what it references, as an extended
file's rules do. A reference reaches only the same file or a file it extends, never a file that
extends it.

## The three policies

These are the three policies the `constraint` kind is designed against, each a relation and the
rule that checks it.

**One range per dependency** across a workspace, leaving out workspace packages and specifiers
that are not ranges:

```yaml
version: 2
selectors:
  workspace-manifests:
    paths: ["packages/*/package.json", "!packages/docusaurus/package.json"]
relations:
  workspace-names:
    from: workspace-manifests
    read: { json: "$.name" }
    fields: { name: value }
  dependency-ranges:
    from: workspace-manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields: { field: entry.parent, dependency: entry.key, range: entry.value }
    where:
      range: { not_matches: "^(catalog|npm|workspace):" }
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
```

**The same keys in every locale**, over each locale's merged tree:

```yaml
version: 2
selectors:
  locale-files: { paths: "config/locales/**/*.yml" }
relations:
  locale-keys:
    from: locale-files
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
    where: { key: { not_matches: "^(date|devise|views)\\." } }
rules:
  - id: locale-keys-match-english
    kind: constraint
    input: locale-keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
        groups: [de, fr, pl]
        plurals: cldr
    level: error
```

A group is the rows that share a `group_by:` value, so `groups:` (with `equal_to:` only) names
the locales that must exist: one whose files hold no keys lacks every English key, in findings
with no path. A file that holds only its root (`fr:`, `fr: {}` or `fr: []`) holds no keys.
`plurals: cldr` compares `inbox: { one, other }` with a locale's `inbox: { one, few, many,
other }` as one key, `inbox`, and `items_one`, `items_other` as `items`. Grouped by a path
token instead (`fields: { locale: stem, ... }`), every file is a group, and an empty one lacks
every key at that file.

**Every dependency approved**, against another relation's field:

```yaml
version: 2
relations:
  dependencies:
    from: { paths: "packages/*/package.json" }
    entries: { format: json, at: "$.dependencies" }
    fields: { manifest: path, dependency: entry.key }
  approved:
    from: { paths: approved-dependencies.json }
    entries: { format: json, at: "$" }
    fields: { name: entry.key }
rules:
  - id: approved-dependencies
    kind: constraint
    input: dependencies
    assert:
      set:
        field: dependency
        group_by: manifest
        subset_of: { relation: approved, field: name }
    level: error
```

**Toolchain pins that agree**: one row per place a Node version is pinned, each compared with the
row `.nvmrc` gives. A part whose path a file lacks gives a row without a value, which differs from
`.nvmrc`'s.

```yaml
version: 2
relations:
  node-pins:
    union:
      - { from: { paths: .nvmrc }, read: { regex: "^v?(\\S+)" } }
      - { from: { paths: package.json }, read: { json: "$.engines.node" } }
      - { from: { paths: package.json }, read: { json: "$.devEngines.runtime.version" } }
      - { from: { paths: package.json }, read: { json: "$.volta.node" } }
    fields: { source: path, version: value }
rules:
  - id: node-pins-agree
    kind: constraint
    input: node-pins
    assert: { equal: { field: version, to: { row: { source: .nvmrc } } } }
    level: error
```

## Editor support: the draft schema

A draft JSON Schema for version 2 ships at
[`schemas/v2/config.json`](https://github.com/asamarts/alint/blob/main/schemas/v2/config.json).
Its `$id` names the release, and it may change with version 2. Point an editor at it with a
modeline on the first line of a version 2 file:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/v0.23.0/schemas/v2/config.json
version: 2
selectors:
  docs: { paths: "docs/**/*.md" }
rules: []
```

Nothing associates a file name with either schema: an editor checks a config against the
schema its modeline names, as for the [version 1 schema](/docs/configuration/). A version 2
file needs the version 2 modeline, and so does a version 2 `.alint.d/` fragment without
`version:`; the schema then asks for `version: 2`, as the version 1 schema asks for
`version: 1`.

The draft checks what the loader checks of a single file: every key, value type, name,
reference, field source, transform, format, `read:` form and count, and the keys that need or
exclude others (one family in a `constraint` rule's `assert:`, one target in its `set`). It cannot
check:

- what needs every file loaded: names and aliases that resolve, re-exports, cycles, and the plan
  checks above (fields that exist, union parts that bind every field, transforms that apply, the
  literals in `in` and `not_in`);
- in a `constraint` rule's `assert:`, what needs its relation or compares two of its values: a
  field the relation has, a literal of the field's type, a `group_by:` or `to: { field: }` that
  names the compared field, a `determines:` field that is part of the key, a `groups:` entry that
  names the reference group, and an `entry.path` field under `plurals: cldr`;
- a name declared twice across selectors, relations and aliases;
- regexes, a `read:` regex's capture group and JSONPaths;
- which files may be version 2, and that a version 1 config does not extend a version 2 one;
- YAML tags, and a key YAML reads as a boolean or null (`true:`), which an editor reads as text;
- a `version:` of 2 written as another number (`2.0`, `+2`, `0x2`, `0o2`, `2e0`), which JSON
  reads as the number 2 and alint refuses;
- as in version 1: that `only:` and `except:` are not both set on one `extends:` entry, a glob
  that does not compile (`paths: "a["`), a `has_ancestor:` that is a path or a glob (`a/b`,
  `*.json`), and a plain `http://` `extends:` entry.
