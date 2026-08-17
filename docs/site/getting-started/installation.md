---
title: Installation
description: Install alint via Homebrew, install.sh, npm, cargo (or cargo-binstall), Docker/Podman, or from source.
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

## install.sh (Linux + macOS)

```bash
curl -sSL https://alint.org/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball from GitHub Releases, verifies its SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). This path is shell-based, so it does not cover Windows; Windows users have [npm](#npm), [cargo](#cargo), or the manual tarball (see [Windows](#windows)).

Pin a specific version (and skip the "latest release" GitHub API lookup, which can rate-limit on shared CI egress IPs):

```bash
ALINT_VERSION=v0.15.0 curl -sSL https://alint.org/install.sh | bash
```

Supply-chain note: the installer verifies the SHA-256 of the release tarball it downloads, but the script itself is fetched from the `main` branch (`alint.org/install.sh` redirects there). To pin the installer too, point `curl` at a release tag instead of `main` (for example `https://raw.githubusercontent.com/asamarts/alint/v0.15.0/install.sh`), or download it from the [Releases page](https://github.com/asamarts/alint/releases) and review it before running.

## npm

```bash
npm install -g @asamarts/alint
```

The [`@asamarts/alint`](https://www.npmjs.com/package/@asamarts/alint) package is a thin wrapper: on install it downloads the platform-matched native binary from GitHub Releases and verifies its SHA-256. Handy in Node/JS projects and CI that already have npm. Zero-install works too:

```bash
npx @asamarts/alint check
```

Supports Linux (x64/arm64), macOS (x64/arm64), and Windows (x64). The install runs a postinstall script, so it needs network access at install time and does not work under `npm install --ignore-scripts`, Bun's `bunx`, or **pnpm 10+** (which blocks dependency build scripts by default — run `pnpm approve-builds @asamarts/alint` to allow it). A future release moves to per-platform packages to lift those limits.

## cargo

```bash
cargo install alint
```

Builds from source against the current stable Rust toolchain (requires rustc 1.85+ and `cargo` on `$PATH`). To install a **pre-built** binary instead of compiling, use [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall alint
```

`cargo binstall` attempts to fetch a pre-built release tarball (verifying its checksum) instead of compiling, falling back to a source build if it cannot resolve one — much faster on CI and low-powered machines when the pre-built path is taken.

## Docker / Podman

A distroless multi-arch image (`linux/amd64`, `linux/arm64`) is published to ghcr.io on each release:

```bash
# Lint the current directory:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest

# Pin to an exact version:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.15.0 check
```

The image is OCI-standard, so **Podman runs it unchanged** — just use the fully-qualified name (Podman does not assume a default registry):

```bash
podman run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest check
```

The image runs as the distroless `nonroot` user (UID 65532); host files must be world-readable. To apply fixes and preserve host ownership, pass `-u`:

```bash
docker run --rm -u $(id -u):$(id -g) -v "$PWD:/repo" ghcr.io/asamarts/alint:latest fix
```

Also published: the bare semver (`:0.15.0`), the `:<major>.<minor>` rolling channel, and the raw git tag (`:v0.15.0`).

## Windows

The `install.sh` one-liner is shell-based and does not cover Windows. On Windows, use:

- **npm:** `npm install -g @asamarts/alint` (see [npm](#npm)) — the simplest path;
- **cargo:** `cargo install alint` or `cargo binstall alint` (see [cargo](#cargo));
- **manual:** download `alint-v0.15.0-x86_64-pc-windows-msvc.tar.gz` from the [Releases page](https://github.com/asamarts/alint/releases), extract, and put `alint.exe` on your `PATH`.

Note on manually-downloaded binaries: a tarball downloaded through a **browser** carries a "mark of the web", so the first run can trip Windows SmartScreen ("More info → Run anyway") or, on macOS, Gatekeeper quarantine — clear it with `xattr -d com.apple.quarantine ./alint`. Binaries fetched by `curl`/npm/cargo/Homebrew/Docker carry no such mark and run without prompts.

## Enterprise mirror / offline install

Air-gapped setups can install without direct github.com access:

- **Docker:** re-tag and push the image into your internal registry, then pull from there.
- **npm:** once a future release ships per-platform packages, point `.npmrc` `registry=` at your internal mirror; today the postinstall wrapper fetches from github.com.
- **cargo:** use a [source replacement](https://doc.rust-lang.org/cargo/reference/source-replacement.html) mirror for the from-source build.

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

## Uninstall

alint's footprint is the binary itself (no managed config or data directory), plus — only if you used remote `extends:` rulesets — a cache under your platform cache dir (`~/.cache/alint/` on Linux). Remove the binary with whatever installed it: `brew uninstall alint`, `npm uninstall -g @asamarts/alint`, `cargo uninstall alint`, or for `install.sh`, `rm ~/.local/bin/alint`; delete the cache dir too if you used remote rulesets.
