---
title: Installation
description: Install alint via Homebrew, install.sh, Docker, cargo, or from source.
sidebar:
  order: 1
---

alint ships as a single native executable with no language runtime, no JVM, and nothing else to install. Pick whichever path matches your environment.

<likec4-view view-id="distributionFlow"></likec4-view>

## Homebrew (macOS + Linuxbrew)

```bash
brew tap asamarts/alint
brew install alint
```

The recommended path on macOS and Linux. The [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint) tap is auto-updated on every release; the formula resolves the matching pre-built tarball for your platform, verifies its SHA-256, and installs to the Homebrew cellar.

## install.sh (Linux + macOS + Windows tarballs)

```bash
curl -sSL https://alint.org/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball from GitHub Releases, verifies its SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). Windows users download the Windows tarball from the [Releases page](https://github.com/asamarts/alint/releases) directly.

Supply-chain note: the installer verifies the SHA-256 of the release tarball it downloads, but the script itself is fetched from the `main` branch (`alint.org/install.sh` redirects there). To pin the installer too, point `curl` at a release tag instead of `main` (for example `https://raw.githubusercontent.com/asamarts/alint/v0.14.2/install.sh`), or download it from the [Releases page](https://github.com/asamarts/alint/releases) and review it before running.

## Docker

A distroless multi-arch image (`linux/amd64`, `linux/arm64`) is published to ghcr.io on each release:

```bash
# Lint the current directory:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest

# Pin to an exact version:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.14.2 check
```

The image runs as the distroless `nonroot` user (UID 65532); host files must be world-readable. To apply fixes and preserve host ownership, pass `-u`:

```bash
docker run --rm -u $(id -u):$(id -g) -v "$PWD:/repo" ghcr.io/asamarts/alint:latest fix
```

Also published: `:<major>.<minor>` (e.g. `:0.10`) and the raw git tag (`:v0.14.2`).

## crates.io

```bash
cargo install alint
```

Builds from source against the current stable Rust toolchain. Requires `cargo` already on `$PATH`.

## From source

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo build --release -p alint
./target/release/alint --help
```

Useful when you want to track `main` between releases or are contributing patches.

## Verify the install

```bash
alint --version
```

Should print `alint <version>` matching the channel you installed from.
