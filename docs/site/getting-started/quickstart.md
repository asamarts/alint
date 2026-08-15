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

Run `alint --help` for a one-line summary of every command, or `alint <cmd> --help` for a command's full options.

## Inspecting your rules

`alint list` shows the effective rules (after `extends:` and overrides), each with its kind and a marker for a conditional (`[when]`) or auto-fixable (`[fix]`) rule:

```text
error    needs-readme  file_exists [fix]
warning  no-bak  file_absent
```

`alint explain <id>` resolves a single rule to its full definition — its kind and categories, a one-line `summary:` of what the kind checks plus a `docs:` link to the reference, the level, the `paths:` scope, any kind-specific `options:`, the author `message:`, the `when:` clause as authored, and a one-line description of the auto-fix:

```text
id:         needs-readme
kind:       file_exists
categories: existence
summary:    Every glob match in paths must correspond to a real file.
docs:       https://alint.org/docs/rules/existence/file_exists/
level:      error
paths:      README.md
message:    README.md must exist.
fix:        create README.md (8 bytes)
```

A rule whose kind takes options shows them under an `options:` block — here a `file_max_size` rule:

```text
id:         readme-size-cap
kind:       file_max_size
categories: content, structure
summary:    File must be at most max_bytes in size.
docs:       https://alint.org/docs/rules/content/file_max_size/
level:      warning
paths:      **/*.md
options:    max_bytes: 10240
```

Pass `--no-docs` to drop the `docs:` line (handy for narrow terminals and screen recordings). Add `--format json` to either for the machine shape: `explain <id> --format json` carries `rule_kind`, `categories`, `summary`, `docs`, `paths`, `options`, `message`, `when`, `conditional`, `fixable`, and `fix`.

## Discovering rule kinds

`alint list` and `alint explain` describe the rules configured in *this* repo. To browse the whole catalog of rule kinds alint ships — independent of any config, so it works in any directory — use `alint rules`:

```bash
alint rules list                     # every kind, with its categories and a one-line summary
alint rules list --search "at most"  # match the kind name, its aliases, AND the summary text
alint rules show file_header         # one kind in detail (accepts an alias, e.g. `header`)
```

`--search` matches summary text now, so a concept surfaces its kinds even when their names don't contain the word:

```text
Rule catalog (2 kinds)

  file_max_lines  Content, Structure  (alias: max_lines)
                  File must have at most max_lines lines, using the same accounting as file_min_lines.
  file_max_size   Content, Structure  (alias: max_size)
                  File must be at most max_bytes in size.
```

`alint rules show <kind>` prints one kind's summary, categories, aliases, and docs link (and accepts an alias, resolving to the canonical kind):

```text
file_header
  The first N lines must match a regex (line-oriented).
  categories: Content
  aliases:    header
  docs:       https://alint.org/docs/rules/content/file_header/
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

The [`agent` format](/docs/reference/output-formats/agent/) has a dedicated reference covering its full report shape.

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default — use `--fail-on-warning` to flip that.

## Where to next

- [Bundled Rulesets](/docs/bundled-rulesets/) — 22 one-line baselines covering Rust, Python, Go, Node, Java, monorepos, GitHub Actions hardening, agent hygiene, license compliance, and more.
- [Cookbook](/docs/cookbook/) — copy-pasteable patterns for real-world repo-maintenance tasks.
- [Configuration](/docs/configuration/) — full `.alint.yml` field reference.
- [Concepts](/docs/concepts/) — the rule model, scopes, when-expressions, composition.
