---
title: 'ci/github-actions@v1'
description: 'ci/github-actions@v1 bundled alint ruleset: GitHub Actions hardening.'
---

GitHub Actions hardening. Guided by the two OpenSSF Scorecard
checks that have the strongest supply-chain signal:

  - "Token-Permissions" — declare the GITHUB_TOKEN scope
    explicitly at workflow level (or narrower).
  - "Pinned-Dependencies" — third-party actions are pinned to
    full commit SHAs, not mutable branches or floating tags.

Plus one readability rule: every workflow has a `name:` so the
Actions UI doesn't fall back to the filename.

Workflow checks remain scoped to `.github/workflows/*.y{,a}ml`. Action pinning also covers
root and nested `action.yml` / `action.yaml` composite-action manifests, so the ruleset
no-ops only in repositories containing neither workflows nor action manifests.

Defaults are non-blocking:
  - `warning` for hardening (permissions + action pinning)
  - `info` for readability

Opt in to stricter enforcement by overriding the rule id in
your own `.alint.yml` (`level: error`), or disable a rule with
`level: off`.

## Adopt with

```yaml
extends:
  - alint://bundled/ci/github-actions@v1
```

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [gha-workflow-contents-read](#gha-workflow-contents-read)<br>`warning` | Workflow grants (or defaults to) write access to the GITHUB\_TOKEN. |
| [gha-pin-actions-to-sha](#gha-pin-actions-to-sha)<br>`warning` | External action, workflow, or container is not pinned to an immutable commit or digest. |
| [gha-workflow-has-name](#gha-workflow-has-name)<br>`info` | Workflow has no `name:` field; the Actions UI will show the filename instead. |

## Rules

### `gha-workflow-contents-read`

OpenSSF Scorecard Token-Permissions check: a workflow should restrict the GITHUB\_TOKEN so a compromised step can't push to the repo. Fires only when a workflow grants (or defaults to) write -- it declares NO permissions anywhere (no workflow-level block AND no per-job block, so it inherits the broad default), OR declares `write-all`, OR `contents: write` at the workflow level. PASSES `contents: read`/`none`, `read-all`, `{}` (empty), and per-job-only permissions.

Scope note: the write checks are workflow-level. A job-level `write-all`/`contents: write`, or a workflow where only SOME jobs declare permissions, is treated as declared and passes -- per-job scoping is the deliberate escape hatch.

The `$[?...]` filter selects the bad state; `yaml_path_absent` emits ONE file-level violation when it matches (a value-op kind would fan out to one warning per top-level key). Telling "no permissions at all" apart from "per-job permissions" needs this two-path disjunction, which the value-comparison kinds can't express.

- **kind**: [`yaml_path_absent`](/docs/rules/structured-query/yaml_path_absent/)
- **level**: `warning`
- **policy**: <https://docs.github.com/en/actions/security-guides/automatic-token-authentication#permissions-for-the-github_token>

> Workflow grants (or defaults to) write access to the GITHUB\_TOKEN. Restrict it: declare `permissions: contents: read` (or `read-all`, or `{}`) at the workflow level, or set per-job `permissions:`, so a compromised step can't push to the repo.

```yaml
- id: gha-workflow-contents-read
  kind: yaml_path_absent
  paths: [".github/workflows/*.yml", ".github/workflows/*.yaml"]
  path: "$[?((!$.permissions && count($.jobs[*].permissions)==0) || $.permissions=='write-all' || $.permissions.contents=='write')]"
  level: warning
  message: >-
    Workflow grants (or defaults to) write access to the
    GITHUB_TOKEN. Restrict it: declare `permissions: contents:
    read` (or `read-all`, or `{}`) at the workflow level, or set
    per-job `permissions:`, so a compromised step can't push to
    the repo.
  policy_url: "https://docs.github.com/en/actions/security-guides/automatic-token-authentication#permissions-for-the-github_token"
```

### `gha-pin-actions-to-sha`

OpenSSF Scorecard Pinned-Dependencies check, now backed by the consumer-aware `pinned_references` kind. It covers ordinary and Docker action steps, reusable-workflow `jobs.*.uses`, composite-action steps, and workflow container/service images without treating unrelated YAML strings as dependencies.

Local `./` action and workflow references are exempt: they live in the same repository and cannot be SHA-pinned. External Actions references require a full 40- or 64-hex Git object id; container references require a full sha256 digest.

- **kind**: [`pinned_references`](/docs/rules/content/pinned_references/)
- **level**: `warning`
- **policy**: <https://docs.github.com/en/actions/security-guides/security-hardening-for-github-actions#using-third-party-actions>

> External action, workflow, or container is not pinned to an immutable commit or digest. Pin Actions with `@<full-commit-sha>` and images with `@sha256:<64-hex-digest>` so a compromised tag cannot silently change what runs. Local `./` references are allowed.

```yaml
- id: gha-pin-actions-to-sha
  kind: pinned_references
  paths:
    - ".github/workflows/*.yml"
    - ".github/workflows/*.yaml"
    - "action.yml"
    - "action.yaml"
    - "**/action.yml"
    - "**/action.yaml"
  format: github_actions
  level: warning
  message: >-
    External action, workflow, or container is not pinned to an immutable
    commit or digest. Pin Actions with `@<full-commit-sha>` and images with
    `@sha256:<64-hex-digest>` so a compromised tag cannot silently change
    what runs. Local `./` references are allowed.
  policy_url: "https://docs.github.com/en/actions/security-guides/security-hardening-for-github-actions#using-third-party-actions"
```

### `gha-workflow-has-name`

Workflows without `name:` display as the filename in the Actions UI. Cheap readability win; purely cosmetic.

- **kind**: [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- **level**: `info`

> Workflow has no `name:` field; the Actions UI will show the filename instead. Add a human-readable `name:` at the top.

```yaml
- id: gha-workflow-has-name
  kind: yaml_path_matches
  paths: [".github/workflows/*.yml", ".github/workflows/*.yaml"]
  path: "$.name"
  matches: '^.+$'
  level: info
  message: >-
    Workflow has no `name:` field; the Actions UI will show the
    filename instead. Add a human-readable `name:` at the top.
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/ci/github-actions@v1
rules:
  - id: gha-workflow-contents-read
    level: off
  - id: gha-pin-actions-to-sha
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/ci/github-actions@v1
    except: [gha-workflow-contents-read]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/ci/github-actions.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/ci/github-actions.yml) in the alint repo.
