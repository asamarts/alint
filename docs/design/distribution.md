# Design doc: distribution and packaging strategy

Status: Proposed (2026-08-16). (Draft | Proposed | Implemented in `<commit>` | Superseded by `<doc>`.)
Decisions: [ADR-0015](../adr/0015-distribution-strategy.md) (distribution channel tiering and supply-chain hardening).
Scope: how alint's binary and editor artifacts reach users, which channels we own vs automate vs delegate to community packagers, and how we make the supply chain verifiable. This is an evergreen strategy doc (like [`deterministic-perf-gating.md`](deterministic-perf-gating.md)), not a per-version rule-kind spec. It is the canonical, self-contained record of the distribution audit and expansion plan; the file:line evidence below is preserved so the register can be re-verified against the tree. Finding IDs are namespaced — `MP-*` machinery/pipeline, `VP-*` version-pin, `D-*` documentation — so section 9 can trace every one to a phase without collision.

---

## 1. Problem

Distribution is alint's adoption funnel *and* its supply-chain trust boundary. The
current story is genuinely good but narrow and under-hardened, and it has grown by
accretion rather than to a plan. Three problems:

1. **Trust is asserted, not verifiable.** `SECURITY.md`'s own threat framing puts
   "supply-chain integrity for everyone who uses it" at stake (line 4), yet nothing
   lets a consumer verify a downloaded binary was built by this repo. The `.sha256`
   companions are fetched from the *same* GitHub Release as the tarball (URL
   derivation at `install.sh:60-62` / `npm/install.js:115-117`, fetched at
   `install.sh:70-71` / `npm/install.js:120`), so they prove transit integrity, not
   authenticity: a compromised release, or a compromised publishing account, swaps
   tarball and checksum together. The v0.15.0 release carries zero signatures,
   provenance, or SBOM assets. crates.io is the only channel with keyless
   *publishing* today (§6 explains why that is not the same as consumer-verifiable
   provenance).

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
uv, and Biome follow the same shape (§5). So the goal is **not** "be everywhere
ourselves." It is to:

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

**musl-only Linux is mostly a strength:** one static binary serves glibc distros,
Alpine/musl, and the distroless Docker image with no gnu variant to maintain, and
the DNS/NSS `getaddrinfo` concerns that dog musl are moot for a linter that does no
networking. The **one real tradeoff**: musl's default allocator is materially
slower than glibc's for allocation-heavy workloads (the documented reason ripgrep
ships a gnu build or overrides the allocator), and alint sets no `jemalloc`/
`mimalloc`/`global_allocator` override — a latent cost for a file-heavy linter that
also runs a deterministic-perf-gating regime. We accept it for now; adding a
`mimalloc` feature (not a gnu target) is the cheaper mitigation if a perf gap
appears (§9 P3). The real reach hole is **Windows**: no arm64 build, and the
win-x64 binary is reachable only via npm and `cargo` (not install.sh, not the
Action, not a Windows package manager). Release assets are named
`alint-v{version}-{target}.tar.gz` (+ `.sha256`), plus `install.sh` and
`SHA256SUMS` (verified against the live v0.15.0 release).

