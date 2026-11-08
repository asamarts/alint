---
status: accepted
date: 2026-11-08
decision-makers: asamarts
---

# 0031. v0.22.1 owner decisions: release shape, npm packaging, and filing dependency gaps

## Status

Accepted (2026-11-08). Settles the shape of the release after v0.22.0, how the npm package carries
its binary, and where gaps found in alint's own dependencies are filed. ADR-0030 decision 4 already
placed the npm move in its own release after v0.22.0; ADR-0015 chose the optionalDependencies model.
The plan is [`docs/design/v0.22.1/README.md`](../design/v0.22.1/README.md).

## Context

- **ayaml is not ready.** The owner asked for the latest adotenv and ayaml in the next release. adotenv
  1.4.0 meets every dependency criterion. ayaml 0.1.1 and its unreleased main do not: `from_value`
  accepts wrong-typed config values, valid YAML is rejected, error locations differ, the crate pins
  `serde` and `indexmap` exactly, and releases are pre-1.0 and unattested. ayaml 0.1.2, which addresses
  most of this, has no date.
- **The YAML parser leaks into the public API.** `RuleSpec.extra`, `config_redaction`,
  `template::render_*` and `alint-testkit`'s `TreeSpec` expose `yaml_serde` types. Swapping the parser
  changes them, which a patch release may not do (`check-patch-compat.sh`).
- **npm's postinstall download breaks.** `@asamarts/alint` downloads its binary in a postinstall
  script, which fails under Bun (it blocks install scripts), `--ignore-scripts`, offline and
  private-registry installs, and a deleted release. There is no native Windows on ARM binary.
- **ayaml's tracker.** ayaml tracks work as tasks in its own repository. Its public text names no
  consumer, and its maintainers asked that gaps reach them for triage rather than as issues filed
  directly.

## Decision

1. **Ship npm now, ayaml in the next minor.** v0.22.1 ships the npm per-platform packages, the
   Windows on ARM binary and adotenv 1.4.0. The ayaml cutover is built and qualified on a branch, and
   merges for the first minor release after ayaml 0.1.2 meets the criteria of the plan's section 2.2
   (v0.23.0 at the earliest, alongside general-linter Phase 3). npm users do not wait on ayaml.
2. **Pure optionalDependencies, no install script.** Each platform's binary ships in
   `@asamarts/alint-<platform>-<arch>` with `os` and `cpu` fields; `@asamarts/alint` lists them as
   exact-version `optionalDependencies` and runs the installed one. No package has an install script
   and there is no network fallback: with optional dependencies omitted, the `alint` command prints how
   to reinstall. `ALINT_BINARY` points the shim at a binary of the user's choosing. A native
   `aarch64-pc-windows-msvc` build joins the release matrix and ships as `@asamarts/alint-win32-arm64`.
3. **Dependency gaps go to each project's own tracker.** Gaps in ayaml go to its maintainers, who check
   them against ayaml's tasks and add generic public tasks through pull requests the owner approves.
   alint-side work is filed as alint issues (#481-#484). adotenv gaps are filed as adotenv issues
   (asamarts/adotenv#71).

## Consequences

- **v0.22.1 changes no rule semantics or public Rust API**, so it stays a patch.
- **The YAML cutover becomes a v0.23 track** with its own gates: differential fuzzing (#482), the
  re-derived depth-guard oracle (#483), and a decision on parser types in the public API (#484).
- **One-time npm bootstrap.** npm can only configure a trusted publisher for a package that exists, so
  the owner publishes a placeholder of each of the six new packages and configures its trusted
  publisher before the v0.22.1 tag. A package without one fails `publish-npm`; the recovery is to
  configure it and rerun the failed jobs, never to re-tag.
- **Provenance and download size.** npm provenance now covers the binary bytes. An install downloads
  one platform's binary.
- **`--omit=optional` no longer gets a working `alint`.** Users who install with optional
  dependencies omitted get a clear error instead of the current download. That is the price of having
  no install script.

## Considered Options

- **One release that waits for ayaml (v0.23.0 with npm, adotenv and ayaml).** Rejected: npm's
  failures are live today, and ayaml 0.1.2 has no date.
- **A dedicated ayaml minor ahead of Phase 3.** Not chosen: the cutover can share v0.23.0 with Phase 3,
  since its gates are independent.
- **A postinstall fallback for `--omit=optional` (esbuild's model).** Rejected: it keeps an install
  script and the release-download path, and the fallback binary has no npm provenance.
- **Filing ayaml gaps directly as ayaml GitHub issues.** Rejected at the ayaml maintainers' request.
  Their public text names no consumer, and duplicate tasks have leaked before.
