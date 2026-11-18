# Security policy

alint is a build-time / CI-time tool that runs against repository contents.
Vulnerabilities can affect supply-chain integrity for everyone who uses it,
so reports are taken seriously and handled privately until a fix ships.

## Supported versions

alint is pre-1.0 and ships from a single line of development. Only the **latest
released minor** is supported: fixes land in a new release, not as backports to older
minors. Published versions are **removed only on a confirmed defect, never
proactively**: crates.io versions can only be yanked (never deleted), and we do not
unpublish npm releases, so pinned builds keep working. There is no separate LTS line.

## Verifying release artifacts

Releases are signed and carry build provenance, so you can confirm a download was
built by this repo's CI from this repo's source before you run it. This defends
against tampered mirrors and tampered downloads. It does not by itself defend a
compromised publishing account (that account is the root of trust), so the org is
hardened with 2FA and minimal token scopes alongside these steps. Signing and
attestation are keyless (Sigstore / GitHub OIDC): there is no long-lived signing
key to leak, and every signature is logged in the public Rekor transparency log.

The `gh` commands need an authenticated GitHub CLI (`gh auth login`, or a
`GH_TOKEN`); the `cosign` commands need cosign v3+ (the signatures use the
new-format Sigstore bundle) but no GitHub account.

**Build provenance** (GitHub CLI). Each tarball, the installer, the SBOM, the
license bundle, and the container image are attested:

```
gh attestation verify alint-<version>-<target>.tar.gz --repo asamarts/alint
gh attestation verify oci://ghcr.io/asamarts/alint:<version> --repo asamarts/alint
```

**Signature over the checksum manifest**, no GitHub CLI needed. `SHA256SUMS` is
cosign-signed; the signature travels as `SHA256SUMS.cosign.bundle`. Verify the
manifest, then check any downloaded asset against it:

```
cosign verify-blob --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github\.com/asamarts/alint/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS   # macOS: shasum -a 256 --check --ignore-missing SHA256SUMS
```

**Container image signature.** cosign resolves the tag to the digest it signed:

```
cosign verify ghcr.io/asamarts/alint:<version> \
  --certificate-identity-regexp '^https://github\.com/asamarts/alint/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

**PyPI wheels** carry PEP 740 attestations, generated at publish time via the same
keyless Sigstore / GitHub OIDC identity. Each wheel's provenance is shown on its file
page at pypi.org/project/alint, and because the wheel repackages the exact attested
release binary, the build provenance above covers its payload too.

Provenance and signatures are attached to every release from the version that
introduced them onward; a release that predates it has none to verify.

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
  `alint-lsp`, `alint-testkit`)
- The bundled rulesets compiled into the binary
- The official GitHub Action (`asamarts/alint`)
- The Docker image (`ghcr.io/asamarts/alint`)
- The Homebrew formula (`asamarts/homebrew-alint`)
- The npm package (`@asamarts/alint`)
- The `install.sh` release installer (served from `alint.org/install.sh`)
- The `xtask` build/release tooling

Out of scope (report directly to upstream):

- Vulnerabilities in transitive dependencies (run `cargo audit` against this
  repo's lockfile to identify; we accept PRs bumping deps to patched versions)
- The alint.org marketing site (separate repo; report there)
- Third-party plugins or rules not maintained by `@asamarts`

## Disclosure of past vulnerabilities

Published advisories live at
https://github.com/asamarts/alint/security/advisories. Each one is listed here
too, with the release that fixes it:

- **GHSA-78jj-xfpm-g6px** (a draft until the 0.23.0 release, which fixes it and
  publishes it): ways around the policy-test sandbox and the trust given to
  `extends:` rulesets, nested configs and editor workspaces, in `alint test`,
  `alint fix`, `alint check` and `alint lsp`. The 0.23.0 entry of the
  [CHANGELOG](CHANGELOG.md) lists each one under Security.

## Threat model

alint walks the filesystem, reads file contents, and optionally calls `git` and the commands a
config names. A repository's own top-level `.alint.yml` is trusted the way its `Makefile` is:
`alint check` in a clone runs that config's command rules (`kind: command` and the other spawning
kinds), spawning fixes and `custom:` facts, fetches its remote `extends:`, and reads the
environment values it names. To lint a repository you do not trust, use your own config
(`alint check -c your-config.yml`) or `--untrusted`, the language server's untrusted mode on the
command line: it refuses every rule, fix and fact that starts a process or runs Git (and
`--changed` and `--base`), refuses a remote `extends:` before anything is fetched, and gives the
config an empty environment. `--no-spawn` refuses the processes alone: the config still fetches
the remote `extends:` it names and reads the environment values it names, and an `extends:` URL
built from those values carries them to the host it names. The language server (`alint lsp`)
runs none of these until the editor says the workspace is trusted. Everything else (a nested or
extended config, and the repository's file contents) is treated as untrusted input. The defensive
posture is:

- **No network access by default.** The only network-touching feature is
  `extends: https://...` URLs the user explicitly puts in their config; those
  are SRI-pinned, so a swap on the upstream URL fails verification.
- **Path-traversal hardened.** All paths are validated to stay within the
  repo root. Symlinks are honoured per the `no_symlinks` rule's verdict. A
  fix never writes or reads a Git directory (`.git` in the spellings Git's
  own checks name, such as `.GIT`, `.git.` and `git~1`, or through a symlink
  to one): a file there would be configuration Git runs commands from. A
  Windows short name Git's checks do not name (`GIT~2`, or a hashed one)
  passes. The walk leaves out a symlink whose
  target is in a Git directory, so no content fix edits `.git/config` under
  another name. No rule reads a Git directory either, from any configuration:
  `.git/config` can hold credentials (the `http.<url>.extraheader` that
  `actions/checkout` persists). A path a rule names there is a load error, and
  one a symlink leads into is refused when it is read. The baseline file a
  config names (`baseline:`) is read only inside the repo root, outside a Git
  directory; the `--baseline` flag names one anywhere.
