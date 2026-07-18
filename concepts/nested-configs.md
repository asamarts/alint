---
title: 'Nested .alint.yml (monorepo layering)'
description: 'alint concept: nested .alint.yml (monorepo layering).'
---

<likec4-view view-id="monorepoNesting"></likec4-view>

Opt into per-subtree configs by setting `nested_configs: true` on the root `.alint.yml`:

```yaml
# /.alint.yml (root)
version: 1
nested_configs: true
rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md"]
    root_only: true
    level: warning
```

```yaml
# /packages/frontend/.alint.yml
version: 1
rules:
  - id: frontend-ts-final-newline
    kind: final_newline
    paths: "**/*.ts"
    level: warning
```

```yaml
# /packages/backend/.alint.yml
version: 1
rules:
  - id: backend-rust-snake-case
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
```

At load time, alint walks the tree (respecting `.gitignore` + `ignore:`), picks up every nested `.alint.yml` / `.alint.yaml`, and **prefixes each nested rule's path-like fields** (`paths`, `select`, `primary`) with the relative directory the config lives in. So the frontend rule above evaluates as if it were `paths: "packages/frontend/**/*.ts"` at the root — it fires only on frontend TypeScript files.

### Restrictions (MVP)

- Only the root config sets `nested_configs: true`. Nested configs can't spawn further nesting.
- Nested configs can only declare `version:` and `rules:` — `extends:`, `facts:`, `vars:`, `ignore:`, `respect_gitignore:`, `fix_size_limit:`, and `allow_out_of_root:` are root-only.
- Every rule in a nested config must have a path-like scope field (`paths`, `select`, or `primary`). Rules without any (e.g. `no_submodules`, which is hardcoded to repo root) can't be nested.
- Absolute paths and `..`-prefixed globs are rejected — they'd escape the subtree the config is supposed to confine.
- Rule-id collisions across configs are rejected with a clear error. Per-subtree overrides aren't supported yet; if you want to disable a root rule under one subtree, use a `when:` gate on the root rule for now.
