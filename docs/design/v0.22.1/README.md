# v0.22.1: npm per-platform packages, Windows on ARM, and dependency adoption

Status: planned (2026-11-08).
Decisions: [ADR-0031](../../adr/0031-v0221-owner-decisions.md) (release shape, npm packaging model,
how dependency gaps are filed). Background: [ADR-0015](../../adr/0015-distribution-strategy.md)
and [`distribution.md`](../distribution.md) section 3.5 (the npm packaging model),
[`yaml-parser.md`](../yaml-parser.md) (the YAML parser criteria).

v0.22.1 is a distribution and dependency release. It changes no rule semantics, no `.alint.yml`
behavior and no public Rust API, so `ci/scripts/check-patch-compat.sh v0.22.0` must pass.

## 1. Scope

In v0.22.1:

1. **npm per-platform packages.** `@asamarts/alint` stops downloading its binary in a postinstall
   script. Each platform's binary ships in its own npm package, installed through
   `optionalDependencies`, with no install script (ADR-0031 decision 2).
2. **A native Windows on ARM binary** (`aarch64-pc-windows-msvc`), which the npm and PyPI
   packages ship. Today Windows on ARM gets the x64 binary under emulation.
3. **adotenv 1.4.0**, up from 1.3.0 ([#480](https://github.com/asamarts/alint/pull/480)).

On its own track, merged for the first minor release after ayaml 0.1.2 meets the criteria
(ADR-0031 decision 1):

4. **The ayaml cutover.** ayaml replaces `yaml_serde` (and so `libyaml-rs`, a C2Rust translation
   with unsafe code). It changes the public types `RuleSpec.extra`, `config_redaction`,
   `template::render_*` and `alint-testkit`'s `TreeSpec` expose, so it cannot ship in a patch.

Out of scope: general-linter Phase 3 (v0.23), Scoop, WinGet, AUR and the other P2 channels of
`distribution.md`, and a private-registry install test (section 6.6, P2).

## 2. Dependency adoption criteria

alint has no single written dependency policy; the checks below are the ones its gates, design
docs and release process already apply. Every new or upgraded runtime dependency is evaluated
against all of them, and a parser-family change against section 2.2 as well.

### 2.1 Every runtime dependency

| # | Criterion | How it is checked |
|---|---|---|
| C1 | Provenance: published from a public tag by crates.io Trusted Publishing; a build attestation is preferred | crates.io version metadata (`trustpub_data`), `gh attestation verify` |
| C2 | MSRV at or below alint's (1.88), on the locked graph | `RUSTUP_TOOLCHAIN=1.88 cargo check --workspace --all-targets` |
| C3 | License on the allowlist; no advisory, ban or unknown source, with no waiver | `cargo deny check`, `cargo audit` |
| C4 | Unsafe code: none, or a reviewed reason | the crate's lints and unsafe census |
| C5 | Requirements that compose: caret ranges on shared dependencies such as `serde`, never exact pins | crates.io dependency metadata, `cargo tree -i` |
| C6 | Behavior: the workspace tests, the shell harnesses and the e2e suite pass; the corpus replay shows zero differences, or each difference is a documented, approved change | `cargo test --workspace`, `ci/scripts/shell-tests.sh`, the corpus-replay workflow |
| C7 | Performance: the deterministic micro gate within +3% of the previous release and +10% of the anchor; macro cells within the bench gate | `xtask micro-det`, `xtask bench-gate` |
| C8 | Resource invariants held: input caps, depth and alias budgets, extends limits | the guard tests and proptests |
| C9 | Semver: the change fits the release (a patch keeps earlier patches' crates compiling) | `ci/scripts/check-patch-compat.sh` |
| C10 | Docs: the CHANGELOG, and every doc that names the version | review |

### 2.2 A parser-family change (YAML)

The "Re-evaluation triggers" and "Security and resource invariants" of
[`yaml-parser.md`](../yaml-parser.md), restated as checks:

| # | Criterion |
|---|---|
| Y-C1 | Stable Serde, a mutable tagged `Value`/`Mapping`, and a `from_value` that rejects wrong-typed input as `yaml_serde` does |
| Y-C2 | Limits that express alint's contract: 128-level depth, 32 MiB input, the byte-weighted alias allowance, each failure with a location |
| Y-C3 | Differential compatibility on alint's committed corpus, including error locations, with every difference either fixed or documented and approved |
| Y-C4 | Representative performance within the micro gate |
| Y-C5 | A non-pre-1.0 compatibility policy, or ownership that justifies an exact pin, with attested releases |
| Y-C6 | Differential parser fuzzing committed before the cutover |
| Y-C7 | `alint-core::yaml_depth` re-derived from the new parser's tokens; its parity property passes with no relaxed assertion |

## 3. adotenv 1.4.0: evaluated, adopted

1.4.0 adds `Document::rename_with_references` and fixes adotenv's own LSP. In adotenv's core crate
the change only adds code (one rustdoc line changed), so alint's parser and edits behave as with
1.3.0.

| Criterion | Result |
|---|---|
| C1 | Met: Trusted Publishing from the `v1.4.0` tag commit `d3f0742` (run 42407499639). No build attestation: the one gap, filed as [asamarts/adotenv#71](https://github.com/asamarts/adotenv/issues/71) |
| C2 | Met: MSRV 1.81; the 1.88 check passes |
| C3 | Met: Apache-2.0 OR MIT; `cargo deny` passes all four checks |
| C4 | Met: `#![forbid(unsafe_code)]` |
| C5 | Met: no required runtime dependency (serde optional, not enabled) |
| C6 | Met: 4,224 workspace tests pass; the parser is unchanged, so the corpus replay is unaffected |
| C9, C10 | Met: a minor upgrade under a caret requirement; docs updated in #480 |

## 4. ayaml: evaluated against 0.1.1 and unreleased main, not yet adoptable

The alint-side cutover from the `ayaml-stage-b` branch was rebased onto main (7b64b1d18) and run
against ayaml 0.1.1 and against ayaml's main (24a7411). It is kept on a local branch,
`ayaml-qualify`, for the re-qualification against 0.1.2.

What passes: the build, clippy, MSRV 1.88, `cargo deny`, `cargo audit`, the e2e and scenario
suites, the corpus-replay, extends, policy-test and structured-query tests. `unsafe-libyaml`,
`libyaml-rs` and `yaml_serde` leave the graph. Of the 868 YAML files committed in this repository,
867 parse to the same value on 0.1.1 and all 868 on main. ayaml's default limits equal alint's
constants exactly.

What blocks the cutover:

| Criterion | Finding | Tracked by |
|---|---|---|
| Y-C1 | `from_value` accepts a number, boolean or null for a string field, so `pattern: null` loads as the regex `/null/` (4 failing tests) | ayaml AY-021 |
| Y-C3 | Valid YAML is rejected, such as `key:\t{a: 1}`, a comment containing `: ` after a plain continuation, `-\n\|2` and `? \n\|`; some invalid YAML is accepted | AY-024, AY-025, and gaps routed to ayaml |
| Y-C3 | Of 172,595 fuzz inputs that both parsers reject, 73,768 report a different location; depth and multi-document errors have no location | gaps routed to ayaml |
| Y-C3 | A core tag on a collection (`!!str {}`) is kept as `Tagged`, which libyaml drops | gap routed to ayaml |
| Y-C2 | The compatibility entry points accept no `Limits` | AY-048 (ayaml 0.2) |
| Y-C4 | `dsl_extends/chain_depth/*` +7% to +9% instructions on 0.1.1 (+5% to +7% on main); alias-heavy input allocates about 10 times as much | gap routed to ayaml |
| Y-C5, C5 | Pre-1.0 releases without attestation; exact `=` pins on `serde` and `indexmap` | AY-004, AY-029 |
| Y-C6 | No differential parser fuzzing on either side | [#482](https://github.com/asamarts/alint/issues/482), gap routed to ayaml |
| Y-C7 | The scanner-parity property had to drop its flow half; the oracle mirrors libyaml, not ayaml | [#483](https://github.com/asamarts/alint/issues/483) |

Gaps that no ayaml task covered went to the ayaml maintainers for triage into ayaml's own tracker
(ADR-0031 decision 3). ayaml 0.1.2 is expected to change, on purpose: typed errors gain a path
prefix, `*a:` becomes a mapping key in compatibility mode, flow-collection error wording changes,
and limits count events, depth and scalar size the same way on every entry point. The
re-qualification records these as intended changes.

## 5. npm per-platform packages: design

### 5.1 Packages

| Package | `os` | `cpu` | Release target |
|---|---|---|---|
| `@asamarts/alint` | linux, darwin, win32 | x64, arm64 | none: the `alint` command, a shim |
| `@asamarts/alint-linux-x64` | linux | x64 | `x86_64-unknown-linux-musl` |
| `@asamarts/alint-linux-arm64` | linux | arm64 | `aarch64-unknown-linux-musl` |
| `@asamarts/alint-darwin-x64` | darwin | x64 | `x86_64-apple-darwin` |
| `@asamarts/alint-darwin-arm64` | darwin | arm64 | `aarch64-apple-darwin` |
| `@asamarts/alint-win32-x64` | win32 | x64 | `x86_64-pc-windows-msvc` |
| `@asamarts/alint-win32-arm64` | win32 | arm64 | `aarch64-pc-windows-msvc` (new) |

- The Linux binaries are static musl builds, so one package per architecture serves glibc and musl
  systems alike, and no `libc` field is needed.
- `@asamarts/alint` lists the six as `optionalDependencies` at the exact release version. npm, pnpm,
  Yarn and Bun install only the one whose `os` and `cpu` match.
- No package has an install script.

### 5.2 The shim

`bin/alint.js` resolves `@asamarts/alint-<platform>-<arch>/bin/alint[.exe]` with
`require.resolve` and runs it with inherited stdio, forwarding the exit code and signal as today.

- `ALINT_BINARY=<path>` runs a given binary instead, for mirrors and air-gapped installs.
- When the platform package is missing, the shim exits 1 with one message: the platform it looked
  for, the likely cause (`--omit=optional`, `--no-optional`, or a lockfile written on another
  platform), and the fix.
- On a platform outside the table, install fails with npm's own `EBADPLATFORM`, as today.

### 5.3 Building the packages

A script stages the seven packages from the release tarballs, the same byte-identical, attested
bytes the GitHub Release and the PyPI wheels carry. It checks each tarball against `SHA256SUMS`,
copies the binary and the license files, and writes each `package.json` from one table, the single
source the shim, the tests and the pin gate also read.

### 5.4 Publishing

The `publish-npm` job:

1. Stages the packages.
2. Publishes the six platform packages, then `@asamarts/alint` last, so the shim never names a
   version that is not on the registry.
3. Uses Trusted Publishing (OIDC) with provenance, which now attests the binary bytes too.
4. Is rerun-safe per package: a version already on the registry is skipped.
5. Applies the existing dist-tag rollback guard to every package: only the highest release moves
   `latest`.

### 5.5 One-time bootstrap (owner action)

npm can configure a trusted publisher only for a package that already exists (npmjs.com, or
`npm trust`, npm 11.15.0 or later, with 2FA). Before the v0.22.1 tag, the owner:

1. Publishes a placeholder `0.0.0` of each of the six new packages.
2. Adds the trusted publisher to each: repository `asamarts/alint`, workflow `release.yml`.

A script does both and prompts for the 2FA code. npm accepts a new package's first publish with
HTTP 202 and processes it for a few minutes; until then `npm deprecate` and `npm trust` answer 404,
so the script waits until the registry serves each placeholder, and a re-run finishes any package
an earlier run published but did not deprecate or trust. A package without its trusted publisher fails
`publish-npm` at publish time. The recovery is the same as for a new crate on crates.io: configure
it, then `gh run rerun --failed`, never re-tag.

### 5.6 Tests

- **Unit tests** (`ci/scripts/test-npm-shim.sh`, rewritten):
  - the package table, the release matrix and the shim agree in both directions;
  - the shim resolves each platform;
  - it reports a missing package;
  - it forwards exit codes and signals;
  - `ALINT_BINARY` works.
- **Pack-and-install test** on each pull request that touches packaging, on Linux, macOS and
  Windows. It stages the packages from a locally built binary, then `npm pack`s and installs them
  into an empty project with:
  - `npm install`;
  - `npm install --ignore-scripts`;
  - `pnpm`;
  - `bun` (`bunx alint --version`).

  `npm install --omit=optional` must print the shim's message.
- **Post-publish smoke:** the npm channel on Ubuntu, macOS, Windows and Windows on ARM, plus
  `bunx` and `npx`, each running `alint --version`, and `npm audit signatures` checking provenance.

## 6. Tasks

Priority: **P0** blocks the v0.22.1 tag; **P1** should ship in v0.22.1; **P2** follows it; **V23**
belongs to the v0.23 track.

### 6.1 Windows on ARM

| ID | Task | Pri | Depends on | Done when |
|---|---|---|---|---|
| W1 | Add `aarch64-pc-windows-msvc` to the release build matrix on the `windows-11-arm` runner; package it like the x64 build | P0 | | a dry-run tag build produces the tarball and its `.sha256` |
| W2 | Build and test it in `cross-platform.yml`, so it cannot rot between releases | P0 | W1 | the job is green on a PR |
| W3 | Confirm `SHA256SUMS`, cosign, the build attestation, the SBOM and the license bundle cover the new asset | P0 | W1 | `gh attestation verify` and `cosign verify-blob` pass on it |
| W4 | Confirm `cargo binstall alint` resolves it (asset name template) | P1 | W1 | a dry run on Windows on ARM picks the native asset |
| W5 | Ship a `win_arm64` PyPI wheel (`build_wheels.py` platform table and tests) | P1 | W1 | `test-build-wheels` passes and the post-publish smoke installs it |

### 6.2 npm packages

| ID | Task | Pri | Depends on | Done when |
|---|---|---|---|---|
| N1 | One package table (platform, arch, target, package name) that the stager, the shim, the tests and the pin gate read | P0 | | the table is committed and the unit tests read it |
| N2 | The package stager: release tarballs in, seven package directories out, checked against `SHA256SUMS` | P0 | N1 | it stages from a local build and from a downloaded release |
| N3 | The new shim with `require.resolve`, `ALINT_BINARY` and the missing-package message; drop `install.js`, the `tar` dependency and the postinstall script | P0 | N1 | the unit tests pass on Linux, macOS and Windows |
| N4 | Rewrite `test-npm-shim.sh`, and add the pack-and-install test (npm, `--ignore-scripts`, `--omit=optional`, pnpm, Bun) on three operating systems | P0 | N2, N3 | green on a PR |
| N5 | `publish-npm`: stage, publish the platform packages then the shim, rerun-safe per package, dist-tag guard per package, provenance | P0 | N2, W1 | a tag rehearsal on a fork, or `npm publish --dry-run` of all seven in CI |
| N6 | Extend the version-pin gate and `bump-version.sh` to the shim's `optionalDependencies` and the package table | P0 | N1 | the pin gate fails on a planted mismatch |
| N7 | Bootstrap script, then the owner publishes the six placeholders and configures their trusted publishers (section 5.5) | P0 | N1 | `npm trust list` shows the publisher for all seven packages |
| N8 | Post-publish smoke: npm on four operating systems, `bunx`, `npx`, `npm audit signatures` | P1 | N5 | it passes against the v0.22.1 publish |
| N9 | Docs: `npm/README.md`, the installation page, the README, RELEASING (bootstrap, recovery, the per-package 404), `distribution.md` status (§3.5 and MP-M3 resolved), the CHANGELOG (including that `ALINT_SKIP_INSTALL` no longer does anything) | P0 | N3 | reviewed; the docs gates pass |
| N10 | A private-registry install test (Verdaccio) with no github.com access | P2 | N5 | the install works with the network to github.com blocked |

### 6.3 Dependencies

| ID | Task | Pri | Depends on | Done when |
|---|---|---|---|---|
| D1 | adotenv 1.4.0 ([#480](https://github.com/asamarts/alint/pull/480)) | P0 | | merged |
| D2 | adotenv: attest published crates ([asamarts/adotenv#71](https://github.com/asamarts/adotenv/issues/71)) | P2 | | adotenv's next release verifies with `gh attestation verify` |

### 6.4 Release

| ID | Task | Pri | Depends on | Done when |
|---|---|---|---|---|
| R1 | Release PR: `bump-version.sh 0.22.1`, CHANGELOG summary, ROADMAP latest-release line, `check-patch-compat.sh v0.22.0`, shell tests in tag context | P0 | all P0 | merged |
| R2 | Tag, then the post-tag checklist: release.yml, the Action pin, the alint.org sync | P0 | R1, N7 | every channel publishes and the smoke passes |
| R3 | Bench record: the engine is unchanged apart from the adotenv lock, so the deterministic micro gate is run first; the full record only if it moves | P1 | R2 | recorded or skipped with the reason |

### 6.5 The ayaml cutover (v0.23 track)

| ID | Task | Pri | Depends on | Done when |
|---|---|---|---|---|
| Y1 | Tracking: cut over once ayaml 0.1.2 meets section 2.2 ([#481](https://github.com/asamarts/alint/issues/481)) | V23 | Y2-Y5, ayaml 0.1.2 | merged for v0.23.0 |
| Y2 | Differential parser fuzzing harness: `yaml_serde` against ayaml, token soup with and without NEL, LS and BOM, plus the corpus ([#482](https://github.com/asamarts/alint/issues/482)) | V23 | | committed and run in CI before the cutover |
| Y3 | Re-derive the `yaml_depth` oracle from ayaml; remove the 0.1.1 shims (`map_compat_error`, `bom_alias_bomb`, the `requires.alint` prefix); pin `Limits::default()` to alint's constants ([#483](https://github.com/asamarts/alint/issues/483)) | V23 | ayaml 0.1.2 | the parity property passes with no relaxed assertion |
| Y4 | Keep parser types out of alint's public API, so a later parser change is not a breaking change ([#484](https://github.com/asamarts/alint/issues/484)) | V23 | | decided in the v0.23 design; ties to the pre-1.0 sealing item |
| Y5 | Re-qualify on 0.1.2: the full matrix, the corpus replay, micro-det, and the intended-change list in the CHANGELOG and `yaml-parser.md` | V23 | ayaml 0.1.2, Y2, Y3 | every section 2.2 criterion met |

## 7. Risks

- **A missing trusted publisher fails the release halfway.** The platform packages publish first,
  so a failure leaves the shim unpublished, which is safe. The bootstrap check (N7) runs before
  the tag.
- **Lockfiles written on one platform.** npm v7+ records every optional platform package in the
  lockfile and installs the matching one later. Old Yarn v1 lockfiles may not; the shim's message
  covers it.
- **The Windows on ARM runner.** If `windows-11-arm` is unavailable or flaky, W1 can cross-compile
  on `windows-latest`, but then nothing tests the binary natively before release.
- **Package size.** Each platform package carries one binary, so an install downloads one binary,
  not six.

## 8. Status log

- 2026-11-08: planned. adotenv 1.4.0 evaluated and adopted (#480). ayaml 0.1.1 and main evaluated:
  not yet adoptable (section 4); its gaps went to ayaml's maintainers, and alint-side work is in
  #481-#484. Owner decisions in ADR-0031.

- 2026-11-08: phases W and N implemented in #486. Done there: W2 (cross-platform.yml builds and
  tests on `windows-11-arm`, with `ensure-rustup.sh`), W3 (the release job collects every
  `*.tar.gz.sha256` into `SHA256SUMS`, so cosign, the build attestation and the SBOM glob cover the
  new tarball with no change), W4 (no binstall metadata: binstall's default
  `{name}-v{version}-{target}` template already names the asset), W5, N1-N4, N6, N8 (the smoke
  code), N9, and N10 (`test-npm-install.sh` installs from a Verdaccio with no uplinks, and nothing
  in the packages fetches). The editor extensions' download maps gained `win32/arm64` too.
  Found on the way: Yarn 4.18 quarantines versions younger than a day by default
  (`npmMinimalAgeGate`), so Yarn users get a new release a day late; pnpm 12 ignores
  `--store-dir` (the test sets `storeDir` in `pnpm-workspace.yaml`). Verifiable only at the tag:
  W1's release build and N5's publish; the release preflight now runs the install test before
  anything publishes.

- 2026-11-08: the owner's first bootstrap run published `@asamarts/alint-linux-x64@0.0.0`, then
  `npm deprecate` failed with 404: npm had accepted the new package (HTTP 202) but was still
  processing it. The script now waits until the registry serves each placeholder, and deprecates
  any placeholder that is not deprecated yet, so a re-run resumes.

- 2026-11-08: the owner's re-run finished `@asamarts/alint-linux-x64` and published
  `@asamarts/alint-linux-arm64`, then stopped at its deprecate, most likely a 2FA prompt that timed
  out during the wait. A second gap: npm asks for 2FA only on a terminal, and `npm trust list`
  needs 2FA, so the script's piped `npm trust list` failed with EOTP and every re-run would have
  tried to add a second Trusted Publisher, which the registry refuses (one per package). The script
  now reads `npm trust list` on a pseudo-terminal (as plain text: under `--json` npm hides its 2FA
  URL), and runs each step for all packages before the
  next, so the 2FA prompts come together and the wait happens once.

Next: the owner's bootstrap (N7: `ci/scripts/npm-bootstrap-platform-packages.sh`), then the release
(R1-R3).
