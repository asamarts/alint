# @asamarts/alint

npm install path for [alint](https://github.com/asamarts/alint), a
language-agnostic linter for repository structure, file existence,
filename conventions, and file content rules.

## Install

```bash
# project-local
npm install --save-dev @asamarts/alint
npx alint check

# global (puts `alint` on PATH)
npm install -g @asamarts/alint
alint check

# zero-install
npx @asamarts/alint check
```

pnpm, Yarn (including Plug'n'Play) and Bun work the same way. The npm package
version tracks alint releases exactly: `npm install @asamarts/alint@0.22.1`
runs alint v0.22.1.

## Supported platforms

| Platform package               | OS      | CPU   |
|--------------------------------|---------|-------|
| `@asamarts/alint-linux-x64`    | Linux   | x64 (static musl, any libc)   |
| `@asamarts/alint-linux-arm64`  | Linux   | arm64 (static musl, any libc) |
| `@asamarts/alint-darwin-x64`   | macOS   | x64   |
| `@asamarts/alint-darwin-arm64` | macOS   | arm64 |
| `@asamarts/alint-win32-x64`    | Windows | x64   |
| `@asamarts/alint-win32-arm64`  | Windows | arm64 |

For other platforms, install via `cargo install alint`,
[Homebrew](https://github.com/asamarts/alint#homebrew-macos--linuxbrew), or a
tarball from [GitHub Releases](https://github.com/asamarts/alint/releases), or
point `ALINT_BINARY` at a binary (below).

## How it works

`@asamarts/alint` lists every platform package above as an exact-version
optional dependency. Each platform package declares its `os` and `cpu`, so your
package manager installs only the one that matches. When you run `alint`, the
shim (`bin/alint.js`) finds that package and runs its binary with your
arguments, passing through the exit code.

- **No install script, no download.** Nothing runs at install time, so it works
  under `--ignore-scripts`, with Bun and pnpm's script blocking, offline, and
  from a registry mirror (point `.npmrc` `registry=` at it).
- **Same bytes as the release.** Each platform package carries the binary from
  the matching GitHub Release tarball, and npm provenance covers it:
  `npm audit signatures` verifies both packages.

## Troubleshooting

**"the alint binary for <platform> is not installed"**: the install left out
optional dependencies, usually because of `npm install --omit=optional` (or
`--no-optional`), or a lockfile written on another platform without them.
Reinstall without omitting optional dependencies.

**`ALINT_BINARY`**: set it to the path of an alint binary and the `alint`
command runs that instead, on any platform.

`ALINT_SKIP_INSTALL`, used by versions before 0.22.1 to skip their postinstall
download, no longer does anything: there is no download to skip.

## License

Apache-2.0 OR MIT, matching the upstream alint project.

## Links

- Project: https://github.com/asamarts/alint
- Releases: https://github.com/asamarts/alint/releases
- Issues: https://github.com/asamarts/alint/issues
- Documentation: https://alint.org
