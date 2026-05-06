---
destination: README.md (top-level alint repo)
status: drafting
blocks_on: v0.9.15 release (for the version-refs bump from v0.9.14 → v0.9.15)
last_touched: 2026-05-06
---

# README hero refresh — incremental polish + case-study social proof

## What this draft does

The current README hero (rewritten in P1, commit `52e7494f`) is already
strong. This draft does NOT rewrite it. It adds two new sections:

1. **"Proven on real OSS repos"** — between the hero bullets and the
   60-second quickstart. Name-drops the 20 P2a case studies + links to
   `examples/README.md`. Anchors the launch claim that alint isn't
   theoretical.
2. **"Where alint shines"** — after "Core capabilities". Surfaces the 5
   positioning narratives crystallised across P2a so a first-time reader
   can self-identify which one matches their repo.

Version refs (`v0.9.14` in 4 places) bump to `v0.9.15` when that
version ships. That's a mechanical post-release pass, not a content
change — left out of this draft.

## What changes

### 1. New section: "Proven on real OSS repos"

Insert directly after the existing 4 hero bullets, before the
"alint fills the active-maintenance gap..." paragraph.

```markdown
## Proven on 20 real OSS repos

alint configs covering the structural-validation surfaces of:

**Single-language workspaces** —
[kubernetes](examples/kubernetes-kubernetes/),
[rust-lang/rust](examples/rust-lang-rust/),
[golang/go](examples/golang-go/),
[python/cpython](examples/python-cpython/),
[nodejs/node](examples/nodejs-node/),
[apache/airflow](examples/apache-airflow/),
[denoland/deno](examples/denoland-deno/),
[tokio-rs/tokio](examples/tokio-rs-tokio/),
[astral-sh/uv](examples/astral-sh-uv/),
[astral-sh/ruff](examples/astral-sh-ruff/),
[clap-rs/clap](examples/clap-rs-clap/),
[microsoft/typescript](examples/microsoft-typescript/),
[facebook/react](examples/facebook-react/),
[prettier/prettier](examples/prettier-prettier/),
[pnpm/pnpm](examples/pnpm-pnpm/),
[helm/helm](examples/helm-helm/),
[pytorch/pytorch](examples/pytorch-pytorch/),
[vercel/turbo](examples/vercel-turbo/).

**Polyglot monorepos** —
[apache/arrow](examples/apache-arrow/) (6 languages: C++/Java/Python/Rust/Go/JS),
[vercel/next.js](examples/vercel-next.js/) (TS + Rust hybrid).

Each case study includes a working `.alint.yml` you can copy as a
starting point + a markdown writeup explaining what alint catches that
the repo's existing tooling misses. See [`examples/`](examples/) for the
full gallery.
```

### 2. New section: "Where alint shines"

Insert after "Core capabilities" and before "Non-goals" (keeping the
"what alint isn't" framing right after "where it shines" preserves the
reader's mental model: capabilities → fit → boundaries).

```markdown
## Where alint shines

alint isn't trying to be everything to everyone. The validation pass
across 20 OSS repos surfaced five distinct shapes of project where
alint earns its keep:

1. **Repos with verify-script sprawl.** *"Replaces the structural
   subset of N hand-rolled validation scripts."* Best fit: kubernetes
   (50 verify scripts → 17 declarative rules), apache/airflow (109
   pre-commit hooks → ~40 % map cleanly), python/cpython (12
   validation surfaces consolidated into 1 alint config).
2. **Repos that rely on convention without explicit checks.**
   *"Catches the conventions your pipeline assumes but doesn't
   verify."* Best fit: tokio (zero hand-rolled scripts; alint catches
   15 conventions tokio's pipeline silently assumes), uv (67-crate
   workspace conventions enforced nowhere in CI today), pnpm (replaces
   the in-tree `meta-updater` plugin), facebook/react, nodejs/node.
3. **Repos with mature tooling that lacks a structural layer.**
   *"Adds a structural floor on top of mature tooling."* Best fit:
   microsoft/typescript (eslint + dprint + knip already tight),
   astral-sh/ruff (900+ Python lint rules but zero rules for ruff's
   own internal-crate `publish = false` discipline), prettier, helm.
4. **Repos that built their own lint-orchestration tool.** *"Replaces
   the structural subset of your custom orchestration layer."* Best
   fit: pytorch (≈86 % of pytorch's 57 `lintrunner.toml` adapters are
   structural; alint sits beneath, lintrunner keeps the AST-aware
   tail).
5. **Tightly-curated minimal-tooling projects.** *"Encodes
   conventions enforced only by code-review discipline."* Best fit:
   golang/go (zero `.github/workflows/`, zero `Makefile`, zero
   `.golangci.yml`; the 31-rule alint config encodes the project's
   structural contract for the first time anywhere).

If your repo doesn't match one of these five — alint is probably
still useful (the rule catalogue is broad), but you may want to
start by reading the closest case study above to see what a working
config looks like in your shape.
```

## What stays the same

- Hero one-liner: *"Fast, language-agnostic linter for repository
  structure, files, and content."*
- All 4 hero bullets (speed / agent-aware / extensible / one binary).
- Repolinter-archived-2026 framing paragraph.
- 60-second quickstart YAML block.
- Capability list, non-goals, install instructions, all subsequent
  sections.
- All version refs at v0.9.14 (will bump to v0.9.15 in a separate
  mechanical post-release pass).

## Open questions before publish

1. The current 4-bullet hero already says *"60 rule kinds across 13
   families, 19 bundled ecosystem rulesets"*. After v0.9.15 those
   counts may shift (rule kinds + 1 if Phase 7 ships `*_path_contains`
   in the same window; rulesets unchanged). Verify counts at
   publish time.
2. The "Proven on 20 real OSS repos" line uses commas inside the
   `[link](...)` markdown — render OK on GitHub. Should also OK on
   alint.org's docs build (Starlight) — verify when the parallel
   alint.org draft lands.
3. The `lintrunner` reference in narrative #4 currently doesn't link
   anywhere. Could link `https://github.com/suo/lintrunner` —
   confirm at publish time it's still the canonical repo.

## Pre-publish checklist

- [ ] v0.9.15 has shipped + the 4 `v0.9.14` references in the live
      README updated to `v0.9.15`.
- [ ] Each `examples/<owner>-<repo>/` link tested (the directories
      exist; the relative paths from `README.md` resolve).
- [ ] `examples/README.md` gallery up to date with the 20-case-study
      list.
- [ ] alint.org draft (`alint-org-hero.md`) ready so the messaging
      matches across surfaces at publish time.
- [ ] STATE.md row for `README.md` flipped from `live` to `live (just
      refreshed)` with date + commit SHA.

## Estimated diff size

~50-70 lines added (one section after the bullets, one after Core
capabilities). Zero lines removed. Strictly additive — easy to
review, easy to revert if needed.
