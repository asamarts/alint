# Design doc: PyPI distribution (wheels for `uvx` / `pipx` / pre-commit)

Status: Scoping (proposal). Not implemented. Scopes the PyPI channel that
`distribution.md` §4 lists as Tier-2 / P3 (demand-gated). Backed by research into
maturin/wheel mechanics and the ruff / uv / typos / dprint precedents, plus alint's
current build matrix and publish jobs.
Decisions: extends `distribution.md` (the §4 PyPI row, the §3.5 packaging-model lesson,
`MP-L5`, `MP-L7`) and ADR-0015. Recommends **Path B (repackage the attested release
binaries)** over Path A (maturin recompile); OIDC Trusted Publishing + PEP 740
attestations; and a separate `alint-pre-commit` mirror repo. No engine / rule / config
contract change. If promoted from demand-gated to a build, record as ADR-0016 or a
phase note on ADR-0015.

## 1. What this delivers (why it is worth doing)

- **Python-ecosystem install paths:** `uvx alint`, `pipx run alint`, `uv tool install
  alint`, `pip install alint` -- the reach that gates this channel.
- **A fast pre-commit hook.** A `language: python` hook that installs the prebuilt
  binary, retiring the current `language: rust` hook that compiles alint from source on
  every developer machine (`MP-L5`). This is the single strongest justification -- the
  pre-commit audience is exactly who asks for it (dprint ships `dprint-py` purely for
  this niche).
- **A strictly better packaging model than the current npm channel.** A wheel *embeds*
  the binary, so it avoids all four npm postinstall-download failure modes from §3.5
  (breaks under `bunx`/Bun, `npm install --ignore-scripts`, offline installs, and a
  deleted release). PyPI is the packaging model npm is migrating *toward*.

Non-goal reminder from `distribution.md`: alint is language-agnostic, so PyPI is
reach-into-an-ecosystem, not a natural home. That is why it stayed demand-gated; this
doc is the build plan for *if/when* we pull the trigger.

## 2. Mechanism: two proven paths, and why Path B

Both paths yield Python-version-agnostic **`py3-none-<platform>`** wheels that drop a
native executable on PATH via the wheel `.data/scripts/` category -- no PyO3, no Python
shim, no interpreter in the hot path. They differ only in where the binary comes from.

