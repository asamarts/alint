---
status: proposed
date: 2026-08-16
decision-makers: asamarts
---

# 0015. Distribution channel tiering and supply-chain hardening

## Status

Proposed. Companion design doc:
[`docs/design/distribution.md`](../design/distribution.md), which carries the full
two-repo audit, the platform matrix, the per-channel effort/automation assessment,
and the phased plan. The decision below is the intended end-state; implementation
is phased (P0-P3 in the design doc) and each phase is independently shippable.

## Context

alint distributes through nine channels today, all auto-published on the `v*` tag,
and the story is solid but grew by accretion. A distribution and documentation
audit surfaced three things worth a durable decision rather than ad-hoc reaction:

- **Trust is asserted, not verifiable.** `SECURITY.md`'s threat framing puts
  supply-chain integrity for everyone who uses it at stake, but no binary artifact
  is verifiable: crates.io has keyless *publishing* (OIDC), which is not the same as
  a consumer-verifiable *attestation*. Release tarballs and the ghcr image carry no
  signature, SLSA provenance, or SBOM; the `.sha256` companions ship from the
  same release as the tarball, so they prove transit, not authenticity.
- **Reach has holes and unclaimed near-free wins.** Windows is thin (the official
  Action fails on Windows runners; no winget/scoop), and several channels that
  resolve or rebuild from our *existing* assets for almost no work (cargo-binstall,
  a Nix flake, Scoop, WinGet, mise) are unclaimed. The npm wrapper uses the
  fragile postinstall-download model.
- **No governing principle for what to adopt.** Without one, "add another package"
  requests get answered case by case, and channel sprawl accrues unbounded
  maintenance.

The decision driver is the precedent of mature single-binary Rust CLIs (ripgrep,
ruff, uv, Biome): upstream owns a *small* automated surface and lets community
packagers own the long tail. We adopt that model deliberately.

## Decision

We will treat distribution as a **tiered portfolio with a keyless-verifiable trust
chain**, not a maximal channel count. Concretely:

1. **Classify every channel as own / automate / delegate.** We *own and
   CI-automate* a small high-reach set; we *delegate* the long tail (Debian, Arch,
   Fedora, nixpkgs, MacPorts, BSD) to community packagers and help them with
   stable tags, conventional asset names, and an SBOM rather than duplicating
   their work. "Be everywhere ourselves" is explicitly rejected as a goal.

2. **Adopt the Tier-1 set now** (design doc section 4): cargo-binstall (document;
   verify naming), an in-repo `flake.nix`, listing the Action on the GitHub
   Marketplace, podman + third-party-fetcher docs, mise, a self-hosted Scoop bucket,
   WinGet via the releaser action, `.deb`/`.rpm` attached to Releases, an AUR
   `alint-bin`, and the **npm migration to the optionalDependencies model** (adding
   win-arm64). Each owned channel must be CI-automated *and* covered by the
   version-pin gate before it ships, and the primary install paths (`install.sh`,
   npm) must support an internal-mirror base-URL so air-gapped enterprises can
   install without github.com egress.

3. **Harden the supply chain to a keyless-verifiable standard** (design doc
   section 6): add SLSA build-provenance attestation and Sigstore/cosign signing
   (incl. the aggregate `SHA256SUMS`) for the Release tarballs and the ghcr image, and
   attach an SBOM to the Release binary (the byte-identical image inherits it), with
   optional best-effort verification in `install.sh` and the npm shim. The npm package
   gets consumer-verifiable provenance via `npm publish --provenance` — but **only when
   npmjs.com's trusted-publisher (OIDC) UI ships**, which is externally blocked today
   (per `release.yml`), so npm keeps its PAT until then. Extend the keyless OIDC
   posture crates.io already has; add no long-lived signing secret. This proves
   *origin*, not benevolence: a compromised publishing account remains the root of
   trust (see Consequences).

4. **Defer Tier-2 channels to eligibility or demand** and **skip Tier-3**:
   Homebrew core waits on the notability bar — and because a repo-owner self-submission
   triggers Homebrew's 3× multiplier (`≥225 stars` vs the third-party `75`, neither met
   at 62 today), the realistic path is a community packager submitting it once it
   crosses 75 (keep the tap regardless); nixpkgs, conda-forge, a self-owned COPR, PyPI
   (maturin `bindings="bin"` wheels, only for the `uvx`/pre-commit reach), and Snap
   (classic-confinement review) are demand-gated; cargo-dist is an optional future
   *consolidation*, not a dependency. `go install` (structurally impossible for a
   non-Go binary), Chocolatey (Scoop + WinGet already cover Windows), Flatpak, Alpine
   aports, official Debian/Fedora (sponsor-gated), a bespoke asdf plugin, and **OS
   code signing / notarization** (dominant paths escape both gatekeepers; no peer
   signs; certs are the long-lived-secret anti-pattern) are skipped.

