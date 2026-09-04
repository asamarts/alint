---
title: pre-commit
description: Run alint as a pre-commit hook.
sidebar:
  order: 2
---

alint ships a [pre-commit](https://pre-commit.com/) hook. The recommended path is the [`alint-pre-commit`](https://github.com/asamarts/alint-pre-commit) mirror, which installs the prebuilt `alint` wheel from PyPI, so the hook is fast and needs no toolchain. Add it to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/asamarts/alint-pre-commit
    rev: v0.16.1
    hooks:
      - id: alint
```

The `alint` hook runs `alint check` against the repo's `.alint.yml` on every commit, blocking commits whose changes introduce errors. The same hook works for any language's repository, since alint lints structure, not code.

The commit gate, plus the manual fix hook:

<likec4-view view-id="preCommitFlow"></likec4-view>

## Auto-fix on demand

A second hook id, `alint-fix`, applies fixers. It's registered under `stages: [manual]` so it does not run on every commit (fixers mutate the working tree). Invoke explicitly:

```bash
pre-commit run alint-fix --all-files
```

## Recommended config

Pin to a tagged release. Updating the `rev:` is how you upgrade alint:

```yaml
repos:
  - repo: https://github.com/asamarts/alint-pre-commit
    rev: v0.16.1
    hooks:
      - id: alint
        # Pass extra args here if you need to:
        args: ["--fail-on-warning"]
```

The mirror's `language: python` hook installs the prebuilt wheel (the same native binary the other channels ship), so there is no Rust toolchain to set up and the hook starts fast.

## Without a PyPI dependency

If you would rather not pull from PyPI, point `repo:` at the alint repository itself. Its `language: rust` hook compiles alint from source on each machine (slower, and it needs a Rust toolchain), but adds no package-registry dependency:

```yaml
repos:
  - repo: https://github.com/asamarts/alint
    rev: v0.16.1
    hooks:
      - id: alint
```
