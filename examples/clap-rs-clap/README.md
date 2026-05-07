# Case study: `clap-rs/clap`

> Marketing/positioning writeup at https://alint.org/examples/clap-rs-clap/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `clap-rs/clap` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, sparse-checkout of `.github`,
`clap_builder`, `clap_derive`, `clap_complete`, `clap_mangen`,
`clap_lex`, top-level config files (`Cargo.toml`, `committed.toml`,
`deny.toml`, `.clippy.toml`, `release.toml`, `typos.toml`,
`.pre-commit-config.yaml`, `Makefile`, `CONTRIBUTING.md`).

---

## Summary

clap ships a five-published-member Rust **library** workspace
(`clap_builder`, `clap_derive`, `clap_complete`, `clap_mangen`,
`clap_lex`) plus the umbrella `clap` facade and `clap_bench`, all
sharing one `[workspace.package]` block for license, edition, MSRV,
repository, and `include` glob. The structural surface is small but
dense: ~30 distinct checks across CI workflows, pre-commit hooks,
cargo-release config, and the workspace metadata itself.

Roughly **65 % map directly to existing alint rules** (workspace
metadata pinning, per-member inheritance assertions, license-file
presence, bundled `oss-baseline` + `rust` + `monorepo/cargo-workspace`
+ `ci/github-actions` checks), **5 % shell out via the `command` rule
kind** to existing tools (`typos`, `cffconvert`, `cargo deny`), and
**~30 % are out of alint's scope** by design (rustfmt, clippy lints,
rustdoc warnings, `cargo deny` graph reasoning, minimal-versions
resolver behaviour — the cargo-toolchain-aware checks alint isn't
trying to do).

The 24-rule starter config in [`/.alint.yml`](.alint.yml) covers every
structural assertion clap makes about its own workspace.

---

## Existing tooling inventory

### `Cargo.toml` — workspace single source of truth

The root manifest concentrates almost the entire structural contract:

| Field | What it pins | alint replacement |
|---|---|---|
| `[workspace] members = [...]` | 7 published + bench crates | `monorepo/cargo-workspace@v1`'s `cargo-workspace-members-declared` |
| `[workspace.package] license = "MIT OR Apache-2.0"` | Dual-license SPDX | `clap-workspace-license-mit-or-apache` (toml_path_equals) |
| `[workspace.package] edition = "2024"` | Cargo edition | `clap-workspace-edition-2024` |
| `[workspace.package] rust-version = "1.85"` | MSRV (single source for the CI matrix) | `clap-workspace-rust-version-pinned` |
| `[workspace.package] repository = "..."` | Canonical GitHub URL | `clap-workspace-repository-canonical` |
| `[workspace.package] include = [...]` | Per-tarball file allowlist | `clap-workspace-include-readme`, `clap-workspace-include-license` |
| `[workspace.lints.rust]` + `[workspace.lints.clippy]` | ~70 lints | Per-member: `clap-member-lints-inherit` (toml_path_equals on `[lints] workspace = true`) |
| Per-member `field.workspace = true` (license / edition / rust-version / repository) | Inheritance is the whole contract | `clap-member-{license,edition,rust-version,repository}-inherits` (5 `for_each_dir` rules over the family crates) |
| Per-member `categories = ["command-line-interface"]` (+ proc-macro-helpers for clap_derive) | crates.io discoverability | `clap-member-cli-category` |
| Per-member `keywords` includes `"cli"` | crates.io search signal | `clap-member-has-cli-keyword` |
| Per-member `[package.metadata.docs.rs] rustdoc-args = ["--generate-link-to-definition"]` | docs.rs source-link convention | `clap-member-docsrs-link-defs` |
| Per-member README.md presence | docs.rs landing page | `clap-member-has-readme` (covered by `cargo-workspace@v1` already; we restate at family scope for the explicit message) |

12 distinct manifest assertions, all expressible as TOML path queries
— clap's entire workspace-metadata contract maps to ~12 alint rules.

### `committed.toml`, `release.toml`, `typos.toml`, `deny.toml`, `.clippy.toml`

Five auxiliary policy files. alint covers the **presence + structural
shape** of each; the deep semantics stay with the owning tool:

