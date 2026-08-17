# Security policy

alint is a build-time / CI-time tool that runs against repository contents.
Vulnerabilities can affect supply-chain integrity for everyone who uses it,
so reports are taken seriously and handled privately until a fix ships.

## Supported versions

alint is pre-1.0 and ships from a single line of development. Only the **latest
released minor** is supported: fixes land in a new release, not as backports to older
minors. Published versions are immutable (crates.io and npm are never deleted) and are
**yanked or deprecated only on a confirmed defect**, never proactively removed, so
pinned builds keep working. There is no separate LTS line.

## Reporting a vulnerability

**Do not file a public GitHub issue for security vulnerabilities.**

Email **security@alint.org** with:

- A description of the vulnerability and its impact
- Steps to reproduce (config snippet + tree shape, or a minimal failing repo)
- The alint version affected (`alint --version`)
- Your suggested severity (low / medium / high / critical), if you have one
- Whether you want public credit when the advisory is published (default: yes,
  with your name + optional affiliation)

You can also report privately via [GitHub's private vulnerability reporting](
https://github.com/asamarts/alint/security/advisories/new) if you prefer that
channel.

PGP-encrypted reports are accepted. Key fingerprint published on request to
the same email address.

## Response timeline

- **Acknowledgement**: within 72 hours of receipt
- **Initial assessment** (severity confirmation, scope): within 7 days
- **Fix or mitigation**: critical issues within 14 days, high within 30 days,
  medium/low within 90 days
- **Public disclosure**: 90 days from initial report, or earlier if a fix
  ships and we agree on a coordinated disclosure date

If you don't hear back within 72 hours, please re-send; email is best-effort
and edge cases happen.

## Scope

In scope:

- The `alint` CLI binary and all crates published from this repo
  (`alint`, `alint-core`, `alint-rules`, `alint-dsl`, `alint-output`,
  `alint-lsp`)
- The bundled rulesets compiled into the binary
- The official GitHub Action (`asamarts/alint`)
- The Docker image (`ghcr.io/asamarts/alint`)
- The Homebrew formula (`asamarts/homebrew-alint`)
- The npm package (`@asamarts/alint`)
- The `xtask` build/release tooling

Out of scope (report directly to upstream):

- Vulnerabilities in transitive dependencies (run `cargo audit` against this
  repo's lockfile to identify; we accept PRs bumping deps to patched versions)
- The alint.org marketing site (separate repo; report there)
- Third-party plugins or rules not maintained by `@asamarts`

## Disclosure of past vulnerabilities

Published advisories live at
https://github.com/asamarts/alint/security/advisories.

No advisories published as of the v0.15.0 release.

## Threat model

alint is designed to be safe to run against untrusted repositories. It walks
the filesystem, reads file contents, optionally calls `git` and `sh`-via-the-
`command` rule kind. The defensive posture is:

- **No network access by default.** The only network-touching feature is
  `extends: https://...` URLs the user explicitly puts in their config; those
  are SRI-pinned, so a swap on the upstream URL fails verification.
- **Path-traversal hardened.** All paths are validated to stay within the
  repo root. Symlinks are honoured per the `no_symlinks` rule's verdict.
- **Plugin trust gating.** The `command` rule kind shells out to external
  CLIs; it's gated to the user's top-level config (extends'd configs cannot
  introduce a `command` rule).
- **No telemetry.** alint sends nothing over the network except the
  user-explicit `extends:` URLs above.
- **Reproducible builds.** `Cargo.lock` is committed. CI uses
  `dtolnay/rust-toolchain@stable` with a pinned `rust-toolchain.toml`.

If you find a way to exfiltrate data, escalate privileges, or compromise the
host running alint via a malicious config or repo content, that's in scope and
worth a report.
