---
title: 'command'
description: 'Shell out to an external CLI per matched file. alint command rule, plugin (tier 1) family.'
sidebar:
  order: 1
categories: ['plugin-tier-1']
---

Shell out to an external CLI per matched file. Exit `0` is a pass; non-zero is one violation whose message is the (truncated) stdout+stderr. Working directory is the repo root; stdin is closed.

```yaml
- id: workflows-clean
  kind: command
  paths: ".github/workflows/*.{yml,yaml}"
  command: ["actionlint", "{path}"]
  level: error
```

Argv tokens accept the same path-template substitutions as `pair` and `for_each_dir`: `{path}`, `{dir}`, `{stem}`, `{ext}`, `{basename}`, `{parent_name}`. The first token is the program (looked up via `PATH` if it's a bare name).

Environment threaded into the child:

| Var | Value |
|---|---|
| `ALINT_PATH` | matched path (relative to root) |
| `ALINT_ROOT` | absolute repo root |
| `ALINT_RULE_ID` | the rule's `id:` |
| `ALINT_LEVEL` | `error` / `warning` / `info` |
| `ALINT_VAR_<NAME>` | one per top-level `vars:` entry |
| `ALINT_FACT_<NAME>` | one per resolved fact, stringified |

`timeout: <seconds>` (default 30) bounds each invocation; past the limit the child is killed and a violation reports the timeout.

**Trust gate.** Every process-spawning rule kind — `command`, `generated_file_fresh`, and `command_idempotent` — is allowed only in the user's own top-level config. Any of them introduced via `extends:` (local file, HTTPS URL, or `alint://bundled/`) is a load-time error — the same gate that protects `custom:` facts. Adopting a published ruleset must never imply granting it arbitrary code execution.

**Path confinement + `allow_out_of_root`.** Every config-declared path is confined to the repo root — a rule can't read or resolve a file outside the tree it was pointed at. The top-level-only `allow_out_of_root:` key relaxes this for *reads* (`json_schema_passes` `schema_path:`, `pair_hash` `target:`, `registry_paths_resolve` `source:`) when a trusted config must reference an external file. It is **rejected from `extends:`'d rulesets** (same trust model as the spawn gate above) and a permitted read emits a note. See [Configuration → `allow_out_of_root`](/docs/configuration/#allow_out_of_root).

`--changed` interaction: `command` is a per-file rule, so under `alint check --changed` it spawns only for files in the diff. The expensive check is automatically incremental in CI.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `command` | list of string | yes |  | Argv tokens. The first token is the program (looked up via PATH if it's a bare name); remaining tokens accept `{path}` and friends. |
| `timeout` | integer (>= 1) |  | `null` | Per-file timeout in seconds. Default 30. Past this, the child is killed and a violation reports the timeout. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
