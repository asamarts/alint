# Design doc: distribution and packaging strategy

Status: Proposed (2026-08-16). (Draft | Proposed | Implemented in `<commit>` | Superseded by `<doc>`.)
Decisions: [ADR-0015](../adr/0015-distribution-strategy.md) (distribution channel tiering and supply-chain hardening).
Scope: how alint's binary and editor artifacts reach users, which channels we own vs automate vs delegate to community packagers, and how we make the supply chain verifiable. This is an evergreen strategy doc (like [`deterministic-perf-gating.md`](deterministic-perf-gating.md)), not a per-version rule-kind spec. It is the canonical, self-contained record of the distribution audit and expansion plan; the file:line evidence below is preserved so the register can be re-verified against the tree.

---

## 1. Problem

Distribution is alint's adoption funnel *and* its supply-chain trust boundary. The
current story is genuinely good but narrow and under-hardened, and it has grown by
accretion rather than to a plan. Three problems:

1. **Trust is asserted, not verifiable.** `SECURITY.md` names "supply-chain
   integrity for everyone who uses it" as a core value, yet nothing lets a
   consumer verify a downloaded binary was built by this repo. The `.sha256`
   companions are fetched from the *same* GitHub Release as the tarball
   (`install.sh:60-62`, `npm/install.js:115-117`), so they prove transit
   integrity, not authenticity: a compromised release (or a compromised
   publishing account) swaps tarball and checksum together. The v0.15.0 release
   carries zero signatures, provenance, or SBOM assets. crates.io (keyless OIDC
   trusted publishing) is the *only* channel today with real provenance.

2. **Reach has holes and unclaimed near-free wins.** Windows users get only a
   manual-tarball path from the canonical install page and the official GitHub
   Action silently fails on Windows runners; several high-ROI channels that fall
   out of the *existing* release assets for almost no work (cargo-binstall, a Nix
   flake, Scoop, WinGet, mise) are unclaimed; and the npm wrapper uses the
   fragile postinstall-download model that breaks under `bunx`, `--ignore-scripts`,
   and offline installs.

3. **Small correctness debt.** A copy-paste-broken `brew install` command on the
   marketing site, npm advertised on a page that omits it, a Zed extension pinned
   two minors stale and invisible to the version-pin gate, and a release pipeline
   that couples the irreversible crates.io publish to an unrelated build-matrix
   leg.

**Strategic frame (the insight that should drive the plan).** A mature
single-binary Rust CLI does *not* own every channel. ripgrep is the proof:
upstream's real per-release surface is just **crates.io + GitHub release
binaries** (both fall out of one CI run); its ~20 other packages (Debian, Arch,
Fedora, nixpkgs, Homebrew-core, Scoop, WinGet, Chocolatey, the BSD ports) are
maintained by *those* ecosystems, and upstream publicly disowns its snap. ruff,
uv, and Biome follow the same shape (section 5). So the goal is **not** "be
everywhere ourselves." It is to:

- **(a)** grab the handful of near-free, auto-updating channels that resolve or
  rebuild from assets we already publish;
- **(b)** automate, in our own CI, the few high-reach channels we want to *own*;
- **(c)** make the trust chain verifiable end to end;
- **(d)** let popularity pull the long tail in via community packagers, and help
  them (stable tags, conventional asset names, an SBOM) rather than duplicate them.

---

## 2. Current state

### 2.1 Platform matrix

Exactly **5 target triples** are built (`release.yml:107-121`); all Linux is
**musl-only** (static, no glibc build):

| Triple | Built | How |
|---|:--:|---|
| `x86_64-unknown-linux-musl` | ✅ | ubuntu-latest, native |
| `aarch64-unknown-linux-musl` | ✅ | ubuntu-latest via `cross` |
| `x86_64-apple-darwin` | ✅ | macos-15-intel |
| `aarch64-apple-darwin` | ✅ | macos-latest (Apple Silicon) |
| `x86_64-pc-windows-msvc` | ✅ | windows-latest |
| `aarch64-pc-windows-msvc` | ❌ | not in matrix |
| any `*-linux-gnu` | ❌ | musl-only by design |

