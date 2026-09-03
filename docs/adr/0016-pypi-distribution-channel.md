---
status: accepted
date: 2026-09-03
decision-makers: asamarts
---

# 0016. PyPI distribution channel (wheels for uvx / pipx / pre-commit)

## Status

Accepted (2026-09-03). Companion design doc:
[`docs/design/distribution-pypi.md`](../design/distribution-pypi.md), which carries the
mechanism comparison, the wheel/platform mapping, the publish and pre-commit design, and
the phased build plan. Extends [ADR-0015](0015-distribution-strategy.md), which listed
PyPI as a Tier-2, demand-gated channel; this ADR promotes it to a build and fixes the
mechanism.

## Context

ADR-0015 tiered PyPI as "adopt later" because alint is language-agnostic, so PyPI is
reach-into-an-ecosystem rather than a natural home. Two forces moved it off the shelf:

- **The pre-commit hook compiles from source (`MP-L5`).** The `language: rust` hook needs
  a full Rust toolchain and is the slowest install path. The pre-commit audience is
  exactly who asks for a PyPI channel; dprint ships `dprint-py` purely for this niche.
- **The P1 supply-chain arc changed the mechanism's cost/benefit.** ADR-0015 assumed PyPI
  via maturin (recompile in CI, "how ruff and typos ship"). After making the release
  binaries single-source and SLSA-attested, a maturin wheel would introduce a third build
  variant that a `pip` user gets without the release's own provenance.

The Python packaging model is also strictly better than the current npm channel: a wheel
embeds the binary, so it avoids npm's four postinstall-download failure modes
(`--ignore-scripts`, `bunx`, offline installs, a deleted release).

## Decision

We will build a PyPI distribution channel this cycle, delivering `uvx alint`,
`pipx run alint`, `uv tool install alint`, and `pip install alint`, plus a fast
`language: python` pre-commit hook. Specifically:

- **Path B (repackage the attested release binaries), not Path A (maturin recompile).**
  For each release target, repackage the exact release tarball bytes into a
  Python-version-agnostic `py3-none-<platform>` wheel that drops the binary on PATH via
  the wheel `.data/scripts/` category. The wheel payload is byte-identical to the release
  binary, so the release's build-provenance transitively covers it. Path A remains the
  documented fallback if the custom assembler becomes a burden.
- **Publish tokenlessly** via OIDC Trusted Publishing with PEP 740 attestations
  (`pypa/gh-action-pypi-publish` under `id-token: write` only), mirroring the crates.io and
  npm posture. No `PYPI_API_TOKEN`.
- **A separate `asamarts/alint-pre-commit` mirror repo** carries the `language: python`
  hook (ruff's `ruff-pre-commit` model), decoupling the pre-commit `rev` from the release
  tag and keeping the alint repo clean.
- **Claim the PyPI name `alint`** (available; the installed command stays `alint`).

## Consequences

Easier: the Python-ecosystem install paths and a fast, cross-language pre-commit hook that
retires `MP-L5`; a packaging model with no postinstall-download failure modes; one
verifiable provenance chain across GitHub and PyPI.

Harder: a bespoke wheel assembler to maintain (mitigated by a stdlib-only gate that
asserts platform tags, the executable bit, byte-identical payload, RECORD integrity, and
reproducibility); a second public repo to auto-sync each release; a new release-pipeline
job and a macOS deployment-target pin (`MP-L7`) whose value is a stable wheel tag.

## Considered Options

- **Path A (maturin `bindings="bin"`):** the industry default (ruff, uv, typos). Rejected
  as the primary mechanism because it recompiles the binary in CI, producing a third build
  variant without the release's own attestation, against the grain of the P1 investment.
  Kept as the documented fallback.
- **Path B (repackage the attested binary):** chosen. Byte-identical, single provenance
  chain, reuses the existing build matrix (dprint-py / zig-pypi precedent).
- **Stay demand-gated (ADR-0015 status quo):** rejected; the pre-commit win is concrete
  and the mechanism question is now settled.
- **No mirror repo (docker-image hook, or a root `pyproject.toml`):** rejected; the
  docker-image hook needs local Docker, and a root `pyproject.toml` makes the Rust repo
  oddly `pip install`-able and reintroduces a publish-timing race.

## More Information

- Design doc: [`docs/design/distribution-pypi.md`](../design/distribution-pypi.md).
- Extends [ADR-0015](0015-distribution-strategy.md) (distribution tiering and
  supply-chain hardening); resolves `MP-L5` (pre-commit) and `MP-L7` (macOS floor pin)
  from [`docs/design/distribution.md`](../design/distribution.md).
- Implementation: `ci/scripts/build_wheels.py` (with `test-build-wheels.py`), the
  `publish-pypi` job in `.github/workflows/release.yml`, and the `pypi` smoke leg in
  `ci/scripts/smoke-channel.sh`.