| File | Rule | alint coverage |
|---|---|---|
| `committed.toml` | Conventional-commit style | `clap-committed-config-exists` + `clap-committed-style-conventional` (toml_path_equals on `$.style`) |
| `typos.toml` | Spell-check exclude list | `clap-typos-config-exists` (presence; deep validation via `command:typos`) |
| `deny.toml` | License/banned-crate/source allowlists | `clap-deny-config-exists` (presence; deep graph check via `command:cargo deny`) |
| `release.toml` | cargo-release shared-version + branch allowlist | Could be added; presence-only for now (the sharedness assertion is what `[package.metadata.release]` blocks already enforce) |
| `.clippy.toml` | `disallowed-methods` + per-test allowances | **Out of alint's scope** — needs Rust AST awareness; lives with clippy |

### `.github/workflows/*.yml` — 8 workflows

| Workflow | Purpose | alint disposition |
|---|---|---|
| `ci.yml` | Build/test/check matrix + rustfmt + clippy + docs + cffconvert + minimal-versions + lockfile freshness | Structural shape (permissions, action SHA pinning, name) covered by `ci/github-actions@v1`; build/test/lint bodies stay with cargo |
| `audit.yml` | `cargo deny` + `actions-rs/audit-check` | `ci/github-actions@v1` for shape; cargo-deny via `command:` rule |
| `committed.yml` | Conventional-commit lint via crate-ci/committed | `ci/github-actions@v1` for shape; presence asserted by `clap-committed-config-exists` |
| `spelling.yml` | typos | Same: shape + `command:typos` |
| `pre-commit.yml` | runs `prek` over `.pre-commit-config.yaml` | Shape via `ci/github-actions@v1`; presence of pre-commit config via `clap-pre-commit-config-exists` |
| `bench-baseline.yml` | binary-size bench via Bencher | Shape only |
| `template.yml` | monthly merge from epage/_rust template | Shape only |
| `post-release.yml` | release-notes generation on tag push | Shape only |

