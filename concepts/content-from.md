---
title: '`content_from:` for fix ops'
description: Reference fix-op body content from a file path instead of inlining it in YAML. Paths resolve relative to the lint root; bytes are read at fix-apply time.
sidebar:
  order: 8
---

The three content-providing fix ops — `file_create`, `file_prepend`, `file_append` — accept a `content_from: <path>` field as an alternative to inline `content:`. The path resolves relative to the lint root and is read when `alint fix` actually runs, so the source file's bytes flow into the target without round-tripping through YAML.

## When to reach for it

LICENSE / NOTICE / CONTRIBUTING / SPDX boilerplate is awkward to inline. A real Apache-2 LICENSE is ~10 KB; pasting it into YAML quotes is fragile (escape rules, indentation drift, code-search hits in the wrong column). Stash the canonical bytes under `.alint/templates/`, point `content_from:` at it:

```yaml
- id: must-have-license
  kind: file_exists
  paths: [LICENSE]
  root_only: true
  level: error
  fix:
    file_create:
      content_from: ".alint/templates/LICENSE-MIT.txt"
```

`alint fix` reads `.alint/templates/LICENSE-MIT.txt` from disk and writes the result to `LICENSE`. The template file lives in version control next to the rule, which makes review natural (the LICENSE bytes the fixer would create are on the diff).

## Mutual exclusion with `content:`

Exactly one of `content:` / `content_from:` must be set on a fix op. Both is a config error; neither is a config error. The XOR is enforced at config-load time, before any rule fires.

## Resolution semantics

The path is **relative to the lint root** — the path you pass to `alint check` (or `alint fix`), or the current directory if you don't. Same root the rest of alint uses. Absolute paths work but reduce portability across checkouts.

The source file is read at **fix-apply time**, not at config-load time. Two consequences:

1. The template file doesn't need to exist when `alint check` runs — only when `alint fix` runs against a violating tree. A check-only CI workflow won't fail because the template is missing.
2. Editing the template between `alint check` and `alint fix` is fine — the latest bytes go into the target.

## Missing source = `Skipped`, not `Error`

If the `content_from:` path doesn't exist or can't be read at fix-apply time, alint reports a `Skipped` outcome with a clear message ("`content_from` `path` could not be read"). It does NOT abort the whole fix run or leave a half-written file. Same advisory posture as the rest of the fix path.

## The three fix ops

The same `content_from:` shape works on all three:

```yaml
# file_create — typical for missing top-level documents
fix:
  file_create:
    content_from: ".alint/templates/LICENSE.txt"

# file_prepend — typical for SPDX / copyright headers
fix:
  file_prepend:
    content_from: ".alint/templates/spdx-header.rs"

# file_append — typical for trailing trailers / signoffs
fix:
  file_append:
    content_from: ".alint/templates/footer.md"
```

## Working with templates in a monorepo

`content_from:` paths are resolved against the root the user invoked `alint check` from. With `nested_configs: true`, a sub-config in `packages/foo/.alint.yml` still resolves `content_from:` against the workspace root — so a single `.alint/templates/` directory at the root supplies content to every sub-package without duplication. Pair this with rule templates for "every package gets the same generated NOTICE file" patterns.
