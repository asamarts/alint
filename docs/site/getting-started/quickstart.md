---
title: Quickstart
description: Write your first .alint.yml and run alint check.
sidebar:
  order: 2
---

The shortest useful `.alint.yml` adopts a bundled ruleset and nothing else.

```yaml
# .alint.yml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

Drop that at the root of your repo, then:

```bash
alint check           # run all rules against the current directory
alint fix --dry-run   # preview the auto-fixes that would be applied
alint fix             # apply every fixable violation in place
alint list            # list effective rules (after extends + overrides)
alint explain <id>    # show a rule's full, resolved definition
alint facts           # evaluate facts against the repo — debug `when:` clauses
```

## Output formats

```bash
alint check --format human    # default; colorized; grouped by file
alint check --format json     # stable, versioned JSON schema
alint check --format sarif    # SARIF 2.1.0 (for GitHub Code Scanning)
alint check --format github   # GitHub Actions workflow commands
alint check --format markdown # PR-comment-friendly tables
alint check --format junit    # CI test-report shape
alint check --format gitlab   # GitLab Code Quality
alint check --format agent    # LLM-shaped JSON with per-violation `agent_instruction`
```

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default — use `--fail-on-warning` to flip that.

## Where to next

- [Bundled Rulesets](/docs/bundled-rulesets/) — nineteen one-line baselines covering Rust, Python, Go, Node, Java, monorepos, GitHub Actions hardening, agent hygiene, license compliance, and more.
- [Cookbook](/docs/cookbook/) — copy-pasteable patterns for real-world repo-maintenance tasks.
- [Configuration](/docs/configuration/) — full `.alint.yml` field reference.
- [Concepts](/docs/concepts/) — the rule model, scopes, when-expressions, composition.
