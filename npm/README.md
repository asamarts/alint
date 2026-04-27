# @a-lint/alint

npm install path for [alint](https://github.com/asamarts/alint), a
language-agnostic linter for repository structure, file existence,
filename conventions, and file content rules.

This package downloads and wraps the platform-matched native alint
binary at install time. The package itself ships **no JavaScript runtime
behaviour** — it's a thin shim around the same release tarballs the
`install.sh` and Homebrew paths consume. Same SHA-256 verification, same
upstream artifacts.

## Install

```bash
# project-local
npm install --save-dev @a-lint/alint
npx alint check

# global (puts `alint` on PATH)
npm install -g @a-lint/alint
alint check
```

The npm package version tracks alint releases exactly: `npm install
@a-lint/alint@0.5.11` downloads alint v0.5.11.

## Supported platforms

| OS      | Architectures        |
|---------|----------------------|
| Linux   | x64, arm64 (musl)    |
| macOS   | x64, arm64           |
| Windows | x64                  |

For unsupported platforms, install via `cargo install alint`,
[Homebrew](https://github.com/asamarts/alint#homebrew), or by
downloading the tarball directly from
[GitHub Releases](https://github.com/asamarts/alint/releases).

## Skipping the postinstall download

Set `ALINT_SKIP_INSTALL=1` before `npm install`. The shim will install
without staging a binary, and `alint` invocations will print a clear
"binary not found" error pointing at the missing step. Useful for CI
systems that snapshot `node_modules` and don't want a network hop on
every restore.

## How it works

1. `package.json` declares `bin/alint.js` as the npm-exposed bin.
2. `npm install` runs `install.js` (postinstall):
   - Detects `process.platform` + `process.arch`.
   - Downloads
     `https://github.com/asamarts/alint/releases/download/v<ver>/alint-v<ver>-<target>.tar.gz`
     plus its `.sha256` companion.
   - Verifies SHA-256.
   - Extracts the `alint` (or `alint.exe`) binary into `bin-platform/`.
3. `bin/alint.js` is invoked when the user runs `alint`. It locates the
   staged binary and spawns it with the original argv.

## License

Apache-2.0 OR MIT, matching the upstream alint project.

## Links

- Project: https://github.com/asamarts/alint
- Releases: https://github.com/asamarts/alint/releases
- Issues: https://github.com/asamarts/alint/issues
- Documentation: https://alint.org (when published)