**musl-only Linux is a strength, not a gap:** one static binary serves glibc
distros, Alpine/musl, and the distroless Docker image with no gnu variant to
maintain. The real hole is **Windows**: no arm64 build at all, and the win-x64
binary is reachable only via npm and `cargo` (not install.sh, not the Action, not
a Windows package manager). Release assets are named
`alint-v{version}-{target}.tar.gz` (+ `.sha256`), plus `install.sh` and
`SHA256SUMS` (verified against the live v0.15.0 release).

### 2.2 Live channels (nine, all auto-published on the `v*` tag)

The whole system is one `needs:`-chained dependency graph in `release.yml`, all
triggered by a `v*.*.*` tag push:

```
preflight ─▶ build (5-target matrix) ─▶ release ─▶ publish-npm
                     │                     │      ─▶ homebrew
                     ├─▶ docker            │      ─▶ publish-vscode (+Open VSX)
                     └─▶ publish-crates    └─▶ publish-jetbrains
```

| Channel | Mechanism | Coverage | Auth |
|---|---|---|---|
| **GitHub Releases** | tarball + per-file `.sha256` + concatenated `SHA256SUMS`; also force-moves the `v0` major tag so `@v0` tracks latest | all 5 triples | `GITHUB_TOKEN` |
| **install.sh** (`curl \| bash` via alint.org) | platform-detect → SHA-256 verified download → `$HOME/.local/bin` | 4 (no Windows) | none |
| **GitHub Action** `asamarts/alint@v0` | composite; fetches install.sh at the consumer's pinned ref with 3x retry; binary version derives from the action ref | 4 (no Windows) | none |
| **crates.io** `cargo install alint` | source build; publishes 6 crates in dep order; **keyless OIDC** trusted publishing | any Rust target | OIDC |
| **npm** `@asamarts/alint` | **postinstall-download** wrapper (no native bytes in the tarball; `install.js` downloads + SHA-256-verifies at install time) | 5 (declares win-arm64 it can't serve) | `NPM_TOKEN` (PAT) |
| **Docker** `ghcr.io/asamarts/alint` | distroless-static, nonroot (UID 65532), re-extracts the *release* binaries so the image is byte-identical to the tarballs; tags `:vX.Y.Z`/`:X.Y.Z`/`:X.Y`/`:latest` | linux amd64+arm64 | `GITHUB_TOKEN` |
| **Homebrew tap** `asamarts/homebrew-alint` | formula regenerated from `SHA256SUMS` in CI (no rebuild), golden-tested in preflight, committed over SSH | macOS arm/intel + Linuxbrew x64/aarch64 | SSH deploy key |
| **VS Code + Open VSX** | `vsce publish` + `ovsx publish`, version stamped from tag; real-client e2e in `editors-e2e.yml` | n/a | `VSCE_PAT`, `OVSX_PAT` |
| **JetBrains Marketplace** | `gradle publishPlugin` (one LSP4IJ plugin covers the suite); hardened by a bytecode deny-scan finalizer | n/a | token + signing cert/key/password |
| pre-commit hook | `.pre-commit-hooks.yaml`, `language: rust` → compiles from source (ignores prebuilt binaries) | any Rust target | none |
| Zed / Neovim / Emacs / Sublime | **manual, PR-based, post-release** to each registry | n/a | n/a |
| Helix / Eclipse | docs-only config snippets (nothing to publish) | n/a | n/a |

### 2.3 What is already good (do not regress)

- **One tag fans out to every channel** via a single `needs:`-chained `release.yml`.
- **crates.io is keyless (OIDC trusted publishing)** — the provenance model to extend.
- **The Homebrew tap is already zero-touch:** `update-homebrew-formula.sh`
  regenerates the formula from the release manifest (no rebuild) and is
  golden-tested in preflight. No per-release human step.
- **Docker is distroless-static + nonroot**, and re-extracts the *release*
  binaries so the image is byte-identical to the tarballs. Being OCI-standard, it
  runs unchanged under podman/containerd/nerdctl.
- **The JetBrains internal-API deny-scan** (`verifyNoMarketplaceDeniedApis`) is a
  robust, well-engineered gate against a real class of mid-release rejection.
- **No cross-repo version drift:** every authored surface in both repos pins
  v0.15.0 consistently, held by `check-version-pins.sh` and the alint.org pin gate.
- **`alint.org/install.sh` is not broken:** it 302-redirects to the repo's `main`
  copy (a disclosed mutable-`main` caveat, with a tag-pinned alternative offered),
  not a stale static copy.

---

## 3. Findings register (harden)

Deduplicated from a two-repo audit (release machinery + documentation). Severity is
impact x likelihood. file:line is evidence for re-verification, not exhaustive.

### 3.1 Machinery

| ID | Sev | Finding | Evidence | Fix |
|---|:--:|---|---|---|
| **H1** | HIGH | No signing / SLSA provenance / SBOM / attestation on any binary artifact; `.sha256` proves transit only. | grep across `.github/`, `install.sh`, `Dockerfile`, `npm/`; v0.15.0 assets carry none; `release.yml:305-317` docker sets no attestation | section 6 |
| **H2** | HIGH | Official GitHub Action silently fails on Windows runners (composite → install.sh hard-exits) though a win-x64 binary exists. | `action.yml:56-130`, `install.sh:32-38` | Windows path + doc note |
| **M1** | MED | Build matrix needlessly gates the irreversible crates.io publish (and docker). A flaky win/aarch64 leg blocks a publish needing zero binaries. | `release.yml:104` (`fail-fast:false`), `:329` (`publish-crates needs: build`) | `publish-crates` → `needs: preflight` |
| **M3** | MED | Four expiring-token channels, each a split-brain single point of failure, all run after the Release exists. | `NPM_TOKEN release.yml:375`; `VSCE_PAT/OVSX_PAT :475-476`; JetBrains trio `:525-528`; `HOMEBREW_TAP_DEPLOY_KEY :420` | migrate npm to OIDC; document rotation; prefer keyless |
| **M4** | MED | npm declares `win32/arm64` (os×cpu) it cannot serve; hard postinstall error instead of clean `EBADPLATFORM`. | `npm/package.json:31-32`, `npm/install.js:52-58` | add win-arm64 target + fix declaration |
| **L2** | LOW | install.sh resolves "latest" via the unauthenticated GitHub API (60 req/hr/IP); 403s on shared CI egress. | `install.sh:48` | doc `ALINT_VERSION` pin / cache |
| **L4** | LOW | `action-selftest` pins `v0.12.0` though its comment says track the previous minor (should be v0.14.0). | `action-selftest.yml:94` | bump to previous minor each release |
| **L5** | LOW | `cross` installed unpinned at release time (fresh unversioned build dep in the aarch64 hot path). | `release-binary.sh:34-40` | pin `--version` |
| **L6** | LOW | Four editor channels (Zed/Neovim/Emacs/Sublime) are source-only, manual, post-release; only VS Code + JetBrains are tag-gated. | `RELEASING.md:209-218` | automate what can be; track the rest |
| **L1** | INFO | install.sh exists in three independently-updatable copies (release-pinned; `main` served via raw + the alint.org 302; the Action fetches at the consumer's ref). The headline `curl` pulls from mutable `main`. | `install.sh` header, `release.yml:190`, `action.yml:116`, alint.org `_redirects:9` | pair with signing (section 6) |

### 3.2 Version-pin blind spot

| ID | Sev | Finding | Evidence | Fix |
|---|:--:|---|---|---|
| **M2** | MED | Zed extension pinned at 0.13.0 (two minors stale), manual PR, **not in the pin gate** — so the gate falsely implies "all versions match." | `editors/zed/extension.toml:3`, `editors/zed/Cargo.toml:3`; `check-version-pins.sh:51-58` scope | add every editor manifest to `check-version-pins.sh` |

### 3.3 Documentation

| ID | Sev | Finding | Evidence | Fix |
|---|:--:|---|---|---|
| **H1-d** | HIGH | Copy-paste-broken `brew install alint` with **no tap** — errors "No available formula" on a clean machine (alint is tap-only). | alint.org `src/pages/migrating-from/ls-lint.astro:389` | qualify to `asamarts/alint/alint` |
| **H2-d** | HIGH | npm is a broken promise: the canonical install page has no npm section, yet the docs landing advertises "install via … npm …" and links there. | `docs/site/getting-started/installation.md` (no npm); `index.mdx:26,42` | add npm section (+ `about/index.md` links) |
| **M2-d** | MED | "install.sh (Linux + macOS + Windows tarballs)" heading implies `curl \| bash` covers Windows; it does not, and npm/cargo (which do) are not surfaced to Windows users there. | `installation.md:21`, `README.md:97` | reframe; surface Windows paths |
| **M1-d** | MED | Homebrew shown in two divergent (both-correct) forms across the most-visited surfaces. | `README.md:108-109` etc. vs `agent-friendly-linter.astro:184`, `why-alint.md:214` | standardize on one |
| **M3-d** | LOW | Broken npm-README anchor (`#homebrew` vs `#homebrew-macos--linuxbrew`). | `npm/README.md:37` | fix anchor |
| **L1-d** | LOW | Stale + self-inconsistent Docker minor-tag examples (`:0.13` vs `:0.10`; current 0.15). | `README.md:161`, `installation.md:49` | refresh / de-hardcode |
| **L2-d** | LOW | Ancient npm version example `@asamarts/alint@0.5.11`. | `npm/README.md:26` | refresh |
| **L3-d** | LOW | Homepage snippet omits `cargo`. | alint.org `index.astro:35-46` | add or accept as teaser |
| **L4-d** | LOW | `about/index.md` project links omit npm + the GitHub Action (inconsistent with `SECURITY.md:43-51`). | `docs/site/about/index.md:30-36` | align |
| **L5-d** | INFO | Local synced-docs show v0.14.2 — gitignored build artifacts re-synced from the tag at deploy; live site is correct. | `src/content/docs/docs/.../installation.md` | none (benign) |
| **L3-m** | LOW | RELEASING.md says editor-marketplace prereqs are "NOT yet done" though the jobs demonstrably publish. | `RELEASING.md:195-206` | update runbook |

### 3.4 Gaps (expected channels no surface documents)

`G1` no Windows package manager (winget/scoop/chocolatey); `G2` no
`cargo binstall` (though prebuilt tarballs + `.sha256` already exist); `G3` no
Nix/AUR/apt/snap/flatpak. All addressed in section 4.

### 3.5 npm packaging model (MED)

The **postinstall-download** model breaks under **`bunx`/Bun** (blocks lifecycle
scripts by default), **`npm install --ignore-scripts`** (common in hardened CI),
and **offline/air-gapped** installs. Migrate to the **optionalDependencies** model
(section 4) and add the missing win-arm64 target.

---

## 4. Channel expansion

Effort: **S** = a file or docs line; **M** = a CI job / workflow + any account or
secret; **L** = a multi-week external or social process.

### Tier 1 — adopt now (best effort:reward)

| Channel | Effort | Rationale |
|---|:--:|---|
| **cargo-binstall** | S | Highest ROI. Our assets are already named `alint-v{version}-{target}.tar.gz`, which matches binstall's `{name}-v{version}-{target}` probe, so `cargo binstall alint` likely resolves today — verify with `--dry-run` and add a one-time `[package.metadata.binstall]` `bin-dir` if the nested tarball path (`alint-v{version}-{target}/alint`) needs it. Then document it so Rust users stop compiling from source. |
| **In-repo `flake.nix`** | S | One file → `nix run github:asamarts/alint`, no external gatekeeper, rebuilds from the tag. Precedent: eza ships its own flake. |
| **podman (docs only)** | S | Not a channel: podman runs the existing OCI image unchanged. The one nuance is short-name resolution (podman does not implicitly prepend `docker.io/`), so document a **fully-qualified** `podman run ghcr.io/asamarts/alint check`. Rootless needs nothing from us. |
| **mise (`ubi`/`github` backend)** | S | `mise use ubi:asamarts/alint` resolves our releases live with no plugin or manifest (our conventional asset names already satisfy it); optional one-time registry PR for a bare shortname. Also covers asdf users, so no bespoke asdf plugin is needed. |
| **Third-party fetchers (docs only)** | S | `eget asamarts/alint`, `ubi`, `bin` pull our existing releases with zero work from us — one docs line, and the honest answer for "`go install`" users (Tier 3). |
| **Scoop self-hosted bucket** | M | Self-updating on Windows: `checkver:github` + `autoupdate` + the Excavator action bump version and hash for us. Mirrors the Homebrew-tap pattern. |
| **WinGet (WinGet Releaser action)** | M | Largest native-Windows audience (ships in Win11). The action auto-opens the manifest PR each release; the only friction is Microsoft's merge review. |
| **`.deb` + `.rpm` on Releases** | M | `cargo-deb` and `cargo-generate-rpm` generate from `Cargo.toml` in CI and attach to the Release (glibc + musl variants). Standard modern-Rust-CLI move (precedent: bottom). Not an apt/dnf *repo* — just downloadable packages. |
| **AUR `alint-bin`** | M | Arch users expect it; `github-actions-deploy-aur` pushes over SSH and regenerates `.SRCINFO`/checksums. We own it, fully CI-driven. |
| **npm → optionalDependencies** | M | A hardening migration, not new reach (§3.5). Adopt Biome's pure model (per-platform `@asamarts/alint-<target>` packages tagged with `os`/`cpu`, a thin `bin` shim, **no install script**), or esbuild's hybrid (optionalDeps + a postinstall fallback for `--no-optional`). Add win-arm64. Then `npx alint` and `bunx alint` both work. |

Reconciling Tier 1 with what we already do: the **Homebrew tap is already
auto-bumped** (no `bump-formula-pr` wiring needed), and Docker is already on
**GHCR** (the registry without Docker Hub's anonymous pull throttle) — a Docker
Hub *mirror* for `docker run` short-name discoverability is optional, low-priority.

### Tier 2 — adopt later (gated on eligibility, demand, or consolidation)

| Channel | Effort | Rationale |
|---|:--:|---|
| **Homebrew core** | M | Tap-less `brew install alint` + free BrewTestBot autobump, but requires notability (the firm audit bar is **≥75 stars OR ≥30 forks OR ≥30 watchers**), a stable versioned release (no HEAD-only / self-updating), builds green on their CI, and ~30 days age. Submit when we clear the bar; **keep the tap either way**. |
| **nixpkgs** | M | Wide reach, mostly bot-maintained after entry — but do the in-repo flake first, and wire a `passthru.updateScript` because r-ryantm cannot auto-bump Rust's `cargoHash`. |
| **conda-forge** | M | Cheap to keep once in (regro autotick bot bumps; we merge), but the audience skews data-science — worth the one-time staged-recipes PR on demand. |
| **Self-owned COPR** | M | Native `dnf install` UX for Fedora/RHEL via webhook auto-rebuild, no official-Fedora gatekeeper (precedent: ripgrep's community COPR). |
| **PyPI (maturin `bindings="bin"` wheels)** | M | Fully automatable and clean (how ruff and typos ship: per-platform wheels via `maturin-action` + OIDC). **Only worth it for the Python-ecosystem wins** — `uvx alint`, `uv tool install`, `pipx run alint`, and pinnable **pre-commit** hooks in Python/mixed monorepos. A language-agnostic tool is not a natural PyPI resident (Biome and dprint deliberately skip it); adopt only if we want that reach. |
| **cargo-dist / `dist`** | M–L | A *consolidation* play: it would regenerate install.sh/npm/Homebrew/the release workflow and hand us binstall metadata + shell/PowerShell installers for free. But it replaces plumbing we already have and adds single-vendor bus-factor (the vendor has sunset a paid product once). Evaluate when hand-rolled upkeep starts to hurt. |
| **Snap** | M+ | A path-reading linter needs **classic** confinement → a one-time manual Canonical review. Modest incremental reach; low priority. |

### Tier 3 — skip (not worth it or structurally impossible)

| Channel | Why skip |
|---|---|
| **Go (`go install`)** | Structurally cannot distribute a non-Go binary — it fetches Go *source* and compiles it. Point users at the Tier-1 fetchers (`eget`/`ubi`/`bin`). |
| **Flatpak / Flathub** | GUI/desktop-oriented; the sandbox fights a linter that reads arbitrary repo paths. No flagship-CLI precedent. |
| **Alpine aports** | The static musl binary already runs on Alpine; aports entry is maintainer-gated for little marginal value. |
| **Official Debian/Ubuntu, official Fedora** | Need an external Debian-Developer / Fedora-packager sponsor (weeks-to-months, out of our control). The self-owned COPR covers the `dnf` UX without the gate; revisit only on real popularity. |
| **Bespoke asdf plugin** | Superseded by mise's `github`/`ubi` backend. |
| **taiki-e/upload-rust-binary-action** | Not a channel — a build helper; its default naming is conveniently binstall-compatible, so it is an optional internal simplification of the release job, nothing more. |

---

## 5. Precedent (comparable Rust CLIs)

The compact evidence for the section-1 frame. **SELF** = upstream-run (per-release
CI); **COMMUNITY** = distro/packager-run (out of upstream's hands).

| Tool | SELF (upstream owns) | COMMUNITY (delegated) |
|---|---|---|
| **ripgrep** (gold standard) | crates.io + GitHub binaries only | Homebrew-core, Debian, Arch, Fedora, nixpkgs, Scoop, WinGet, Chocolatey, MacPorts, BSD ports; snap disowned |
| **ruff** (astral) | curl installer + PyPI(maturin) + GHCR + GH binaries | Homebrew-core, conda-forge, Arch, Alpine. **No npm CLI** |
| **biome** | npm(optionalDependencies) + standalone binaries + Docker | Homebrew-core. crates.io is a placeholder |
| **uv** (astral) | curl/PS installer + PyPI + GHCR + GH binaries + WinGet | Homebrew, Scoop, Nix, pacman |
| **dprint** | curl installer + npm + crates.io(+binstall) + GH binaries | Homebrew, Scoop. **Not on PyPI** |
| **typos-cli** | crates.io + GH binaries + pre-commit/Action + PyPI wheels | Homebrew, conda, Arch. No npm |
| **bottom** | GH binaries incl. `.deb`/`.rpm` (glibc+musl) + COPR | AUR, nixpkgs |

Takeaways: (1) every one keeps SELF small; (2) the npm-CLI precedents are Biome and
esbuild, **not** ruff (ruff has no npm CLI package); (3) PyPI adopters all have
Python-developer users; (4) `.deb`/`.rpm`-on-Releases + a self-owned COPR is the
common "native Linux UX without the distro gate" pattern.

---

## 6. Supply-chain hardening (the H1 plan)

The objective: a consumer can prove a downloaded alint artifact was built by this
repo's CI from this repo's source, using tooling that needs no long-lived secret.
Extend the keyless (OIDC) posture crates.io already has to every binary artifact.

1. **Build provenance / attestation (SLSA).** Add `actions/attest-build-provenance`
   to `release.yml` for every Release tarball *and* the ghcr image → signed,
   OIDC-backed provenance (no secret to expire), verifiable with
   `gh attestation verify <artifact> --repo asamarts/alint`. For the image, also
   enable buildx/registry attestations (the release image currently sets none).

2. **Keyless signing (Sigstore / cosign).** cosign-sign the tarballs and the ghcr
   image via OIDC → `cosign verify` / `cosign verify-attestation`. This is the
   authenticity layer the `.sha256` files cannot provide, without a GPG/minisign
   key to store, rotate, and trust-distribute.

3. **SBOM.** Build with `cargo auditable` (embeds the dependency graph in the
   binary, queryable post-hoc) and attach a CycloneDX or SPDX SBOM (via
   `cargo cyclonedx` or syft), attested alongside the binary.

4. **Verification UX.** Teach `install.sh` and the npm shim to *optionally* verify
   provenance/signature when `gh` or `cosign` is present (never a hard dependency),
   and document `gh attestation verify` / `cosign verify` on the install page and
   in `SECURITY.md`. Pair this with the mutable-`main` caveat (`L1`): signing plus
   an optionally tag-pinned installer closes the loop.

This is the single highest-trust-ROI item in the plan and the one most aligned with
what alint claims to stand for.

---

## 7. Maintenance model (free vs ongoing)

Classifying every channel by who does the per-release work makes the sprawl risk
explicit. Prefer the top two bands.

- **Truly free — no per-release upstream work** (resolves/rebuilds from our
  existing releases, or a bot bumps it): cargo-binstall, in-repo `flake.nix`,
  podman, mise (`ubi`/`github`), Scoop bucket with `checkver`/`autoupdate` +
  Excavator, and — once admitted — homebrew-core (BrewTestBot), conda-forge
  (autotick bot), nixpkgs (r-ryantm, with the `cargoHash` updateScript caveat).
- **Semi — auto-PR authored, external human merges each release:** WinGet (the
  releaser action opens the PR; Microsoft reviews). PAT expiry is the classic
  failure mode.
- **Ongoing per-release but fully automated in our CI (team-owned):** npm (either
  model), `.deb`/`.rpm` attachment, AUR `alint-bin`, the Homebrew tap bump, COPR
  (webhook rebuild). Every tag, no human.
- **Manual / external gatekeeper (cannot self-drive entry):** official
  Debian/Ubuntu, official Fedora, Alpine aports — need a sponsor to admit; then the
  distro maintainer updates it (free to us, out of our hands). Today's manual
  editor channels (Zed/Neovim/Emacs/Sublime) also live here until automated.

---

## 8. Answers to the specific questions asked

- **cargo-binstall — metadata or just naming?** Just naming, usually. It resolves
  at the user's install time from crates.io metadata → our GitHub Releases, trying
  templates like `{name}-v{version}-{target}` (v-prefix optional, `.tar.gz`/`.zip`
  auto-tried). Our `alint-v{version}-{target}.tar.gz` matches; only the nested
  in-archive bin path may need a one-time `[package.metadata.binstall]` `bin-dir`.
  Verify with `cargo binstall --dry-run`; that is the whole task.
- **Nix — nixpkgs vs flake?** In-repo flake first (Tier 1: one file, no gatekeeper,
  instant `nix run`), nixpkgs later (Tier 2) for discoverability, with a
  `passthru.updateScript` because the generic bot cannot bump `cargoHash`.
- **npm — is our approach best practice?** No — migrate. We are on the
  postinstall-download model (verified in-repo), which breaks under
  Bun/`--ignore-scripts`/offline. Best practice is optionalDependencies (Biome
  pure model, or esbuild hybrid). Add win-arm64 while there.
- **PyPI — worth it for a non-Python tool?** Technically excellent and fully
  automatable (maturin `bindings="bin"`, per-platform wheels, OIDC — how ruff and
  typos ship), **but** every Rust CLI that chose PyPI has Python users. Adopt only
  for the concrete wins: `uvx`/`uv tool install`/`pipx run` and pinnable pre-commit
  hooks. Otherwise our curl installer + Homebrew + npm already reach these users.
- **Go (`go install`) — can it ship a Rust binary?** No, flatly — it fetches Go
  source and compiles. The only Go-adjacent option is third-party fetchers
  (`eget`/`ubi`/`bin`) that grab our existing releases; a one-line docs mention.
- **podman — separate channel or same image?** Same image, no separate channel,
  genuinely free. Docker and podman are both OCI; one artifact also serves
  containerd/nerdctl/Kubernetes. Only action: document a fully-qualified registry.
- **Homebrew — core vs tap?** Keep the tap now (full control, instant releases,
  already zero-touch); add core later when we clear the star/fork bar. You can have
  both.

Research corrections folded into the above (so premises are not carried forward
wrong): ruff has **no** npm CLI package (it is a PyPI precedent); dprint is **not**
on PyPI (the PyPI `dprint` is unrelated); `ubi` is written in Rust (only `eget`/`bin`
are Go); cargo-dist is **not** deprecated (renamed `dist`, canonical upstream), the
real caveat is single-vendor bus-factor.

---

## 9. Phased plan

Each phase is independently shippable; nothing here changes rule semantics or
`.alint.yml`. Ordering is by ROI and by what unblocks what.

- **P0 — correctness + near-free wins (no release required for most).** Fix the
  broken `brew install` command (`H1-d`); add npm to `installation.md` and the
  `about` links (`H2-d`, `L4-d`); standardize the Homebrew form (`M1-d`); fix the
  npm win-arm64 declaration (`M4`), the broken anchor (`M3-d`), the stale examples
  (`L1-d`, `L2-d`), and the RELEASING note (`L3-m`); add every editor manifest to
  `check-version-pins.sh` (`M2`); bump the `action-selftest` pin (`L4`); decouple
  `publish-crates` from the build matrix (`M1`); pin `cross` (`L5`). **Document**
  cargo-binstall (`G2`), podman (fully-qualified), and the third-party fetchers.

- **P1 — supply-chain hardening (next release).** Section 6: attestation + cosign +
  SBOM + verification docs. Highest trust ROI; keyless, so no new secret debt.

- **P2 — owned high-reach channels (one pipeline pass over 1-2 releases).** npm →
  optionalDependencies (+ win-arm64); Scoop bucket; WinGet Releaser; `.deb`/`.rpm`
  on Releases; AUR `alint-bin`; in-repo `flake.nix`; mise registry entry. Each is
  CI-automated and covered by the (expanded) version-pin gate; no per-release human.

- **P3 — eligibility / demand-gated.** Homebrew core (at the star bar), nixpkgs
  (after the flake), conda-forge, self-owned COPR, PyPI wheels (if we want Python
  reach), a cargo-dist consolidation evaluation, and the Windows-arm64 build target
  (cheap once the other Windows work exists).

---

## 10. Risks and non-goals

- **Non-goal: own every distro package.** Debian/Arch/Fedora/nixpkgs/MacPorts are
  community-packager territory; we make their job easy (stable tags, conventional
  asset names, an SBOM) rather than duplicate it.
- **Risk: channel sprawl → maintenance drag.** Mitigation: every *owned* channel
  must be CI-automated and covered by the version-pin gate; prefer channels in the
  top two maintenance bands (section 7).
- **Risk: cargo-dist single-vendor bus-factor.** Treat consolidation as optional;
  do not couple the release to it without an exit plan.
- **Risk: PyPI positioning mismatch.** Shipping a language-agnostic tool on PyPI
  can confuse positioning; adopt only for the concrete `uvx`/pre-commit wins,
  framed as an integration, not a primary home.
- **Windows-arm64** joins the build matrix on demand; it is cheap once the other
  Windows work (Scoop/WinGet/npm) exists.

---

## 11. Decisions

The channel tiering (section 4) and the supply-chain posture (section 6) are
ratified in [ADR-0015](../adr/0015-distribution-strategy.md). The audit findings
(section 3) are tracked as follow-up work; P0 is mechanical and can land ahead of
the ADR's acceptance.
