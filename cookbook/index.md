---
title: Cookbook
description: Real-world alint patterns — copy-pasteable, each targeting a specific repo-maintenance problem.
sidebar:
  order: 1
---

Each pattern below is meant to be copied into a `.alint.yml` and customized. If you're starting from scratch, [Getting Started](/docs/getting-started/quickstart/) is a better entry point.

## 1. One-line baseline from a bundled ruleset

The shortest useful `.alint.yml` — adopt the OSS-hygiene baseline and nothing else. Good for "we just want README / LICENSE / no merge markers" rigour on a fresh repo.

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

## 2. Compose several bundled rulesets for a specific stack

A Rust monorepo wants OSS docs + Rust-idiomatic structure + layout checks + no tracked build artefacts:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1                              # Cargo.toml, target/ ban, snake_case
  - alint://bundled/monorepo@v1                          # every crate has README
  - alint://bundled/hygiene/no-tracked-artifacts@v1      # node_modules, target/, .DS_Store…
  - alint://bundled/hygiene/lockfiles@v1                 # Cargo.lock only at root
```

Language-specific rulesets (`rust`, `node`, `python`, `go`) are gated by facts (`when: facts.is_<lang>`) and silently no-op in projects where they don't apply, so layering them is cheap.

## 3. Override a bundled rule without restating its body

Children in an `extends:` chain only need to declare the fields that change. The inherited `kind`, `paths`, `pattern`, etc. carry over:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1

rules:
  # Turn a warning into a blocking error for our repo:
  - id: oss-license-exists
    level: error

  # Silence a rule we've deliberately opted out of:
  - id: oss-code-of-conduct-exists
    level: off
```

Unknown-id overrides are flagged at config load, so typos don't silently pass.

## 4. Adopt only part of a bundled ruleset

When you want most of a bundled ruleset but not all of it, filter at the `extends:` entry with `only:` or `except:` (mutually exclusive). Unknown rule ids in either list are flagged at load time.

```yaml
version: 1
extends:
  # Most of oss-baseline, minus the CoC nag:
  - url: alint://bundled/oss-baseline@v1
    except: [oss-code-of-conduct-exists]

  # Just the pinning check from the CI ruleset, nothing else:
  - url: alint://bundled/ci/github-actions@v1
    only: [gha-pin-actions-to-sha]
```

## 5. Enforce values inside `package.json` with structured queries

`json_path_equals` applies a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) query and checks the value. Missing fields are treated as violations (conservative default — scope narrowly if a field is truly optional).

```yaml
version: 1
rules:
  - id: require-mit-license
    kind: json_path_equals
    paths: "packages/*/package.json"
    path: "$.license"
    equals: "MIT"
    level: error

  - id: semver-package-version
    kind: json_path_matches
    paths: "packages/*/package.json"
    path: "$.version"
    matches: '^\d+\.\d+\.\d+$'
    level: error
```

## 6. Lock down GitHub Actions workflows

`yaml_path_equals` for workflow-wide permissions; `yaml_path_matches` for action-SHA pinning. Both use the same JSONPath engine — YAML is coerced through serde into a JSON value first, so array and wildcard expressions work the same way. If you want the full set without typing them, `extends: [alint://bundled/ci/github-actions@v1]` ships these rules plus a `name:` presence check.

`if_present: true` on the pinning rule means workflows with only `run:` steps (no `uses:` at all) are silently OK — the rule only fires on actual matches that fail the regex.

```yaml
version: 1
rules:
  # OpenSSF: workflows should declare `permissions.contents: read` explicitly.
  - id: workflow-contents-read
    kind: yaml_path_equals
    paths: ".github/workflows/*.yml"
    path: "$.permissions.contents"
    equals: "read"
    level: error

  # Security practice: pin third-party actions to a full commit SHA,
  # not a mutable @v4-style tag. `$.jobs.*.steps[*].uses` iterates
  # every step across every job. `if_present: true` skips workflows
  # that have no `uses:` at all.
  - id: pin-actions-to-sha
    kind: yaml_path_matches
    paths: ".github/workflows/*.yml"
    path: "$.jobs.*.steps[*].uses"
    matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
    if_present: true
    level: warning
```

## 7. Enforce Cargo manifest shape across a workspace

`toml_path_equals` / `toml_path_matches` round out the structured-query family for Rust and Python (`pyproject.toml`) projects.

```yaml
version: 1
rules:
  - id: rust-edition-2024
    kind: toml_path_equals
    paths: "crates/*/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: error

  - id: crate-version-follows-semver
    kind: toml_path_matches
    paths: "crates/*/Cargo.toml"
    path: "$.package.version"
    matches: '^\d+\.\d+\.\d+(-[\w.-]+)?$'
    level: error
```

