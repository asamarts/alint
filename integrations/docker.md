---
title: Docker
description: Run alint from a distroless container image.
sidebar:
  order: 3
---

A distroless multi-arch image is published to ghcr.io on every alint release.

## Run against the current directory

```bash
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest
```

The image's `WORKDIR` is `/repo`, so the bind-mount lets alint see your repo as if it were running on the host.

## Pin to a version

```bash
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.9.6 check
```

Tags published per release: the exact git tag (`:v0.9.6`), the bare semver (`:0.9.6`), the `<major>.<minor>` channel (`:0.9`), and `:latest`.

## Apply auto-fixes

The default user is the distroless `nonroot` user (UID 65532). For `alint fix` to write with host file ownership preserved, pass `-u`:

```bash
docker run --rm -u $(id -u):$(id -g) -v "$PWD:/repo" ghcr.io/asamarts/alint:latest fix
```

Without `-u`, fixed files end up owned by UID 65532 on the host — annoying to clean up.

## Why distroless

The base image is `gcr.io/distroless/static-debian12:nonroot`. No shell, no package manager, just glibc-free libc plus alint. Trivy/Grype scans against this base typically come back clean.

The in-image binary is the same statically-linked musl build that ships in the GitHub Release tarballs — byte-for-byte identical, so verifying one verifies the other.

## CI usage

For one-shot lint runs in CI systems that prefer containers over running custom installers:

```yaml
# Generic CI (GitLab, Drone, BuildKite, …)
script:
  - docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.9.6 check --format json
```

For GitHub Actions specifically, the [official Action](/docs/integrations/github-actions/) is more ergonomic.