5. **Every owned channel is keyless where the registry allows it.** Migrate npm to
   OIDC trusted publishing (retiring the `NPM_TOKEN` PAT) **once npmjs.com's
   trusted-publisher UI ships — it is broken today, so this is not on our timeline**,
   and prefer OIDC/keyless for any new channel, to shrink the expiring-token surface
   that today can produce a split-brain release.

This changes no rule behaviour and no `.alint.yml` semantics. It adds CI jobs,
package manifests, attestation/signing steps, and docs.

## Consequences

Easier:

- **Trust becomes verifiable.** `gh attestation verify` / `cosign verify` let any
  consumer (or a hardened CI) prove an artifact's *origin* keylessly — the biggest
  step toward what `SECURITY.md` frames, short of the account-compromise case below.
- **A decision rule replaces case-by-case debate.** New "package alint for X"
  requests resolve against the tiers: own it only if it is high-reach and
  CI-automatable, else point the requester at the community-packager path.
- **Windows and Rust-user reach improve** via winget/scoop and a documented
  `cargo binstall alint` that stops needless from-source compiles.
- **The near-free channels cost almost nothing ongoing** because they resolve from
  assets we already publish or are bot-bumped.

Harder, and accepted:

- **The release pipeline grows** attestation, signing, SBOM, and several publish
  jobs — more surface to keep green, and the version-pin gate must expand to cover
  the new manifests (including the editor manifests it misses today).
- **Owned Windows/Linux packaging is real per-release CI** even when automated;
  each channel is another thing that can fail mid-release.
- **The npm migration is a breaking internal change** to how the package installs
  (optionalDependencies vs postinstall), requiring per-platform sub-packages and
  careful testing under `bunx`/`--ignore-scripts`/offline.
- **Deliberately unclaimed channels** mean some users find only community packages
  we do not control; we accept that in exchange for bounded maintenance.
- **Attestation does not defend against account compromise.** OIDC roots trust in the
  GitHub org that runs the release; a compromised account produces validly-attested
  artifacts, and the force-moved `v0` tag is repointable. The org stays the ultimate
  SPOF; we mitigate with account hardening, protected tags, minimal `id-token` scope,
  and SHA-pinning guidance — not with attestation alone.
- **The npm migration has a bootstrap cost:** OIDC cannot publish a package's first
  version, so each new per-platform sub-package needs a one-time token to seed it, and
  the win-arm64 build target must exist before its npm sub-package can ship.

## Considered Options

- **Tiered portfolio (chosen)** vs **maximal presence** (own every channel) vs
  **minimal** (crates.io + GitHub Releases only, ripgrep-strict). Maximal is
  unbounded maintenance for a pre-1.0 tool; minimal under-serves Windows and
  non-Rust users and forgoes near-free wins. The tiered model captures the cheap
  high-reach channels while delegating the tail.
- **Keyless attestation + cosign (chosen)** vs **GPG/minisign detached
  signatures** vs **checksums only (status quo)**. GPG/minisign reintroduce a
  long-lived key to store, rotate, and trust-distribute; checksums prove only
  transit. Sigstore/OIDC matches the crates.io posture and adds no secret.
- **npm optionalDependencies (chosen)** vs the **postinstall-download status quo**.
  The status quo breaks under Bun, `--ignore-scripts`, and offline installs; the
  optionalDependencies model (Biome/esbuild precedent) survives all three.
- **cargo-dist deferred as optional consolidation (chosen)** vs **adopting it now**.
  Adopting now would regenerate working plumbing and add single-vendor bus-factor
  (the vendor has already sunset a paid product once); we revisit only when
  hand-rolled upkeep hurts.
- **PyPI as a demand-gated integration (chosen)** vs **as a primary channel** vs
  **never**. A language-agnostic tool is not a natural PyPI resident (Biome and
  dprint skip it), but the `uvx`/`uv tool`/`pipx`/pre-commit wins are real for
  Python and mixed monorepos, so we keep it available on demand rather than
  foreclosing it.

## More Information

- Design doc with the full audit, platform x channel matrix, per-channel
  effort/automation/precedent assessment, supply-chain plan, and phased rollout:
  [`docs/design/distribution.md`](../design/distribution.md).
- Operational release runbook the automation extends:
  [`RELEASING.md`](../../RELEASING.md).
- Related: ADR-0004 (extends trust boundary and path confinement) is the same
  supply-chain-integrity value applied to config; this applies it to binaries.
- Key anchors: `.github/workflows/release.yml` (the fan-out), `install.sh`,
  `npm/install.js` (postinstall model to migrate), `Dockerfile` (distroless image
  to attest), `ci/scripts/update-homebrew-formula.sh` (the zero-touch tap
  pattern to mirror for Scoop), `ci/scripts/check-version-pins.sh` (the pin gate
  to extend to editor manifests).
