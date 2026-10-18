---
title: 'pinned_references'
description: 'alint pinned_references rule (content): Require executable references in supported CI and container files to identify immutable artifacts rather...'
sidebar:
  order: 16
categories: ['content', 'security-unicode-sanity']
---

Require executable references in supported CI and container files to identify immutable artifacts rather than mutable tags or branches.

With `format: auto` (the default), the repository-relative filename selects GitHub Actions workflows and `action.yml`, pre-commit, CircleCI, GitLab CI, Cloud Build, or Dockerfile semantics. Set `format:` explicitly for a nonstandard filename. The rule checks GitHub `steps[*].uses`, reusable-workflow `jobs.*.uses`, composite-action steps, job/service images, pre-commit `rev`, CircleCI orbs and images, GitLab images and services, Cloud Build step images, Dockerfile `FROM`, and HTTP(S) `ADD`.

GitHub and pre-commit references must end in a full 40- or 64-hex Git object id. Container references must carry `@sha256:` plus 64 lowercase hexadecimal characters; a readable tag may remain before the digest. CircleCI registry orbs accept an exact `major.minor.patch` version because published orb versions are immutable. Local GitHub `./…` actions, pre-commit `local`/`meta` repositories, `FROM scratch`, and local Dockerfile `ADD` sources are exempt. Remote `ADD` requires `--checksum=sha256:<64-hex>`.

YAML aliases and merge keys are resolved with explicit-key precedence, while findings retain navigable source ranges. Unknown auto-detected paths and malformed/non-UTF-8 inputs fail as analysis failures instead of passing silently. Variable references fire because the checked file does not prove an immutable value. Check-only and offline: resolving a tag to a SHA/digest is a network-gated suggestion, not a deterministic edit.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `format` | one of `auto` \| `github_actions` \| `pre_commit` \| `circleci` \| `gitlab_ci` \| `cloud_build` \| `dockerfile` |  | `auto` | Parser and semantic selector set. `auto` infers it from the path. |

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Mutable CI and container references are reported

The rule fires on this repository:

```text
.circleci/config.yml
.github/workflows/ci.yml
.gitlab-ci.yml
.pre-commit-config.yaml
Dockerfile
actions/demo/action.yml
cloudbuild.yaml
```

```yaml title=".circleci/config.yml"
version: 2.1
orbs:
  node: circleci/node@volatile
```

```yaml title=".github/workflows/ci.yml"
jobs:
  call:
    uses: owner/repo/.github/workflows/test.yml@main
```

```yaml title=".gitlab-ci.yml"
test:
  image: alpine:latest
  script: echo test
```

```yaml title=".pre-commit-config.yaml"
repos:
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v5.0.0
  - repo: https://example.test/missing-rev
```

```text title="Dockerfile"
FROM rust:1.90
ADD https://example.test/tool /usr/local/bin/tool
```

```yaml title="actions/demo/action.yml"
runs:
  using: composite
  steps:
    - uses: actions/checkout@v4
```

```yaml title="cloudbuild.yaml"
steps:
  - name: gcr.io/cloud-builders/docker:latest
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: immutable-build-inputs
    kind: pinned_references
    paths:
      - .github/workflows/*.yml
      - "**/action.yml"
      - .pre-commit-config.yaml
      - .circleci/config.yml
      - .gitlab-ci.yml
      - cloudbuild.yaml
      - Dockerfile
    level: error
```

`alint check` reports:

```ansi
[2m--- .circleci/config.yml -------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m3:9[0m  CircleCI orb is not pinned to an exact immutable version

[2m--- .github/workflows/ci.yml ---------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m3:11[0m  GitHub Actions reference is not pinned to a full commit SHA

[2m--- .gitlab-ci.yml -------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m2:10[0m  container image is not pinned to a full sha256 digest

[2m--- .pre-commit-config.yaml ----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m3:10[0m  pre-commit repository is not pinned to a full commit SHA
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m4:11[0m  external pre-commit repository has no pinned `rev`

[2m--- Dockerfile -----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m1:1[0m  Dockerfile base image is not pinned to a full sha256 digest
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m2:1[0m  remote Dockerfile ADD source has no full sha256 checksum

[2m--- actions/demo/action.yml ----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m4:13[0m  GitHub Actions reference is not pinned to a full commit SHA

[2m--- cloudbuild.yaml ------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimmutable-build-inputs[0m
              [2m2:11[0m  container image is not pinned to a full sha256 digest

[2mSummary (9 violations):[0m
  [1m[31mx 9 errors[0m
  0 passing [2m*[0m 1 failing
```

### Full commits digests exact orbs and local references pass

This repository is compliant:

```text
.circleci/config.yml
.github/workflows/ci.yml
.gitlab-ci.yml
.pre-commit-config.yaml
Dockerfile
actions/demo/action.yml
```

```yaml title=".circleci/config.yml"
version: 2.1
orbs:
  node: circleci/node@5.2.0
jobs:
  test:
    docker:
      - image: cimg/base:current@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

```yaml title=".github/workflows/ci.yml"
jobs:
  call:
    uses: owner/repo/.github/workflows/test.yml@0123456789abcdef0123456789abcdef01234567
  build:
    container: ubuntu:24.04@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    steps:
      - uses: ./actions/local
```

```yaml title=".gitlab-ci.yml"
test:
  image: alpine:latest@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  script: echo test
```

```yaml title=".pre-commit-config.yaml"
repos:
  - repo: local
  - repo: meta
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: 0123456789abcdef0123456789abcdef01234567
```

```text title="Dockerfile"
FROM scratch
ADD local-tool /usr/local/bin/tool
ADD --checksum=sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef https://example.test/tool /usr/local/bin/remote-tool
```

```yaml title="actions/demo/action.yml"
runs:
  using: composite
  steps:
    - uses: actions/checkout@0123456789abcdef0123456789abcdef01234567
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: immutable-standard-inputs
    kind: pinned_references
    paths:
      - .github/workflows/*.yml
      - "**/action.yml"
      - .pre-commit-config.yaml
      - .circleci/config.yml
      - .gitlab-ci.yml
      - Dockerfile
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