## 8. Monorepo: every package has README + license + non-stub docs

`for_each_dir` iterates every directory matching `select:` and evaluates the nested `require:` block against each, substituting `{path}` with the iterated directory. `file_min_lines` catches the "README is a title plus `TODO`" case without being pedantic about word count.

```yaml
version: 1
rules:
  - id: every-package-is-documented
    kind: for_each_dir
    select: "packages/*"
    level: error
    require:
      - kind: file_exists
        paths: "{path}/README.md"

      - kind: file_min_lines
        paths: "{path}/README.md"
        min_lines: 5
        level: warning

      - kind: file_exists
        paths: ["{path}/LICENSE", "{path}/LICENSE.md"]
        level: warning
```

## 9. Nested `.alint.yml` for subtree-specific rules

Large repos rarely have a single policy. `nested_configs: true` auto-discovers `.alint.yml` files in subdirectories and scopes each nested rule's `paths` / `select` / `primary` to the subtree it lives in. The frontend team can own `packages/frontend/.alint.yml` without waiting on root-config review:

```yaml
# .alint.yml (repo root)
version: 1
nested_configs: true
extends:
  - alint://bundled/oss-baseline@v1
```

```yaml
# packages/frontend/.alint.yml
version: 1
rules:
  - id: components-are-pascal-case
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"   # auto-scoped to packages/frontend/**
    case: pascal
    level: error
```

MVP guardrails: nested rules must declare at least one scope field; absolute paths and `..`-prefixed globs are rejected; duplicate rule ids across configs surface with a clear message.

## 10. Auto-fix hygiene on commit

Pair a low-severity rule with a fixer and let `alint fix` do the boring part. Ideal for pre-commit or editor-save hooks.

```yaml
version: 1
rules:
  - id: trim-trailing-whitespace
    kind: no_trailing_whitespace
    paths: ["**/*.md", "**/*.rs", "**/*.yml"]
    level: info
    fix:
      file_trim_trailing_whitespace: {}

  - id: final-newline
    kind: final_newline
    paths: ["**/*.md", "**/*.rs", "**/*.yml"]
    level: info
    fix:
      file_append_final_newline: {}

  - id: no-bak-files
    kind: file_absent
    paths: "**/*.{bak,swp,orig}"
    level: warning
    fix:
      file_remove: {}
```

Preview with `alint fix --dry-run`; apply with `alint fix`. Content-editing fixers honour `fix_size_limit` (default 1 MiB) and skip oversize files rather than rewriting them.

## 11. Conditional rules gated on repo facts

Facts are evaluated once per run and referenced in `when:`. Here: only enforce snake_case Rust filenames when the repo actually *is* a Rust project.

```yaml
version: 1

facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]
  - id: is_typescript
    any_file_exists: ["tsconfig.json", "packages/*/tsconfig.json"]

rules:
  - id: rust-snake-case
    when: facts.is_rust
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error

  - id: ts-kebab-case
    when: facts.is_typescript and not (facts.is_rust)
    kind: filename_case
    paths: "src/**/*.ts"
    case: kebab
    level: warning
```

## 12. Cross-file relationships

`pair` and `unique_by` cover the "every X has a matching Y" and "no two files share a derived key" cases — the ones that ad-hoc shell pipelines usually get wrong on the edges. Template tokens are `{path}`, `{dir}`, `{basename}`, `{stem}`, `{ext}`, `{parent_name}`.

```yaml
version: 1
rules:
  # Every `*.c` source file has a same-directory `*.h` header:
  - id: every-c-has-a-header
    kind: pair
    primary: "src/**/*.c"
    partner: "{dir}/{stem}.h"
    level: error

  # No two Rust source files share a stem anywhere in the repo — a
  # frequent mod-path surprise in larger workspaces:
  - id: unique-rs-stems
    kind: unique_by
    select: "**/*.rs"
    key: "{stem}"
    level: warning
```

## 13. Ban risky characters / files outright

The security-family rules catch categories that are almost never intentional. Trojan-Source (CVE-2021-42574), zero-width tricks, and stray merge markers all lead to "I didn't write that" incidents.

```yaml
version: 1
rules:
  - id: no-merge-markers
    kind: no_merge_conflict_markers
    paths: ["**/*"]
    level: error

  - id: no-bidi
    kind: no_bidi_controls
    paths: ["**/*"]
    level: error
    fix:
      file_strip_bidi_controls: {}

  - id: no-zero-width
    kind: no_zero_width_chars
    paths: ["**/*"]
    level: error
    fix:
      file_strip_zero_width: {}

  - id: no-committed-env
    kind: file_absent
    paths: [".env", ".env.*.local"]
    level: error
```

