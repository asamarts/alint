---
title: 'command'
description: 'alint rule kind `command` (Plugin (tier 1) family).'
sidebar:
  order: 1
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

**Trust gate.** `command` rules are only allowed in the user's own top-level config. A `kind: command` rule introduced via `extends:` (local file, HTTPS URL, or `alint://bundled/`) is a load-time error — the same gate that protects `custom:` facts. Adopting a published ruleset must never imply granting it arbitrary process execution.

`--changed` interaction: `command` is a per-file rule, so under `alint check --changed` it spawns only for files in the diff. The expensive check is automatically incremental in CI.

---

