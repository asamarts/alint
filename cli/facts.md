---
title: 'alint facts'
description: 'alint facts prints the resolved value of every facts: entry in the effective config, the quickest way to debug a when: clause.'
---

Rules can be gated on facts about the repository (for example "is there a
`Cargo.toml`?") through `when:` clauses, and bundled rulesets declare their
own facts. When a rule runs or skips unexpectedly, `alint facts` shows what
each fact resolved to.

Facts are evaluated once per run, before any rule. A rule whose `when:` is
false is dropped without reading a file, which is how one config can carry
Rust, Python and Node rules and only run the ones that apply. The cost is that
a skipped rule is silent: it doesn't fail, it just never reports. `alint facts`
makes the gate visible.

## What it prints

A config that extends `alint://bundled/python@v1` (which declares
`has_python`) and adds three facts of its own:

```yaml
facts:
  - id: n_py_files
    count_files: "**/*.py"
  - id: has_rust
    any_file_exists: [Cargo.toml]
  - id: branch
    git_branch: {}
```

```bash
alint facts
```

```text
has_python  any_file_exists  true
n_py_files  count_files      1
has_rust    any_file_exists  false
branch      git_branch       "main"
```

One line per fact: its id, its kind and the value it resolved to. Facts from
`extends:` rulesets come first, then the config's own. Any rule gated on
`facts.has_rust` is skipped in this repository, and a `when:` such as
`facts.n_py_files > 5` is false.

## Fact kinds

| Kind | Resolves to |
| --- | --- |
| `any_file_exists` | `true` if any listed path or glob matches a file |
| `all_files_exist` | `true` only if every listed path matches |
| `count_files` | the number of files matching a glob |
| `file_content_matches` | `true` if a file's content matches a pattern |
| `git_branch` | the current branch name |
| `custom` | the result of a command, allowed only in your own top-level config |

A fact that isn't declared reads as `null` in a `when:` clause, which is
false. A misspelled fact name therefore turns a rule off rather than failing,
and `alint facts` is the quickest way to see which names exist.

## Debugging a rule that never fires

1. `alint explain <rule-id>` shows the rule's `when:` clause.
2. `alint facts` shows the value of each fact the clause reads.
3. If a fact has the wrong value, override it: declare a fact with the same
   id in your own `facts:` block, as the bundled rulesets suggest when their
   detection heuristic doesn't fit your layout.

## Examples

Resolved value of every fact in the effective config:

```bash
alint facts
```

Against another checkout:

```bash
alint facts path/to/repo
```

As JSON, for a script or a CI log:

```bash
alint facts --format json
```

```json
{
  "facts": [
    {
      "id": "has_python",
      "kind": "any_file_exists",
      "value": true
    },
    {
      "id": "n_py_files",
      "kind": "count_files",
      "value": 1
    }
  ],
  "kind": "facts",
  "schema_version": 1
}
```

(Shortened to the first two facts; the output lists every fact.)

## Reference

```
Evaluate the config's `facts:` entries and print resolved values.

A debugging aid for `when:` clauses: prints the resolved value of every `facts:` entry in the
effective config.

Usage: alint facts [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to evaluate facts against

          [default: .]
```

The [global options](/docs/cli/#global-options) apply to `alint facts` too, where they are relevant.

## See also

- [Scoping](/docs/concepts/targeting/scoping/): `when:`, `paths:` and `scope_filter:`
- [Configuration: `facts`](/docs/configuration/#facts): declaring facts
- [`alint explain`](/docs/cli/explain/): the `when:` clause of one rule