### 2.2 Live channels (nine auto-published on the `v*` tag, plus three manual/docs-only)

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
| **GitHub Releases** | tarball + per-file `.sha256` + concatenated `SHA256SUMS`; also force-moves the `v0` major tag so `@v0` tracks latest (see `MP-N2`) | all 5 triples | `GITHUB_TOKEN` |
| **install.sh** (`curl \| bash` via alint.org) | platform-detect → SHA-256 verified download → `$HOME/.local/bin` (prints a PATH hint if that dir is not on `PATH`, `install.sh:104-108`) | 4 (no Windows) | none |
| **GitHub Action** `asamarts/alint@v0` | composite; fetches install.sh at the consumer's pinned ref with 3x retry; binary version derives from the action ref | 4 (no Windows) | none |
| **crates.io** `cargo install alint` | source build; publishes 6 crates in dep order; **keyless OIDC** publishing | any Rust target | OIDC |
| **npm** `@asamarts/alint` | **postinstall-download** wrapper (no native bytes in the tarball; `install.js` downloads + SHA-256-verifies at install time) | 5 (declares win-arm64 it can't serve) | `NPM_TOKEN` (PAT) |
| **Docker** `ghcr.io/asamarts/alint` | distroless-static, nonroot (UID 65532), re-extracts the *release* binaries so the image is byte-identical to the tarballs; tags `:vX.Y.Z`/`:X.Y.Z`/`:X.Y`/`:latest` | linux amd64+arm64 | `GITHUB_TOKEN` |
| **Homebrew tap** `asamarts/homebrew-alint` | formula regenerated from `SHA256SUMS` in CI (no rebuild), golden-tested in preflight, committed over SSH | macOS arm/intel + Linuxbrew x64/aarch64 | SSH deploy key |
| **VS Code + Open VSX** | `vsce publish` + `ovsx publish`, version stamped from tag; real-client e2e in `editors-e2e.yml` | n/a | `VSCE_PAT`, `OVSX_PAT` |
| **JetBrains Marketplace** | `gradle publishPlugin` (one LSP4IJ plugin covers the suite); hardened by a bytecode deny-scan finalizer | n/a | token + cert/key/password (4 secrets) |
| pre-commit hook (manual/source) | `.pre-commit-hooks.yaml`, `language: rust` → compiles from source (see `MP-L5`) | any Rust target | none |
| Zed / Neovim / Emacs / Sublime (manual) | PR-based, post-release to each registry | n/a | n/a |
| Helix / Eclipse (docs-only) | config snippets (nothing to publish) | n/a | n/a |

### 2.3 What is already good (do not regress)

- **One tag fans out to every automated channel** via a single `needs:`-chained `release.yml`.
- **crates.io publishing is keyless (OIDC trusted publishing)** — no long-lived `CARGO_REGISTRY_TOKEN`.
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
  copy (a disclosed mutable-`main` caveat, `MP-N1`), not a stale static copy.

---

## 3. Findings register (harden)

Deduplicated from a two-repo audit (release machinery + documentation), then
independently fact-checked (every citation and structural claim verified against
the tree; external claims verified against primary sources). Severity is impact x
likelihood; `file:line` is evidence for re-verification, not exhaustive.

### 3.1 Machinery and pipeline (`MP-*`)

| ID | Sev | Finding | Evidence | Fix / phase |
|---|:--:|---|---|---|
| **MP-H1** | HIGH | No signing / SLSA provenance / SBOM / attestation on any binary artifact; `.sha256` proves transit only. | grep across `.github/`, `install.sh`, `Dockerfile`, `npm/`; v0.15.0 assets carry none; `release.yml:305-317` docker sets no attestation | §6 / P1 |
| **MP-H2** | HIGH | Official GitHub Action silently fails on Windows runners (composite → install.sh hard-exits) though a win-x64 binary exists. | `action.yml:56-130` (`bash "$install_sh"` at `:129`), `install.sh:32-38` | Windows path in the Action / P2; doc note / P0 |
| **MP-M1** | MED | Build matrix needlessly gates the irreversible crates.io publish (and docker). A flaky win/aarch64 leg blocks a publish needing zero binaries. | `release.yml:104` (`fail-fast:false`), `:329` (`publish-crates needs: build`) | `publish-crates` → `needs: preflight` / P0 (effect next tag) |
| **MP-M2** | MED | Four expiring-token channels, each a split-brain single point of failure, all run after the Release exists. | `NPM_TOKEN release.yml:375`; `VSCE_PAT/OVSX_PAT :475-476`; the four JetBrains secrets `:525-528`; `HOMEBREW_TAP_DEPLOY_KEY :420` | migrate npm to OIDC / P1; rotation runbook / P0 |
| **MP-M3** | MED | npm declares `win32/arm64` (os×cpu) it cannot serve; hard postinstall error instead of clean `EBADPLATFORM`. | `npm/package.json:31-32`, `npm/install.js:52-58` | fix declaration / P0; add win-arm64 build+package / P2 |
| **MP-L1** | LOW | install.sh resolves "latest" via the unauthenticated GitHub API (60 req/hr/IP); 403s on shared CI egress. | `install.sh:48` | doc `ALINT_VERSION` pin / P0 |
| **MP-L2** | LOW | `action-selftest` pins `v0.12.0` though its comment says track the previous minor (should be v0.14.0). | `action-selftest.yml:94` | bump each release / P0 |
| **MP-L3** | LOW | `cross` installed unpinned at release time (fresh unversioned build dep in the aarch64 hot path). | `release-binary.sh:34-40` | pin `--version` / P0 (effect next tag) |
| **MP-L4** | LOW | Four editor channels (Zed/Neovim/Emacs/Sublime) are source-only, manual, post-release; only VS Code + JetBrains are tag-gated. | `RELEASING.md:208-218` | automate what can be / P2 |
| **MP-L5** | LOW | The pre-commit hook compiles from source (`language: rust`) — the slowest install path, needs a full Rust toolchain. | `.pre-commit-hooks.yaml` | ship a prebuilt-binary hook, or a PyPI-wheel `language: python` hook once PyPI lands / P2-P3 |
| **MP-N1** | NOTE | install.sh exists in three independently-updatable copies (release-pinned; `main` served via raw + the alint.org 302; the Action fetches at the consumer's ref). The headline `curl` pulls from mutable `main`. | `install.sh` header, `release.yml:190`, `action.yml:116`, alint.org `_redirects:9` | pair with signing (§6) |
| **MP-N2** | NOTE | The `v0` major tag is force-moved each release, so `@v0` consumers auto-receive whatever it is repointed to — the same mutable-ref surface as `MP-N1`, one level up (a compromised account can repoint it; `@v0` also crosses 0.x breaking minors). | `release.yml` (`v0` force-move) | recommend SHA-pinning for security-sensitive consumers; note the auto-update tradeoff / §6 |

### 3.2 Version-pin blind spot (`VP-*`)

| ID | Sev | Finding | Evidence | Fix / phase |
|---|:--:|---|---|---|
| **VP-M1** | MED | Zed extension pinned at 0.13.0 (two minors stale), manual PR, **not in the pin gate** — so the gate falsely implies "all versions match." | `editors/zed/extension.toml:3`, `editors/zed/Cargo.toml:3`; `check-version-pins.sh:51-58` scope | add every editor manifest to `check-version-pins.sh` / P0 |

### 3.3 Documentation (`D-*`)

| ID | Sev | Finding | Evidence | Fix / phase |
|---|:--:|---|---|---|
| **D-H1** | HIGH | Copy-paste-broken `brew install alint` with **no tap** — errors "No available formula" on a clean machine (alint is tap-only). | alint.org `src/pages/migrating-from/ls-lint.astro:389` | qualify to `asamarts/alint/alint` / P0 |
| **D-H2** | HIGH | npm is a broken promise: the canonical install page has no npm section, yet the docs landing advertises "install via … npm …" and links there. | `docs/site/getting-started/installation.md` (no npm); `index.mdx:26,42` | add npm section (+ `about/index.md` links) / P0 |
| **D-M1** | MED | "install.sh (Linux + macOS + Windows tarballs)" heading implies `curl \| bash` covers Windows; it does not, and npm/cargo (which do) are not surfaced to Windows users there. | `installation.md:21`, `README.md:97` | reframe; surface Windows paths / P0 |
| **D-M2** | MED | Homebrew shown in two divergent (both-correct) forms across the most-visited surfaces. | `README.md:108-109` vs `agent-friendly-linter.astro:184`, `why-alint.md:214` | standardize on one / P0 |
| **D-L1** | LOW | Broken npm-README anchor (`#homebrew` vs `#homebrew-macos--linuxbrew`). | `npm/README.md:37` | fix anchor / P0 |
| **D-L2** | LOW | Stale + self-inconsistent Docker minor-tag examples (`:0.13` vs `:0.10`; current 0.15). | `README.md:161`, `installation.md:49` | refresh / de-hardcode / P0 |
| **D-L3** | LOW | Ancient npm version example `@asamarts/alint@0.5.11`. | `npm/README.md:26` | refresh / P0 |
| **D-L4** | LOW | Homepage snippet omits `cargo`. | alint.org `index.astro:35-46` | add or accept as teaser / P0 |
| **D-L5** | LOW | `about/index.md` project links omit npm + the GitHub Action (inconsistent with `SECURITY.md:43-51`). | `docs/site/about/index.md:30-36` | align / P0 |
| **D-L6** | LOW | RELEASING.md says editor-marketplace prereqs are "NOT yet done" though the jobs demonstrably publish. | `RELEASING.md:195-206` | update runbook / P0 |
| **D-N1** | NOTE | Local synced-docs show v0.14.2 — gitignored build artifacts re-synced from the tag at deploy; live site is correct. | `src/content/docs/docs/.../installation.md` | none (benign) |

### 3.4 Gaps (expected channels no surface documents)

`G1` no Windows package manager (winget/scoop/chocolatey); `G2` no `cargo binstall`
(though prebuilt tarballs + `.sha256` already exist); `G3` no Nix/AUR/apt/snap/
flatpak. All addressed in §4 (winget/scoop adopt; Chocolatey explicitly deferred to
Tier 3; the rest tiered).

### 3.5 npm packaging model

The **postinstall-download** model breaks under **`bunx`/Bun** (blocks lifecycle
scripts by default), **`npm install --ignore-scripts`** (common in hardened CI),
and **offline/air-gapped** installs. Migrate to the **optionalDependencies** model
(§4) and add the missing win-arm64 target — the packaging axis of `MP-M3`, scheduled
in P2.

---

## 4. Channel expansion

Effort: **S** = a file or docs line; **M** = a CI job / workflow + any account or
secret; **L** = a multi-week external or social process.

### Tier 1 — adopt now (best effort:reward)

| Channel | Effort | Rationale |
|---|:--:|---|
| **cargo-binstall** | S | Highest ROI, and cleaner than first thought. Assets named `alint-v{version}-{target}.tar.gz` hit binstall's default `{name}-v{version}-{target}` probe (one of ~10 built-in templates, no metadata needed); it finds the repo via the crates.io `repository` field, which alint already sets (`crates/alint/Cargo.toml:8` → `github.com/asamarts/alint`); it auto-appends the musl fallback on every Linux host (so musl-only resolves on glibc *and* musl); and the nested `alint-v{version}-{target}/` bin path is covered by default bin-dir probing. So `cargo binstall alint` very likely resolves today with no repo change — confirm once with `--dry-run`; only add `[package.metadata.binstall]` if that fails (and note that ships in a release, not a docs line). Then document it so Rust users stop compiling from source. |
| **In-repo `flake.nix`** | S | One file → `nix run github:asamarts/alint`, no external gatekeeper, rebuilds from the tag. Precedent: eza ships its own flake. |
| **GitHub Marketplace listing (the Action)** | S | The Action is already a live channel but is not *listed* on the GitHub Marketplace — a release-time metadata + checkbox step that adds real discoverability for free. |
| **podman (docs only)** | S | Not a channel: podman runs the existing OCI image unchanged. The one nuance is short-name resolution (podman does not implicitly prepend `docker.io/`), so document a **fully-qualified** `podman run ghcr.io/asamarts/alint check`. Rootless needs nothing from us. |
| **mise (`ubi`/`github` backend)** | S | `mise use ubi:asamarts/alint` resolves our releases live with no plugin or manifest (our conventional asset names already satisfy it); optional one-time registry PR for a bare shortname. Also covers asdf users, so no bespoke asdf plugin is needed. |
| **Third-party fetchers + `cargo install --git` (docs only)** | S | `eget asamarts/alint`, `ubi`, `bin` pull our existing releases with zero work from us — one docs line, and the honest answer for "`go install`" users (Tier 3). `cargo install --git` is the zero-infra Rust fallback for unreleased `main`. |
| **Scoop self-hosted bucket** | M | Self-updating on Windows: `checkver:github` + `autoupdate` + the Excavator action bump version and hash for us. Mirrors the Homebrew-tap pattern. |
| **WinGet (WinGet Releaser action)** | M | Largest native-Windows audience (ships in Win11). The action auto-opens the manifest PR each release; the only friction is Microsoft's merge review. |
| **`.deb` + `.rpm` on Releases** | M | `cargo-deb` and `cargo-generate-rpm` generate from `Cargo.toml` in CI and attach to the Release. **One musl-static package each** (we build no gnu binary — §2.1 — and a static binary installs fine on glibc hosts). Not an apt/dnf *repo* — just downloadable packages. |
| **npm → optionalDependencies** | **M–L** | The heaviest Tier-1 item (the ADR calls it a breaking internal change) and coupled to the win-arm64 build (P2). Adopt Biome's pure model (per-platform `@asamarts/alint-<target>` packages tagged with `os`/`cpu`, a thin `bin` shim, **no install script** — confirmed on Biome 2.x), or esbuild's hybrid (optionalDeps + a postinstall fallback for `--no-optional`). Caveat: OIDC trusted publishing cannot publish a package's *initial* version, so each new per-platform sub-package needs a one-time token bootstrap. Then `npx alint` and `bunx alint` both work. |
| **AUR `alint-bin`** | M | Arch users expect it; `github-actions-deploy-aur` pushes over SSH and regenerates `.SRCINFO`/checksums. We own it, fully CI-driven. |

Reconciling with what we already do: the **Homebrew tap is already auto-bumped**
(no `bump-formula-pr` wiring needed), and Docker is already on **GHCR** (the registry
without Docker Hub's anonymous pull throttle) — a Docker Hub *mirror* for `docker
run` short-name discoverability is optional, low-priority.

### Tier 2 — adopt later (gated on eligibility, demand, or consolidation)

| Channel | Effort | Rationale |
|---|:--:|---|
| **Homebrew core** | M | Tap-less `brew install alint` + free BrewTestBot autobump. The notability bar is `≥75 stars OR ≥30 forks OR ≥30 watchers` for a *third-party* packager — but a **self-submission (the repo owner opening the PR) triggers a 3× multiplier → `≥225 stars OR ≥90 forks OR ≥90 watchers`** (`shared_audits.rb`), plus a real 30-day-age gate and no-HEAD-only rule. alint is at 62 stars / 1 fork (2026-08-16), so self-submission is far off; the realistic path is to let a third-party packager submit at the 75-star bar. **Keep the tap either way.** |
| **nixpkgs** | M | Wide reach, mostly bot-maintained after entry — but do the in-repo flake first, and either use `importCargoLock`/`cargoLock` (no FOD hash) or wire a `passthru.updateScript` (`nix-update-script`), because r-ryantm cannot auto-bump Rust's `cargoHash`. |
| **conda-forge** | M | Cheap to keep once in (regro autotick bot bumps; we merge), but the audience skews data-science — worth the one-time staged-recipes PR on demand. |
| **Self-owned COPR** | M | Native `dnf install` UX for Fedora/RHEL via webhook auto-rebuild, no official-Fedora gatekeeper (precedent: bottom and ripgrep both have community COPRs). |
| **PyPI (maturin `bindings="bin"` wheels)** | M | Fully automatable and clean (how ruff and typos ship: per-platform wheels landing the binary as a PATH script via `maturin-action` + OIDC). **Only worth it for the Python-ecosystem wins** — `uvx alint`, `uv tool install`, `pipx run alint`, and pinnable **pre-commit** hooks (which would also remedy `MP-L5`). Biome deliberately skips PyPI; dprint, by contrast, *does* ship an official `dprint-py` for exactly this pre-commit/Python niche — mild evidence for the demand-gated play. Adopt if we want that reach. |
| **cargo-dist / `dist`** | M–L | A *consolidation* play: it would regenerate install.sh/npm/Homebrew/the release workflow and hand us binstall metadata + shell/PowerShell installers for free. Actively maintained (axodotdev, releases into 2026); the caveat is single-vendor bus-factor (the vendor sunset its hosted product once). Evaluate when hand-rolled upkeep starts to hurt. |
| **Snap** | M+ | A path-reading linter needs **classic** confinement → a one-time manual Canonical review. Modest incremental reach; low priority. |

### Tier 3 — skip (not worth it or structurally impossible)

| Channel | Why skip |
|---|---|
| **Go (`go install`)** | Structurally cannot distribute a non-Go binary — it fetches Go *source* and compiles it. Point users at the Tier-1 fetchers (`eget`/`ubi`/`bin`). |
| **Chocolatey** | Windows reach overlaps Scoop + WinGet (both adopted in Tier 1); the NuGet-package + community-moderation path is extra maintenance for little marginal audience. Revisit only on Windows-user demand. |
| **Flatpak / Flathub** | GUI/desktop-oriented; the sandbox fights a linter that reads arbitrary repo paths. No flagship-CLI precedent. |
| **Alpine aports** | The static musl binary already runs on Alpine; aports entry is maintainer-gated for little marginal value. |
| **Official Debian/Ubuntu, official Fedora** | Need an external Debian-Developer / Fedora-packager sponsor (weeks-to-months, out of our control). The self-owned COPR (Tier 2) covers the `dnf` UX without the gate; revisit only on real popularity. |
| **Bespoke asdf plugin** | Superseded by mise's `github`/`ubi` backend. |
| **pkgx, GNU Guix, Spack, webi, Homebrew `--HEAD`** | Niche or community-owned; none needs upstream action. Named here so the omission is deliberate, not overlooked. |
| **taiki-e/upload-rust-binary-action** | Not a channel — a build helper; its default naming is conveniently binstall-compatible, so it is an optional internal simplification of the release job, nothing more. |

---

## 5. Precedent (comparable Rust CLIs)

The compact evidence for the §1 frame. **SELF** = upstream-run (per-release CI);
**COMMUNITY** = distro/packager-run (out of upstream's hands).

| Tool | SELF (upstream owns) | COMMUNITY (delegated) |
|---|---|---|
| **ripgrep** (gold standard) | crates.io + GitHub binaries (incl. a CI-built `.deb`) | Homebrew-core, Debian, Arch, Fedora, nixpkgs, Scoop, WinGet, Chocolatey, MacPorts, BSD ports; snap disowned |
| **ruff** (astral) | curl installer + PyPI(maturin) + GHCR + GH binaries | Homebrew-core, conda-forge, Arch, Alpine. **No npm CLI** (only `@astral-sh/ruff-wasm-*` library packages) |
| **biome** | npm (optionalDependencies, no postinstall on 2.x) + standalone binaries + Docker | Homebrew-core. crates.io is a placeholder (`biome` = v0.0.0) |
| **uv** (astral) | curl/PS installer + PyPI + GHCR + GH binaries | Homebrew, Scoop, **WinGet**, Nix, pacman (WinGet is bot/community-maintained, not upstream) |
| **dprint** | curl installer + npm + crates.io (generic binstall heuristics) + GH binaries + an official `dprint-py` on PyPI | Homebrew, Scoop |
| **typos-cli** | crates.io + GH binaries + pre-commit/Action + PyPI wheels | Homebrew, conda, Arch. No npm |
| **bottom** | GH binaries incl. `.deb`/`.rpm` (glibc+musl) | AUR, nixpkgs, **COPR** (`atim/bottom`, community) |

Takeaways: (1) every one keeps SELF small; (2) the npm-CLI precedents are Biome and
esbuild, **not** ruff (ruff has no npm CLI package); (3) PyPI adopters all have
Python-developer users, but dprint's `dprint-py` shows the pre-commit niche can
justify it; (4) `.deb`/`.rpm`-on-Releases + a (usually community) COPR is the
common "native Linux UX without the distro gate" pattern.

---

## 6. Supply-chain hardening (the `MP-H1` plan)

The objective: a consumer can prove a downloaded alint artifact was built by this
repo's CI from this repo's source, using tooling that needs no long-lived secret.

**Honest threat model first.** Keyless (OIDC) attestation roots its trust in the
GitHub org/account that runs the workflow. It defends strongly against *post-publish*
tampering, malicious mirrors, and typosquats, and it makes any compromise
*attributable* — but it does **not** defend against the compromised-publishing-account
case §1 leads with, because that account is the root of trust (a compromised account
produces validly-attested malicious artifacts). The GitHub org therefore remains the
ultimate single point of failure; the mitigations are account hardening (2FA,
protected tags, minimal `id-token` scope) alongside the steps below. This is also why
"crates.io has provenance" is imprecise: crates.io has keyless *publishing* (no PAT),
not a consumer-verifiable *attestation* a user can check — precisely the gap this
section closes for binaries.

1. **Build provenance / attestation (SLSA).** Add `actions/attest-build-provenance`
   to `release.yml` for every Release tarball (`subject-path`) *and* the ghcr image
   (`subject-digest` + `push-to-registry`) → signed, OIDC-backed provenance,
   verifiable with `gh attestation verify <artifact> --repo asamarts/alint` (and
   `oci://…` for the image). Keyless; no secret to expire.

2. **Keyless signing (Sigstore / cosign).** `cosign sign-blob` the tarballs (and the
   aggregate `SHA256SUMS`) and `cosign sign` the ghcr image, via OIDC/Fulcio/Rekor →
   `cosign verify-blob` / `cosign verify`. Signing the single `SHA256SUMS` manifest is
   cheaper leverage than per-tarball signatures and covers every asset at once.

3. **npm provenance (near-free, pairs with the OIDC migration).** Once npm moves to
   optionalDependencies (native bytes in the sub-packages) + OIDC trusted publishing,
   `npm publish --provenance` (one flag + `id-token: write`) attaches Sigstore-backed,
   consumer-verifiable SLSA provenance on npmjs.com. Caveat: the initial version of
   each per-platform sub-package needs a one-time token bootstrap (OIDC cannot publish
   a first version).

4. **SBOM.** Build with `cargo auditable` (embeds the dep graph in a `.dep-v0` binary
   section, readable via `cargo audit bin`) and attach a CycloneDX/SPDX SBOM (via
   `cargo cyclonedx` or syft, or `actions/attest-sbom`) to the Release. The
   distroless-static image has near-zero OS surface, so we attest the *binary's* SBOM
   and skip image OS-package CVE scanning as low-value.

5. **Verification UX.** Teach `install.sh` and the npm shim to *optionally* verify
   provenance/signature when `gh` or `cosign` is present (never a hard dependency), and
   document `gh attestation verify` / `cosign verify` on the install page and in
   `SECURITY.md`. Pair with the mutable-ref caveats (`MP-N1`, `MP-N2`): signing plus an
   optionally tag-pinned installer and SHA-pinned `@vX.Y.Z` Action usage closes the loop.

This is the single highest-trust-ROI item in the plan and the one most aligned with
what alint claims to stand for.

---

## 7. Maintenance model (free vs ongoing)

Classifying every channel by who does the per-release work makes the sprawl risk
explicit. Prefer the top two bands.

- **Truly free — no per-release upstream work** (resolves/rebuilds from our
  existing releases, or a bot bumps it): cargo-binstall, in-repo `flake.nix`,
  podman, mise (`ubi`/`github`), the Action's `@v0` float, third-party fetchers,
  Scoop bucket with `checkver`/`autoupdate` + Excavator, and — once admitted —
  homebrew-core (BrewTestBot), conda-forge (autotick bot), nixpkgs (r-ryantm, with
  the `cargoHash`/updateScript caveat).
- **Semi — auto-PR authored, external human merges each release:** WinGet (the
  releaser action opens the PR; Microsoft reviews). PAT expiry is the classic
  failure mode.
- **Ongoing per-release but fully automated in our CI (team-owned):** npm (either
  model), `.deb`/`.rpm` attachment, AUR `alint-bin`, the Homebrew tap bump, COPR
  (webhook rebuild). Every tag, no human.
- **Manual / external gatekeeper (cannot self-drive entry):** official
  Debian/Ubuntu, official Fedora, Alpine aports — need a sponsor to admit; then the
  distro maintainer updates it. Today's manual editor channels
  (Zed/Neovim/Emacs/Sublime, `MP-L4`) and the source-compiling pre-commit hook
  (`MP-L5`) also live here until automated.

---

## 8. Answers to the specific questions asked

- **cargo-binstall — metadata or just naming?** Just naming, and we already satisfy
  it (§4 Tier 1): the v-prefixed default template matches our assets, the crates.io
  `repository` field is set, the musl fallback is automatic on every Linux host, and
  default bin-dir probing covers the nested archive path. `cargo binstall alint` very
  likely works today; confirm with `--dry-run`.
- **Nix — nixpkgs vs flake?** In-repo flake first (Tier 1: one file, no gatekeeper,
  instant `nix run`), nixpkgs later (Tier 2) for discoverability, using `cargoLock`/
  `importCargoLock` or a `passthru.updateScript` because the generic bot cannot bump
  `cargoHash`.
- **npm — is our approach best practice?** No — migrate. We are on the
  postinstall-download model (verified in-repo), which breaks under
  Bun/`--ignore-scripts`/offline. Best practice is optionalDependencies (Biome pure
  model on 2.x, or esbuild hybrid). Add win-arm64, and note OIDC can't publish a
  sub-package's first version (one-time bootstrap).
- **PyPI — worth it for a non-Python tool?** Technically excellent and fully
  automatable (maturin `bindings="bin"`, per-platform wheels, OIDC — how ruff and
  typos ship), **but** the payoff is Python-ecosystem reach: `uvx`/`uv tool
  install`/`pipx run` and pinnable pre-commit hooks. Biome skips PyPI; dprint ships
  `dprint-py` for exactly that niche. Adopt on demand.
- **Go (`go install`) — can it ship a Rust binary?** No, flatly — it fetches Go
  source and compiles. The only Go-adjacent option is third-party fetchers
  (`eget`/`ubi`/`bin`) that grab our existing releases; a one-line docs mention.
- **podman — separate channel or same image?** Same image, no separate channel,
  genuinely free. Docker and podman are both OCI; one artifact also serves
  containerd/nerdctl/Kubernetes. Only action: document a fully-qualified registry.
- **Homebrew — core vs tap?** Keep the tap now (full control, instant releases,
  already zero-touch). Homebrew core is far off for *self*-submission (the 3×
  owner-submission multiplier → 225 stars vs alint's 62); the realistic route is a
  third-party packager submitting at the 75-star bar. You can have both.

Research corrections folded into the above (so premises are not carried forward
wrong): ruff has **no** npm CLI package (it is a PyPI precedent); dprint **is** on
PyPI via the official `dprint-py` (only the unrelated squat is "not dprint"); `ubi`
is written in Rust (only `eget`/`bin` are Go); cargo-dist is **not** deprecated
(renamed `dist`, actively maintained), the real caveat is single-vendor bus-factor;
uv's WinGet and bottom's COPR are community-maintained, not upstream.

---

## 9. Phased plan

Each phase is independently shippable; nothing here changes rule semantics or
`.alint.yml`. Ordering is by ROI and by what unblocks what. Every §3 finding is
assigned below.

- **P0 — correctness + near-free wins (most need no release *to land*; the CI edits
  take effect and are first testable at the next `v*` tag).** Docs: `D-H1` broken
  brew, `D-H2` npm section + `D-L5` about links, `D-M1` Windows framing, `D-M2`
  homebrew form, `D-L1` anchor, `D-L2` docker tags, `D-L3` npm example, `D-L4`
  homepage cargo, `D-L6` RELEASING note. Pin gate: `VP-M1` add editor manifests.
  Pipeline: `MP-M1` decouple `publish-crates`, `MP-M3` fix the npm win-arm64
  declaration, `MP-L1` document `ALINT_VERSION`, `MP-L2` bump the selftest pin,
  `MP-L3` pin `cross`, `MP-M2` write the token-rotation runbook. Also a doc note for
  `MP-H2` (the Action is Linux/macOS-only today). **Document** cargo-binstall (`G2`),
  podman (fully-qualified), the third-party fetchers + `cargo install --git`, and
  **list the Action on the GitHub Marketplace**.

- **P1 — supply-chain hardening (next release).** §6: attestation + cosign (incl.
  signing `SHA256SUMS`) + SBOM + verification docs, **plus migrating npm to OIDC
  trusted publishing so `npm publish --provenance` rides along** (`MP-M1`/`MP-H1`/
  `MP-M2` token reduction). Addresses `MP-N1`/`MP-N2` via signing + SHA-pin guidance.
  Highest trust ROI; keyless, so no new secret debt.

- **P2 — owned high-reach channels (one pipeline pass over 1-2 releases).** Add the
  **win-arm64 build target first**, then npm → optionalDependencies that ships it
  (resolving the packaging axis of `MP-M3`); make the **Action work on Windows**
  (`MP-H2`); Scoop bucket; WinGet Releaser; `.deb`/`.rpm` (musl-static) on Releases;
  AUR `alint-bin`; in-repo `flake.nix`; mise registry entry; automate the editor
  channels where possible (`MP-L4`) and ship a prebuilt-binary pre-commit hook
  (`MP-L5`). Each is CI-automated and covered by the (expanded) version-pin gate.

- **P3 — eligibility / demand-gated.** Homebrew core (only once the star bar is
  actually met — 225 self-submission or a third-party submit at 75), nixpkgs (after
  the flake), conda-forge, self-owned COPR, PyPI wheels (which also give a
  `language: python` pre-commit hook), Snap, a cargo-dist consolidation evaluation,
  and — if a perf gap appears — a `mimalloc` feature to offset the musl allocator.

### Success criteria (per-phase definition of done)

- **P0:** the version-pin gate is green across *all* editor manifests; `cargo
  binstall alint --dry-run` resolves the release; the canonical install page lists
  every live channel including npm and a real Windows path; `publish-crates` no
  longer `needs: build`.
- **P1:** `gh attestation verify` and `cosign verify-blob` succeed against a released
  asset; `npm publish --provenance` shows a provenance badge on npmjs.com; an SBOM is
  attached and attested.
- **P2:** every *owned* channel (npm, Scoop, WinGet, deb/rpm, AUR, flake, mise) is
  produced by CI on the tag and is in the pin gate; a Windows-runner `asamarts/alint`
  Action step succeeds; a win-arm64 binary exists and its npm sub-package installs.
- **P3:** each entry lands only when its gate clears (homebrew-core at the real star
  bar; PyPI when we want the Python reach), so "done" is per-channel, on demand.

---

## 10. Risks and non-goals

- **Non-goal: own every distro package.** Debian/Arch/Fedora/nixpkgs/MacPorts are
  community-packager territory; we make their job easy (stable tags, conventional
  asset names, an SBOM) rather than duplicate it.
- **Risk: channel sprawl → maintenance drag.** Mitigation: every *owned* channel
  must be CI-automated and covered by the version-pin gate; prefer channels in the
  top two maintenance bands (§7).
- **Risk: the publishing account is the ultimate SPOF.** Attestation proves origin,
  not benevolence (§6); mitigate with account hardening, protected tags, and minimal
  OIDC scope, and offer SHA-pinning to security-sensitive consumers (`MP-N2`).
- **Risk: cargo-dist single-vendor bus-factor.** Treat consolidation as optional; do
  not couple the release to it without an exit plan.
- **Risk: PyPI positioning mismatch.** Shipping a language-agnostic tool on PyPI can
  confuse positioning; adopt only for the concrete `uvx`/pre-commit wins.
- **Accepted: the musl allocator tradeoff** (§2.1) — revisit with a `mimalloc`
  feature, not a gnu target, if perf-gating shows a gap.
- **Sequencing:** win-arm64's *build* precedes its npm *package* (P2), so no P3 item
  blocks a P2 item; the P0 CI edits are inert until the next tag by design.

---

## 11. Decisions

The channel tiering (§4) and the supply-chain posture (§6) are ratified in
[ADR-0015](../adr/0015-distribution-strategy.md). The audit findings (§3) are tracked
as follow-up work; P0 is mechanical and can land ahead of the ADR's acceptance.
