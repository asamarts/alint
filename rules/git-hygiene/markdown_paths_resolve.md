---
title: 'markdown_paths_resolve'
description: 'Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo.'
sidebar:
  order: 3
categories: ['git-hygiene', 'cross-file']
---

Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. Targets the AGENTS.md / CLAUDE.md / `.cursorrules` staleness problem: agent-context files reference paths in inline backticks (`` `src/api/users.ts` ``), and those paths drift as the codebase evolves. The `agent-context-no-stale-paths` rule shipped in v0.6 surfaces *candidates* via a regex; this rule does the precise existence check.

The `prefixes` list is **required** — a backticked token must start with one of these to be considered a path candidate. No defaults: every project's layout differs, and a missing prefix is silent while a wrong default trips false positives.

The scanner skips fenced code blocks (```` ``` ```` / `~~~`) and 4-space-indented blocks; those contain code samples, not factual claims about the tree. Trailing `:line` / `#L<n>` location suffixes are stripped before lookup, as are trailing punctuation and trailing slashes. Glob characters (`*`, `?`, `[`) trigger globset matching against the file index — pass if at least one file matches.

By default the rule skips backticked tokens containing template-variable markers (`{{ }}`, `${ }`, `<…>`). Set `ignore_template_vars: false` to validate them as literal paths.

Check-only — auto-fixing a stale path means guessing the new location, which is unsafe.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `ignore_template_vars` | boolean |  | `true` | When true (default), skip backticked tokens containing `{{ ... }}`, `${ ... }`, or `<...>` template-variable markers. These are placeholders, not real paths. |
| `prefixes` | list of string | yes |  | Whitelist of path-shape prefixes to validate. A backticked token must start with one of these to be considered a path candidate. No defaults — every project's layout differs and the user must declare which prefixes mark a path. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Backticked file paths in Markdown that do not exist

The rule fires on this repository:

```text
AGENTS.md
docs/
docs/extant.md
src/
src/exists.ts
```

`AGENTS.md`:

````markdown
# Agents file

See `src/exists.ts` for the implementation.
Reference `src/missing.ts` and `docs/gone.md`.

```yaml
example: `src/in-codeblock.ts`
```

Inline `npm install` doesn't trigger.
Path with line: `src/exists.ts:42`.
Template var: `src/{{user_id}}.ts`.
````

`docs/extant.md`:

```markdown
# real doc
```

`src/exists.ts`:

```ts
export const x = 1;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: agents-paths-resolve
    kind: markdown_paths_resolve
    paths: ["AGENTS.md"]
    prefixes: ["src/", "docs/"]
    level: warning
```

### Every backticked path in the doc resolves to a real file

This repository is compliant:

```text
AGENTS.md
docs/
docs/guide.md
src/
src/api.ts
src/utils.ts
```

`AGENTS.md`:

````markdown
# Agents file

Production code lives under `src/api.ts` and `src/utils.ts`.
User-facing docs are at `docs/guide.md`.

```yaml
# In code blocks, references aren't validated:
example: `src/anything-goes.ts`
```
````

`docs/guide.md`:

```markdown
# guide
```

`src/api.ts`:

```ts
export function api() {}
```

`src/utils.ts`:

```ts
export const u = 1;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: agents-paths-resolve
    kind: markdown_paths_resolve
    paths: ["AGENTS.md"]
    prefixes: ["src/", "docs/"]
    level: warning
```

