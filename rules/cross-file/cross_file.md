---
title: 'cross_file'
description: 'alint cross_file rule (cross-file): A source must hold a relation to one or more targets (or, for resolves, the filesystem).'
sidebar:
  order: 6
categories: ['cross-file']
---

A `source` must hold a `relation` to one or more `targets` (or, for `resolves`, the filesystem). The `source` is a single `{ file, extract }` (for `registered`, a `file` or `files` with no `extract`) — **or**, for the set relations only (`subset` / `superset` / `set_equals`), `{ files: <glob>, extract }`, whose matches are read and whose extracted values are **unioned into one set** (the "every `*hl-X*` across `runtime/doc/*.txt` must equal the `default link X` set in `highlight.c`" shape — symbol-set / cross-language parity). For the value relations, `targets` is either a `{ files: <glob>, extract }` map (one query applied per glob match) or a sequence of `{ file, extract }` (heterogeneous pins); `extract` is the same one-of as `registry_paths_resolve` (`toml`/`json`/`yaml`/`xml` JSONPath, `lines`, `regex` group 1) plus `whole_file: {}` (the entire file content as one value — for byte-level content equality without `identical`'s no-`extract`/no-`normalize` constraint). `relation` (default `equals`) selects the assertion, checked independently per target; the *shape* (which of `source.file`/`source.files`, `source.extract`, `targets` is present) follows the relation and is validated at load:

Each target glob map or fixed-list entry may add `matches: <regex>`. The predicate runs on each extracted string before the relation and before fixer correlation; non-matching values behave like absent optional values and are never rewritten. This narrows syntactically broad structured queries without changing their source spans.

A regex target whose path is `Dockerfile` / `Dockerfile.*` and whose pattern selects `FROM`
is constrained to top-level `FROM` lines accepted by alint's Dockerfile parser; heredoc payloads
and continuation bodies do not become extracted values. The versioning/toolchain selectors apply
equivalent syntax-aware guards to Python, Rust toolchain TOML, Gemfile, and changelog targets.
Every cross-file extraction fails analysis above 10,000 values, and a syntax-aware scanner fails
above 100,000 physical lines. `sync_from` rewrites only the exact target-value occurrence carried
by the live finding, so a baseline-suppressed duplicate in the same file is untouched. Regex
propagation declines more than 10,000 edits or output above the lower of `fix_size_limit` and the
32 MiB hard ceiling.

| `relation` | source ⇒ | asserts (per target) |
|---|---|---|
| `equals` | exactly one value `v` | every target value `== v` |
| `subset` | a set `S` | `S ⊆ T` (singleton `S` = membership) |
| `superset` | a set `S` | `S ⊇ T` (the source covers every target value) |
| `set_equals` | a set `S` | `S == T` |
| `identical` | whole file content | the target is byte-identical (no `extract`; optional `skip_header_lines`) |
| `resolves` | a set of paths | each path exists on disk (no `targets`; the forward half of `registry_paths_resolve`) |
| `registered` | a set of filesystem paths (`source.file` / `source.files` with no `extract`, templated by the path template `register_as`: `{path}` default, `{dir}`, `{basename}`, `{stem}`, `{ext}`, `{parent_name}`) | each member is an element of every target's extracted list; fixable with `create_and_register` |

`normalize` relaxes the value comparison — a single transform or an ordered list applied left-to-right (`normalize: [trim, semver-minor]`): `trim`, `lower`, `semver-major` (the leading `MAJOR` band — the dotnet SDK shape), and `semver-minor` (the leading `MAJOR.MINOR` band, each token's leading digits with a non-digit prefix stripped — so `4.36-dev`, `4.36.0`, `pnpm@11.3.0` and `>=22.13` reconcile; the protobuf / pnpm version-format case). `identical` reads whole files byte-for-byte (`normalize`/`extract` do not apply), with an optional `skip_header_lines` to ignore a differing license/header; `resolves` extracts paths from the `source` (no `targets`) and checks each exists relative to the source file's directory. A structured query that selects a non-string reports one value-free violation naming only the count and JSON types; any string matches continue through the relation. Non-literal string values (interpolation / antiquotation) are skipped with an informational note — **except** a `whole_file: {}` source/target, whose single value (the entire file content) is compared verbatim even when it embeds `${…}`/`{{…}}` markers (those mark interpolated *paths*, not content); `whole_file` still honours `normalize`, so it sits between a query-extract and a no-normalize `identical`. `allow_missing_target` controls absent files/values. `equals` requires the `source` to extract **exactly one** value; to pull the latest entry from a multi-match file (e.g. the newest version in a multi-release `CHANGELOG.md`), anchor the regex so it captures only the first match — `regex: '(?s)\A.*?## (\d+\.\d+\.\d+)'` (`(?s)` makes `.` cross newlines, `\A` anchors at the start, `.*?` reaches the first heading lazily) yields a single value (a leading `## Unreleased` is skipped because the version pattern doesn't match it). The released `cross_file_value_equals` is a **byte-compatible alias** (`relation` defaults to `equals`). Cross-file.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow_missing_target` | boolean |  | `false` | When true, an absent target file or a missing extracted target value is tolerated instead of reported as drift. |
| `normalize` | normalize spec |  |  | A normalize transform, or an ordered list of transforms applied left-to-right (`[trim, semver-minor]`). `semver-major` / `semver-minor` keep only the leading MAJOR / MAJOR.MINOR band. |
| `register_as` | string |  | `null` | For `relation: registered` only: the value to look for (and, on fix, add) in each target list, as a `{path}` / `{dir}` / `{stem}` template over the matched member path. Defaults to `{path}` (the matched path verbatim), so a `files: "crates/*"` of directories needs no `register_as`; a source that names a `.../Cargo.toml` uses `{dir}` to register the parent directory. |
| `relation` | one of `equals` \| `subset` \| `superset` \| `set_equals` \| `identical` \| `resolves` \| `registered` |  |  | The assertion checked between the source and each target: `equals` (default), `subset`, `superset`, `set_equals`, `identical` (whole file byte-for-byte), or `resolves` (each path the source extracts exists on disk). |
| `skip_header_lines` | integer (>= 0) |  | `null` | For the `identical` relation only: drop this many leading lines from both files before comparison, to ignore a differing license or generated header. |
| `source` | object { `extract`, `file`, `files` } | yes |  | The file whose extracted value(s) form the reference side of the relation: a single `{ file, extract }`, or (set relations only) `{ files: <glob>, extract }` whose matches are unioned into one set. |
| `targets` | targets spec |  |  | The file(s) compared against the source, one relation check per target. Absent for `resolves` (the target is the filesystem). |

Plus the common `level`, `id`, `when`, and `fix` fields. This rule analyses the whole repository, so it takes no `paths` or `expect_matches`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A used value missing from the allow-list

The rule fires on this repository:

```text
allowed.json
used.json
```

```json title="allowed.json"
{"allowed": ["a", "b"]}
```

```json title="used.json"
{"used": ["a", "x"]}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: used-are-allowed
    kind: cross_file
    source:
      file: allowed.json
      extract: { json: "$.allowed[*]" }
    targets:
      files: "used.json"
      extract: { json: "$.used[*]" }
    relation: superset
    level: error
```

`alint check` reports:

```ansi
[2m--- used.json ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mused-are-allowed[0m
              used.json has value(s) not present in allowed.json: "x"
              [2mrelated:[0m comparison source: allowed.json

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every used value appears in the allow-list

This repository is compliant:

```text
allowed.json
used.json
```

```json title="allowed.json"
{"allowed": ["a", "b", "c"]}
```

```json title="used.json"
{"used": ["a", "c"]}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: used-are-allowed
    kind: cross_file
    source:
      file: allowed.json
      extract: { json: "$.allowed[*]" }
    targets:
      files: "used.json"
      extract: { json: "$.used[*]" }
    relation: superset
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