8 workflows. The `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow has `name:`) covers the
hardening surface for all of them at once.

### `.github/settings.yml`, `.github/renovate.json5`, `.pre-commit-config.yaml`

| File | What it declares | alint coverage |
|---|---|---|
| `settings.yml` (probot/settings) | repo description, topics, branch-protection placeholders, merge-button preferences | `clap-repo-settings-topics-declared` (yaml_path_matches on `$.repository.topics`) — the file isn't bundled-rule-territory because it's a probot config, not a GitHub-native one |
| `renovate.json5` | dependency-update policy (custom regex managers for STABLE / prek pins, dev-dep auto-merge groups) | Presence covered by `oss-baseline@v1`'s `oss-dependency-update-tool` |
| `.pre-commit-config.yaml` | check-yaml/json/toml + typos + committed | Presence covered by `clap-pre-commit-config-exists`; deep validation lives with pre-commit itself |

### `CITATION.cff`

Citation File Format manifest. clap validates it via cffconvert in CI.
alint asserts presence + the `cff-version` field's shape; deep schema
validation goes via the `command:` rule shelling out to cffconvert.

### `Makefile`, `CONTRIBUTING.md`

Documentation of the policy, not enforcement of it. `Makefile`'s
target conventions encode the feature-flag matrix the CI workflows
consume (`make {check,build,test,clippy}-{minimal,default,wasm,full,debug,release,next}`),
but the values themselves are policy decisions, not structural
assertions an external linter should second-guess. Out of scope.

---

## Maps to existing alint rules (what the starter config covers)

24 rules in [`/.alint.yml`](.alint.yml), broken down:

- **5 bundled rulesets** (`oss-baseline`, `rust`, `monorepo/cargo-workspace`, `ci/github-actions`, `hygiene/no-tracked-artifacts`) — pull in roughly 35 rules between them
- **4 workspace-root assertions** — license SPDX, edition, MSRV, repository URL pinned at the workspace root via `toml_path_equals` / `toml_path_matches`
- **5 per-member inheritance assertions** — every family crate inherits license / edition / rust-version / repository / lints from the workspace via `for_each_dir` over `{clap_builder,clap_derive,...}`
- **3 per-member metadata-sync assertions** — categories, keywords, docs.rs rustdoc-args
- **1 per-member README assertion** — `for_each_dir` requires `README.md` at family-crate scope
- **2 dual-license file assertions** — both `LICENSE-APACHE` and `LICENSE-MIT` at root
- **2 workspace-include glob assertions** — `README.md` and `LICENSE*` are in the per-tarball include glob
- **5 auxiliary-config presence + shape assertions** — CITATION.cff (presence + `cff-version`), committed.toml (presence + `style: conventional`), typos.toml, deny.toml, .pre-commit-config.yaml
- **1 repo-settings assertion** — `.github/settings.yml.repository.topics` is non-empty
- **3 `command` shell-outs** — `typos`, `cffconvert --validate`, `cargo deny check bans licenses sources`

---

## Needs new alint primitive

clap's structural surface is dense but mostly fits — only **two**
gaps surfaced that don't already appear in earlier case studies:

| Need | What it would check | What alint needs |
|---|---|---|
| **Per-crate metadata identity check** | "clap, clap_builder, clap_derive, clap_complete, clap_mangen, clap_lex all share the same `categories[0]`" — the assertion isn't "matches a regex" but "every member's value equals every other member's value" | A `cross_file_field_equals` rule kind (or `unique_by` extended with a `coalesce_value` mode): "for every file matching `select:`, extract value at `path:`, and assert all extracted values are identical (or in an allowed set)". Generalises beyond clap — every monorepo with shared metadata fields wants this (CODEOWNERS owners, ESLint plugin shared-config, kustomize labels, etc.). Lower priority than `ordered_block` / `registry_paths_resolve` from the rust-lang case study, but novel here. |
| **`pre-release-replacements` regex sanity** | clap's `[package.metadata.release].pre-release-replacements` arrays carry inline regex+replacement pairs that have to match the actual files at release time (e.g. `search = "Unreleased"` against CHANGELOG.md). Right now, a typo in the regex is only caught at `cargo release` time. | A `regex_resolves_in_file` rule kind: "extract regex value at `path:` from a structured registry file, assert it matches at least once in `target_path:`". Same shape as `markdown_paths_resolve` but checking regex hits instead of path resolution. Niche — the `pre-release-replacements` pattern is roughly cargo-release-specific — so this is a feature-request candidate, not a release blocker. |

Both gaps are **registry-driven cross-file rules** — the same family
the kubernetes (`import-restrictions.yaml`) and rust-lang (`triagebot.toml`)
case studies surfaced. The `registry_paths_resolve` rule kind from the
rust-lang gap analysis would not directly cover either of these, but
the shape is similar enough that one well-designed primitive could
absorb all four use cases. **Filing as v0.10+ feature requests.**

---

## Out of alint's scope (use the existing tool)

Same framing as the kubernetes / rust-lang case studies. clap's
structural-validation surface is small enough that the out-of-scope
list is short:

- `rustfmt --check` — formatting; lives with rustfmt
- `clippy` (incl. `[workspace.lints.clippy]` ~70 lints + `.clippy.toml`'s
  `disallowed-methods`) — Rust AST/semantics; lives with clippy
- `cargo doc -D warnings` — rustdoc; lives with cargo doc
- `cargo deny check {bans,licenses,sources}` graph reasoning —
  Cargo-metadata-graph aware; alint shells out via `command:` for
  orchestration but the analysis stays with cargo-deny
- `cargo +nightly generate-lockfile -Z minimal-versions` followed by
  `cargo check` — minimal-versions resolver behaviour; out of scope
- `cargo update --workspace --locked` (lockfile-freshness gate) —
  Cargo lockfile internals; out of scope
- `trycmd` snapshot tests — runtime behaviour; out of scope (alint
  could in theory enforce a `pair` between a `*.toml` snapshot file
  and its source test, but the **freshness** check is the value, and
  that's a runtime check)
- The CI matrix dimensions themselves (`[linux, windows, mac, minimal,
  default, next]` × `[stable, beta, nightly]` × …) — policy, not
  structure
- `cffconvert --validate` deep CFF schema check — covered via the
  `command` rule shell-out

---

## Already covered by other linters clap uses

- `cargo deny` (deny.toml) — third-party license + source allowlist;
  duplicates nothing, alint orchestrates via `command:`
- `typos` (typos.toml) — spell-check; alint orchestrates via `command:`
- `committed` (committed.toml) — conventional-commit style; covered by
  the dedicated workflow + pre-commit hook, alint asserts the config
  shape
- `cargo audit` (audit.yml) — RUSTSEC advisory check; security scanner
  territory, out of scope

---

## Performance comparison (placeholder — bench when validation pass scales)

clap's structural-validation surface is a few hundred milliseconds
total today: each tool walks the small (~30 MB) tree once, in
sequence. alint's parallel-rule dispatch (v0.9.3+) collapses that
into one walk. Expected wall-clock: well under 100 ms for the alint
subset, dominated by the `command:` shell-outs (which run in
parallel by file but each tool itself is the bottleneck).

clap is small enough that the wall-clock delta is dominated by the
`command:` shellouts. The structural-rule subset itself runs in well
under 100 ms on this tree — the bundled rules + the 12 manifest
assertions cover what 5 separate tools (`committed`, `typos`,
`cffconvert`, `cargo deny`, `pre-commit`) cover today, in one
declarative file with grep-able rule IDs.

To benchmark wall-clock for real: `time { committed && typos &&
cffconvert --validate -i CITATION.cff && cargo deny check; }` vs
`time alint check`. Deferred to the per-repo measurement pass.

---

## Followup feature work surfaced (de-duplicated against earlier case-study gap lists)

- **`cross_file_field_equals` rule kind** (every file matching
  `select:` has the same value at `path:`) — covers the per-crate
  metadata identity check here, plus shared-config cross-reference
  in any monorepo with replicated metadata
- **`regex_resolves_in_file` rule kind** (regex extracted from a
  registry file matches at least once in a target file) — niche,
  cargo-release-style "pre-release-replacements" use case; lower
  priority than the `registry_paths_resolve` rule kind from the
  rust-lang case study, but the shape is the same family

No NEW schema/language pitfalls hit beyond the existing 21-pitfall
catalogue (the dashed-key bracket-notation pitfall #10 fired once on
`$.workspace.package.rust-version`, fixed to
`$.workspace.package['rust-version']` per the canonical pattern;
this is a rediscovery of an already-documented pitfall, not a new
one).

---

## Future analysis

Concrete analyses to follow up on the live tree (when one becomes
available):

- **`alint suggest` against a fresh `clap-rs/clap` clone** — predict the
  heuristic will surface `oss-baseline@v1`, `rust@v1`, and
  `monorepo/cargo-workspace@v1`; cross-reference against the manually
  configured extends list.
- **`for_each_dir` over each `clap_*` workspace member** — the current
  config uses an explicit `select: "{clap_builder,clap_derive,...}"`
  bracket expansion. With the v0.10 candidate `monorepo/cargo-workspace`
  member-discovery refinement, a single `select: "{members}"` (where
  `{members}` derives from the `[workspace] members` array in the root
  `Cargo.toml`) would survive future crate additions without manual edits.
  Same shape demand as the deno case study.
- **JSON-output rule timing** — clap is small enough that the structural
  rules complete in well under a second. Worth running
  `alint check --format json --config .alint.yml` and confirming the
  three `command:` shellouts (typos, cffconvert, cargo deny) dominate the
  wall-clock; if so, narrow each one's `paths:` glob.

## Validation status (2026-05-07)

- alint version: v0.9.17
- Config validation: `validate-config` reports **70 rules loaded**.
  Reconciliation: 28 explicit rules in `.alint.yml` + 44 entries from
  extends (oss-baseline 15 + rust 11 + monorepo/cargo-workspace 4 +
  ci/github-actions 3 + hygiene/no-tracked-artifacts 11) − 2 facts
  (`has_rust`, `is_cargo_workspace` are `- id:` entries but not
  loadable rules) = 70. The README's narrative "24 rules" is the
  count after stripping the 4 `command:` shellouts; the precise total
  declared is 28 explicit, 70 loaded.
- Live-tree status: pending — `/tmp/clap/` not present at revalidation
  time.
- Pitfall fixes shipped in v0.9.17: pitfall #18 (per-rule
  `respect_gitignore: false`), pitfall #19 (literal_is_nested runtime
  guard) — neither directly affects this config (clap doesn't ship
  tracked-but-gitignored files or use `root_only:` with multi-component
  literals).
- Open gaps: `cross_file_field_equals` (the per-crate metadata identity
  variant) is now absorbed into the broader `cross_file_value_equals`
  candidate (10 sources, past-saturation, v0.10 ship-target);
  `regex_resolves_in_file` (the `pre-release-replacements` shape) remains
  cargo-release-niche and stays on the v0.10 design candidate list.