- **Files read whole are bounded.** The walk reads an ignore file
  (`.gitignore`, `.ignore`, `info/exclude` and a linked worktree's `.git` file
  and `commondir` on the way to it, the global excludes file) only when it is
  a regular file of at most 32 MiB, and stops with an error naming one it will
  not read: a symlink to `/dev/zero` or a FIFO is never read. A baseline file
  is read only when it is a regular file of at most 256 MiB.
- **Plugin trust gating.** The `command` rule kind shells out to external
  CLIs; it's gated to the user's top-level config (extends'd configs cannot
  introduce a `command` rule). A fix of a rule an `extends:`'d ruleset or a
  nested config shaped never writes alint's own configuration (`.alint.yml`
  and the other discovery names, `.alint.d/`, the policy tests under
  `alint-tests/`, the Windows short name of any of them such as `ALINT~1.YML`,
  or a file the configuration was loaded from), so it cannot add such a rule
  for the next run, and never
  writes or reads (`content_from`, `sync_from`) outside the repo root,
  whatever `allow_out_of_root` says. Such a rule's check reads nothing
  outside the root either: `allow_out_of_root` grants the user's own
  top-level rules only, even when the user's config overrides the rule by id
  or names it. The language server's code actions keep the same limits.
- **A local `extends:` file is part of your repository.** Unlike an
  `https://` ruleset not listed in `trusted_extends`, a local one may read
  `env.*` in its conditions and declare `ignore:`. Like every extended
  config, it never spawns a process, never has a fix write alint's
  configuration or outside the root, and never reads a Git directory. It
  names a file relative to the config that extends it, in the repository: an
  absolute, rooted or network path (`/x`, `C:\x`, `\\server\share\x`) is
  refused before alint touches it, unless the top-level config sets
  `allow_out_of_root: true`. Vendor a ruleset only as you would vendor code,
  after reading it.
- **The configuration is the repository's.** Configuration discovery stops
  at the repository's top (the nearest directory holding a `.git` directory
  or file), so a `.alint.yml` above it, in a directory someone else may write
  (`/tmp`, a shared home), never governs it. A `.git` another user owns, or
  one in a directory another user owns, makes no top (an empty `/tmp/.git`
  would otherwise end discovery there). Outside every repository,
  discovery skips a config file another user owns in a directory above the
  one alint runs in, and warns (`skipping /tmp/.alint.yml: owned by another
  user`, with how to use it if you trust it); a config in that directory
  itself is used whoever owns it (root in a container, on a directory the
  host's user owns). On Windows, where alint
  reads no owner, nothing is skipped.
- **Git reads the repository alint names.** alint finds the repository
  itself (the nearest directory holding a `.git` directory or file) and
  names it to every Git command (`--git-dir`, `--work-tree`), so Git never
  discovers one: a directory shaped like a bare repository, whose `config`
  could name a command (`core.fsmonitor`), is never taken for one, whatever
  the Git version (`safe.bareRepository=explicit`, which alint also sets,
  exists from Git 2.38). A repository the current user does not own is
  named only when Git, asked to discover exactly it, accepts it (its
  ownership check and `safe.directory`). With no repository, alint runs no
  Git command. Running alint in a repository whose `.git/config` someone
  else wrote (an unpacked archive, a copied directory) runs whatever that
  config tells Git to run, exactly as `git status` would there: Git's own
  threat model applies. With `--no-spawn` or `--untrusted`, and in a language
  server workspace that is not trusted, alint runs no Git at all: a rule that
  runs Git (the `git_*` kinds, `git_tracked_only`,
  `scope_filter.changed_since`) is refused (skipped by the server), and so are
  `--changed` and `--base`.
- **The language server waits for workspace trust.** Until the editor sends
  the initialization option `{"trusted": true}` (VS Code sends its Workspace
  Trust state), `alint lsp` skips spawning rules, rules that run Git and
  `custom:` facts (a rule whose fix alone spawns lints without the fix),
  fetches no remote `extends:` and reads no environment value; other rules
  lint as usual. `alint lsp --untrusted` takes every workspace as untrusted,
  whatever the editor says. A
  nested or vendored `.alint.yml` never governs a file as a top-level config:
  the workspace folder's config does, as for `alint check` from the root, or,
  in a folder without one, the config at the top of the file's repository
  (the nearest directory holding `.git`), and each adds nested configs only
  with `nested_configs: true`.
- **Policy tests start without host authority.** `alint test` runs a rule,
  fixer or custom fact that spawns a process, at any `require:` depth, or
  reads the host keyring, only when the test declares the capability and the
  command line grants it (`--allow-spawn`, `--allow-host-keyring`). A test's
  tree, fixture and fixes stay in its sandbox: every symlink in its
  repository leads inside it. The Git commands the runner runs itself get
  none of `given.env` and read only the repository `given.git` makes, which
  each names: Git takes no fixture directory for a repository, a fix cannot
  write one, and without `given.git` no Git runs. A test's conditions read
  `given.env` and the runner's sandbox values, not the host's environment.
- **No telemetry.** alint sends nothing over the network except the
  user-explicit `extends:` URLs above.
- **Reproducible builds.** `Cargo.lock` is committed. CI installs toolchains
  with `dtolnay/rust-toolchain` pinned to a commit on its `master` branch, and
  release binaries build with the exact toolchain in `RELEASE_RUST`.

If you find a way to exfiltrate data, escalate privileges, or compromise the
host running alint via a malicious config or repo content, that's in scope and
worth a report.