| | **Path A -- maturin `bindings="bin"` (recompile)** | **Path B -- repackage the prebuilt binary** |
| --- | --- | --- |
| Reference tools | ruff, uv, typos-cli (gold standard) | dprint-py, ziglang |
| Wheel payload | binary **recompiled** in CI by maturin | the **exact attested bytes** from our GitHub release |
| Byte-identical to the release tarball? | No (a separate build) | **Yes** |
| Provenance | wheel needs its own binary attestation | the existing release attestation transitively covers it (§7) |
| CI cost | a full 5-9 target **compile matrix** per release | one cheap runner (download + zip) |
| Code to maintain | ~5 lines of `[tool.maturin]` | a ~150-250 line `build_wheels.py` |
| sdist | natural (ships one; compiles on the user's box if no wheel) | none (nothing to compile) |

**Recommend Path B.** alint just spent the whole P1 supply-chain arc making *the*
binaries single-source and SLSA-attested; Path A would introduce a *third* build variant
(after the release matrix and the crates.io source build) that a `pip` user gets without
the release's own provenance. Path B ships the identical attested bytes, reuses the
existing 5-target matrix (no duplicate compile matrix), and matches the deterministic
`dprint-py` / `ziglang` precedent. Path A is the industry default and the documented
fallback if the custom builder ever becomes a burden.

**The Path B assembler** (`ci/scripts/build_wheels.py`, dprint-py pattern): for each of
the 5 release targets, download the release tarball (or reuse the `alint-<target>` CI
artifact), extract `alint` / `alint.exe` preserving the executable bit, write it to
`alint-<version>.data/scripts/alint[.exe]`, hand-write `.dist-info/{WHEEL,METADATA,RECORD}`
(`Root-Is-Purelib: false`, one `Tag:` per platform, `sha256=` + length per file in
RECORD), and zip to `alint-<version>-py3-none-<tag>.whl`. Deterministic (fixed
timestamps), no compiler, no container.

## 3. Wheel / platform mapping

Map the 5 current release triples to wheel tags. The **Linux binaries are musl-static**,
so they must carry a **compound `manylinux` + `musllinux` tag** -- a `musllinux`-only tag
would make `pip` on ordinary glibc hosts skip the wheel, even though the static binary
runs there fine.

| Release target | Wheel platform tag |
| --- | --- |
| `x86_64-unknown-linux-musl` | `manylinux_2_17_x86_64.musllinux_1_2_x86_64` |
| `aarch64-unknown-linux-musl` | `manylinux_2_17_aarch64.musllinux_1_2_aarch64` |
| `x86_64-apple-darwin` | `macosx_10_12_x86_64` |
| `aarch64-apple-darwin` | `macosx_11_0_arm64` |
| `x86_64-pc-windows-msvc` | `win_amd64` |
| *(P2)* `aarch64-pc-windows-msvc` | `win_arm64` -- **rides P2's win-arm64 build** |

- **Prerequisite `MP-L7`:** pin `MACOSX_DEPLOYMENT_TARGET` in CI so the mac tags are
  stable. Today the floor is the rustc per-target default (10.12 / 11.0) and *drifts* on
  toolchain bumps -- a wheel tag must not move silently.
- **sdist: none (wheels-only).** Path B has nothing to compile, and a source sdist would
  turn an unsupported platform from a clean "no matching wheel" error into a confusing
  Rust-compile failure for a `pip`/`uvx` user who has no toolchain. (If Path A were ever
  chosen, ship a documented sdist escape hatch instead.)

## 4. Publishing: OIDC Trusted Publishing + PEP 740

Mirror the existing tokenless posture (`publish-crates` via `crates-io-auth-action`;
`publish-npm` via `npm publish --provenance`, `NPM_TOKEN` retired):

- Publish with **`pypa/gh-action-pypi-publish`** under `permissions: { id-token: write,
  attestations: write }`. It performs the OIDC exchange (no `PYPI_API_TOKEN`) and
  **generates PEP 740 attestations by default** -- the turnkey guaranteed-attestation
  path (`uv publish`'s attestation emission is less certain, so prefer the PyPA action).
- **Claim the name via a pending Trusted Publisher.** On PyPI register a GitHub Actions
  publisher (project `alint`, owner `asamarts`, repo `alint`, workflow `release.yml`,
  environment `pypi`); a *pending* publisher both creates the project and OIDC-secures it
  on the first publish -- the same "configure once, validated at publish time" model as
  crates.io/npm.
- **Secrets-inventory gate:** no new row required. The `secrets-inventory` job only fires
  on `${{ secrets.X }}` references, and an OIDC job has none. (A documentation-parity row
  in `release-credentials.md` -- Keyless "yes -> Trusted Publishing" -- is optional. If a
  `PYPI_API_TOKEN` fallback were ever wired, it *would* need a row in the same PR.)

## 5. Pre-commit integration (a cross-language front-end, not a Python thing)

This is the one net-new artifact, so it is worth framing precisely: it is a **pre-commit
repo, not a Python repo**. pre-commit (pre-commit.com) is a language-agnostic git-hook
framework used by Go, JS, Rust, and polyglot-monorepo projects alike; a hook is
distributed as a git repo carrying a `.pre-commit-hooks.yaml`, referenced by consumers via
`repo:` + `rev:`. The **PyPI channel itself does not need this repo** -- `uvx` / `pipx` /
`pip install alint` all work without it. The repo is purely the pre-commit front-end, and
it *rides* the PyPI wheel only as its install vehicle: pre-commit's `language: python`
backend runs `pip install alint==<ver>`, pulling the prebuilt wheel (fast, no toolchain).
That `language:` line is plumbing, not a Python dependency for anyone using the hook.

**Reuse, consumer side: fully cross-language.** alint lints repo *structure*, so the same
hook serves every stack -- a Go, TypeScript, or Java-monorepo team all add the identical
`repo: asamarts/alint-pre-commit`, `rev: v<version>`, `hooks: [{id: alint}]`. It is the
opposite of Python-specific in practice.

**Reuse, as a container for other ecosystem front-ends: no.** A Homebrew tap, a GitHub
Action, and a pre-commit mirror each pin conflicting root-file conventions (`Formula/` vs
`action.yml` vs `.pre-commit-hooks.yaml` + pip-installability at root), so they stay
separate repos -- alint already keeps `asamarts/homebrew-alint` apart for the same reason.
There is no single "integrations" repo; this is specifically the pre-commit one.

**Recommended: the mirror repo** (ruff's `ruff-pre-commit` model). A trivial public
`asamarts/alint-pre-commit`: a `pyproject.toml` with `dependencies = ["alint==<version>"]`
(no package code) + a `.pre-commit-hooks.yaml` (`id: alint`, `entry: alint`,
`language: python`, `pass_filenames: false`, `require_serial: true`) + a `mirror.py` that
re-pins the version and `git tag v<version>` per alint release. It **decouples the
pre-commit `rev` from the release tag** -- the mirror re-tags only *after* the wheel is
confirmed live on PyPI, sidestepping the tag-exists-before-wheel-published race -- and
keeps the alint repo clean.

**Alternatives that avoid a second repo (with trade-offs), so the call is informed:**
- *Status quo, keep `language: rust`* (in the main repo): compiles alint from source on
  every dev machine. No second repo, no PyPI needed, but slow + needs rustc -- this *is*
  `MP-L5`, the thing PyPI is meant to retire.
- *`language: docker_image`* (in the main repo's `.pre-commit-hooks.yaml`): runs alint from
  the ghcr image. No second repo *and* no PyPI dependency; the trade-off is a local Docker
  requirement and a heavier per-run.
- *A root `pyproject.toml` in the alint repo* pinning `alint==<version>`: technically
  avoids a second repo (alint's root has no pyproject today, unlike ruff's maturin one),
  but it makes the Rust repo oddly `pip install`-able, clutters the root, and reintroduces
  the publish-timing race the mirror avoids.

**Decision:** the mirror is the price of a *fast* pre-commit hook, and it pays off for
every language's alint users, not Python's. If the pre-commit hook is not wanted, drop the
mirror and PyPI shrinks to `uvx`/`pipx`/`pip` (still useful, a smaller win).

## 6. CI / release integration

- **New `publish-pypi` job in `release.yml`**, gated on the `v*.*.*` tag, `needs: [build]`
  (download the `alint-<target>` matrix artifacts) or `needs: [release]` (download from the
  live Release), `permissions: { id-token: write, attestations: write }`: run
  `build_wheels.py` -> 5 wheels in `dist/` -> `pypa/gh-action-pypi-publish`. Mirrors the
  shape of `publish-crates` / `publish-npm`.
- **Bundle license text into the wheel** (`METADATA` license fields + `THIRD-PARTY-LICENSES`
  + `NOTICE`) for parity with the P1 tarball/Docker license bundle -- this also answers the
  PyPI analog of the "npm license text" tail item.
- **Post-publish smoke:** add a `pypi` leg to `ci/scripts/smoke-channel.sh` (`uvx
  alint@${VER} --version`, plus a `pipx run --spec alint==${VER}` / `pip install`
  assertion), a matrix row in `post-publish-smoke.yml`, and `publish-pypi` to that job's
  `needs:`.
- **Docs:** add a PyPI section to the installation page and the npm/pins surfaces (the same
  places §4/D-* touch).

## 7. Supply-chain continuity (the Path B payoff)

PEP 740 attests the **wheel file**; alint's existing `gh attestation` / cosign attests the
**release binary/tarball**. With Path B the wheel's payload *is* the attested release
binary, so the release attestation transitively vouches for the wheel's contents -- a
single, verifiable provenance chain across GitHub and PyPI, and the SBOM already published
for that binary describes the wheel's bytes too. Path A breaks this (the wheel binary is a
distinct build with only its own PyPI-side attestation). This is the core reason to prefer
Path B given the P1 investment.

## 8. Package name + metadata

- **`alint` is available on PyPI** (verified: the JSON API 404s, as does `alint-cli`).
  Claim `alint` via the pending Trusted Publisher; reserve early since availability can
  change. The PyPI project name is independent of the command -- even under a fallback
  `alint-cli`, the installed binary stays `alint` (set by the `.data/scripts/` filename).
- Metadata: `name = alint`, `version` tracks the crate, dual MIT/Apache license, homepage,
  a conservative `requires-python` floor (the binary needs no Python, but a floor is
  conventional), `Root-Is-Purelib: false`.

## 9. Phased build plan (small, ~1 release of work)

1. **Prereqs:** pin `MACOSX_DEPLOYMENT_TARGET` (`MP-L7`); claim `alint` + register the
   pending PyPI Trusted Publisher.
2. **Wheels + publish:** `build_wheels.py` (Path B) + the `publish-pypi` job -> 5 wheels
   with attestations. `uvx` / `pipx` / `uv tool install` / `pip install` work on all 5
   platforms.
3. **Pre-commit:** the `asamarts/alint-pre-commit` mirror repo + auto-sync (retires
   `MP-L5`).
4. **Verification + docs:** the smoke leg, the optional secrets-inventory parity row, and
   the installation-page PyPI section.
5. **Later (with P2):** the `win_arm64` wheel, once the win-arm64 build target lands.

## 10. Scope boundaries / non-goals

- No sdist (wheels-only). No maturin recompile (Path A) unless Path B's builder proves
  burdensome. No `win_arm64` wheel until P2 adds the target. No conda-forge (its own P3
  channel). No change to rules, config schema, `facts.json`, or any contract -- pure
  packaging. Not committing to *ship* this now -- this scopes the build so it can be
  promoted from demand-gated on a decision.

## 11. Open decisions (for review)

1. **Path A vs Path B** -- recommend **B** (byte-identical, single provenance chain, reuses
   the matrix). Accept the ~150-250 line assembler.
2. **The `alint-pre-commit` mirror repo** -- a second small public repo (a cross-language
   pre-commit front-end, not a Python artifact; see §5); the clean path to a fast
   pre-commit hook. §5 covers the docker-image / root-pyproject alternatives that avoid it.
3. **Name** -- claim `alint` (recommended) vs `alint-cli` fallback.
4. **Promote now or stay demand-gated** -- this doc makes it buildable; the trigger is a
   go decision, given PyPI is reach-not-home for a language-agnostic tool.

## Appendix: precedent + sources

| Tool | Build | Binary origin | Publish | sdist |
| --- | --- | --- | --- | --- |
| ruff / uv | maturin `bindings="bin"` | recompiled | `uv publish` + OIDC | yes |
| typos-cli | maturin `bindings="bin"` | recompiled | `maturin upload` + token | yes |
| **dprint-py** | bespoke `build.py` | **downloaded prebuilt** | `uv publish` + OIDC + PEP 740 | no |
| ziglang (zig-pypi) | bespoke `make_wheels.py` | downloaded prebuilt | -- | no |

Sources (verified 2026-08-28): maturin bindings + `.data/scripts/` packaging
(maturin.rs/bindings); ruff/uv `pyproject.toml` + `publish-pypi.yml` (astral-sh);
`dprint-py` `build.py` + `pyproject.toml` (trim21/dprint-py); ziglang `make_wheels.py`;
PyPI Trusted Publishing + PEP 740 GA (blog.pypi.org 2024-11-14, peps.python.org/pep-0740,
docs.pypi.org/trusted-publishers, pypa/gh-action-pypi-publish); ruff-pre-commit mirror
(astral-sh/ruff-pre-commit); PyPI name check (pypi.org JSON API). alint integration points
cited against `release.yml`, `ci/scripts/release-binary.sh`, `.pre-commit-hooks.yaml`,
`ci/scripts/smoke-channel.sh`, `docs/development/release-credentials.md`, and
`distribution.md` §2.1/§3.5/§4.