## 14. Guard an agent-heavy repo

Coding agents (Claude Code, Cursor agent, Copilot agent, Aider, Codex) leave characteristic structural debris — backup-suffix files, scratch / planning docs, debug-print residue, stale `TODO(claude:)` markers, AI-style affirmation prose. The bundled `agent-hygiene@v1` ruleset (shipped in v0.6) catches all of those without overlapping the existing `hygiene/*` set. Pair it with `agent-context@v1` for `AGENTS.md` / `CLAUDE.md` / `.cursorrules` hygiene:

```yaml
version: 1
extends:
  # OS / editor / build / .env junk — covers what agents AND humans leave behind.
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/hygiene/lockfiles@v1
  # Agent-specific patterns — versioned duplicates, scratch docs,
  # debug residue, AI-affirmation prose, model-attributed TODOs.
  - alint://bundled/agent-hygiene@v1
  # AGENTS.md / CLAUDE.md / .cursorrules hygiene (existence, stub
  # guard, bloat guard, stale-path heuristic). Fact-gated, safe
  # no-op when no agent-context file is present.
  - alint://bundled/agent-context@v1
```

Layer with the language-ecosystem rulesets if your stack matches one — they're all `when: facts.is_<lang>` gated, so extending them costs nothing in projects where they don't apply:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/agent-hygiene@v1
  - alint://bundled/agent-context@v1
  - alint://bundled/rust@v1                # or node / python / go / java
  - alint://bundled/monorepo@v1            # if multi-package
```

### Feeding violations back to an agent

`--format=agent` (also accepted as `--format=agentic` or `--format=ai`) emits a flat JSON shape optimised for an LLM to act on. Each violation carries an `agent_instruction` field templated from the rule's message + location + fix availability + policy URL — so an agent loop can read the violation and apply the suggested remediation directly:

```bash
alint check --format=agent
```

```json
{
  "schema_version": 1,
  "format": "agent",
  "summary": {
    "total_violations": 1,
    "by_severity": {"error": 0, "warning": 1, "info": 0},
    "fixable_violations": 0,
    "passing_rules": 5,
    "failing_rules": 1
  },
  "violations": [
    {
      "rule_id": "agent-no-console-log",
      "severity": "warning",
      "file": "src/api.ts",
      "line": 42,
      "column": 1,
      "human_message": "`console.log` / `.debug` / `.trace` left in non-test source. Route through the project logger or remove before merge.",
      "agent_instruction": "warning: `console.log` / `.debug` / `.trace` left in non-test source. Route through the project logger or remove before merge. To resolve: edit src/api.ts:42:1.",
      "fix_available": false
    }
  ]
}
```

A typical agent-harness pattern: after each edit, run `alint check --format=agent`, parse the JSON, address the first violation, repeat until empty. The `agent_instruction` field is intentionally verbose — it's optimised for an LLM to act on without having to re-derive the action from `rule_id` and `human_message` separately.

### Severity escalation

The bundled defaults are deliberately non-blocking on the heuristic checks (`info` for AI-prose patterns, `warning` for clean-up debt, `error` for unambiguous bugs like `debugger;` in production source). Override per-rule once your team is ready to enforce — field-level override means you only have to declare the field you change:

```yaml
version: 1
extends:
  - alint://bundled/agent-hygiene@v1

rules:
  # Promote scratch-doc bans to error before merge, not just warn.
  - id: agent-no-scratch-docs-at-root
    level: error

  # Tighten the affirmation-prose check from info to warning.
  - id: agent-no-affirmation-prose
    level: warning
```

### When you're writing about agent patterns

Projects that *document* these patterns (a how-to guide about AI hygiene, an internal style guide that quotes agent stock phrases, etc.) will trip the prose / TODO rules on their own examples. The `agent-hygiene@v1` defaults already exclude `**/CHANGELOG*`, `**/ROADMAP*`, `**/cookbook/**`, `**/*test*/**`, and `**/fixtures/**` for that reason. If your docs live somewhere else, extend the exclude list — `paths.exclude` field-overrides the bundled list, so list everything you want excluded:

```yaml
version: 1
extends:
  - alint://bundled/agent-hygiene@v1

rules:
  - id: agent-no-affirmation-prose
    paths:
      include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb,md}"]
      exclude:
        - "**/*test*/**"
        - "**/__tests__/**"
        - "**/fixtures/**"
        - "**/CHANGELOG*"
        - "**/ROADMAP*"
        - "**/*.snap"
        - "docs/agent-style.md"            # your custom doc that quotes the patterns
        - "docs/style/**"                  # or a whole directory
```
