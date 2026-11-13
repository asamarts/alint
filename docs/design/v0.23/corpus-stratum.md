# v0.23 corpus stratum: i18n and JavaScript monorepos

Status: complete (2026-11-12; reviewed 2026-11-13). Frame approved 2026-11-12; all four batches
done. Phase 2 of
[`README.md`](README.md); the synthesis is in [`general-linter.md`](../general-linter.md)
section 12.4.

Results: [`corpus-stratum-log.md`](corpus-stratum-log.md).

## 1. Why

The v0.12 case study's ranked backlog has no locale, key-parity or dependency-version findings
([`general-linter.md`](../general-linter.md) section 12.3). The three flagship Phase 3 policies
(locale key parity, dependency version consistency and toolchain-pin consistency) rest on outside
tools and an older examples set. This stratum measures the demand before the Phase 3 design
commits to assertion families. It adds two kinds of repository the 110-repository corpus lacks:
- repositories with many locale files under an enforced gate;
- JavaScript workspaces chosen for an enforced dependency-version policy.

## 2. Method

The study runs under the v0.12 protocol
([`case_study_100_repos.md`](../v0.12/case_study_100_repos.md)):
- **The unit:** one enforced validation behavior.
- **The where-to-look superset** and the open "what is unusual here?" scan.
- **Stage A:** a deep read that catalogues, classifies and drafts a configuration.
- **Stage B:** a second agent that adversarially verifies the catalogue's completeness.
- **Stage C:** a fix-up pass that applies Stage B's verified configuration issues and missed
  units to the configuration, then re-validates it.
- **Integration:** `alint validate-config` and `alint check` against the pinned clone.
- **Audit:** after the four batches, a review of every classification against the rulings below
  and the clones (log, "Audit").
- **Review:** after the audit, a check of the records against this frame's rulings, the
  configurations' comments and the published figures (log, "Review").

Calibration rulings R1 to R6 ([`case_study_calibration.md`](../v0.12/case_study_calibration.md))
apply unchanged, except where R7 below narrows R2. The configurations join the pinned corpus in
[`../v0.12/corpus/`](../v0.12/corpus/), with rows in `pins.tsv`, so `xtask corpus-replay`
replays them with the rest.

### 2.1 Ruling R7: a tool-run is classified by its logic

R2 classifies every tool-run as D. Applied to this stratum, that would put `i18n-tasks missing`,
`syncpack list-mismatches`, `yarn constraints` and Android lint's `MissingTranslation` in D. The
stratum would then show no demand for the flagship policies, by construction. R7 narrows R2:

- **A check a tool runs over files alint reads is classified by what it asserts.** Each assertion
  the tool enforces in this repository is a unit (R1 still collapses instances). A tool with
  several checks, such as i18n-tasks or a `yarn.config.cjs` with several rules, gives several
  units.
- **What it asserts decides the bucket.**
  - **A:** a kind or bundle expresses it, and `alint check` confirms it against the clone (R4).
  - **B or C:** otherwise, per section 2.2.
- **D** remains for checks that need:
  - code analysis: extracting the strings or imports a program uses, type checks, formatting,
    linting source;
  - running the project's code, build or tests;
  - package-manager resolution semantics (`yarn dedupe --check`, a lockfile's freshness);
  - network or external state.
- **R2 still applies** to code-analysis tools: formatters, linters, type checkers, test runners.

Examples:
- **i18n-tasks:** missing keys is A when each locale is one file (`key_parity`, YAML). forem
  merges each locale from many files, so there it is B-config `cardinality` (forem-01). Where a
  repository enables them, unused keys is D (source extraction) and normalized key order is
  B-adapter (no order operator); forem disables both (`xit`).
- **`syncpack list-mismatches`:** "one version per dependency across every `package.json`" is
  B-adapter with `needs: entry-values` and assertion family `unique-key` (the batch 2 ruling
  below).
- **`yarn constraints`:** each rule in `yarn.config.cjs` is its own unit.

The stratum's coverage figures are therefore not directly comparable to v0.12's 61%. The log
reports them separately and does not merge them.

### 2.2 Buckets

Each unit gets exactly one bucket:
- **A:** expressible today, confirmed against the clone (R4).
- **B-config:** expressible by configuration of the generic `constraint` kind over named selectors
  and relations ([`general-linter.md`](../general-linter.md) sections 7.2 to 7.4), using
  adapters the relation layer already has. Each B-config unit names the one assertion family it
  needs: `equality`, `unique-key`, `cardinality`, `reference`, `set`, `group` or `bounded-count`.
  A functional dependency counts as `unique-key`: one value per key across a group, such as one
  version per dependency.
- **B-adapter:** needs a new reusable adapter or operator. Examples: a format alint cannot read, an
  extractor for ICU or printf placeholders, an order or semver-range operator.
- **B-kernel:** needs a specialized native kernel.
- **B-delegate:** repository-state logic that belongs in a plugin or `command`, not in alint's
  core. An example is validating a message format's own syntax.
- **C:** expressible today but awkward (a preset, a `normalize:`, a bundle relaxation).
- **D:** a deliberate non-goal, with the reason (R2 as narrowed by R7).

Value-add assertions that the repository does not enforce stay outside the ratio (R3, R5).

Rulings from batch 1, applied to every later batch:
- **Key parity across a group counts as `cardinality`.** "Every key exists in every locale" is,
  per key, a count of distinct locales equal to the number of locales (forem-01). `set` is for
  relations between two named sets: inclusion, equality and disjointness (trpc-07, immich-20,
  strapi-62). The ruling decides only the tag: ADR-0033 later expresses key parity as set
  equality per locale, which covers forem-01 too.
- **The relation layer's `entry` adapter is key-only.** An assertion over a locale entry's value
  (a value that equals its key, a banned word in values, placeholders kept per key) needs
  (key path, value) rows, which no adapter emits. It is B-adapter, not B-config.
- **A convention violated at the pin is not enforced (R5).** Neither is a CODEOWNERS file on its
  own, nor a step that only decides whether a post-merge notification runs.
- **A commit-message check that a git kind expresses is C.** The clones are depth 1, so R4 cannot
  confirm it.
- **Two alint defects to work around, not to count against a unit:**
  - `key_parity` nested under `for_each_file` stops with an internal error
    ([#502](https://github.com/asamarts/alint/issues/502); a templated `reference:` there also
    fails to load, [#504](https://github.com/asamarts/alint/issues/504));
  - `git_tracked_only: true` on `file_absent` and `dir_absent` does not see a force-added file
    that `.gitignore` excludes ([#503](https://github.com/asamarts/alint/issues/503)).

Rulings from batch 2, applied by the integrator to every batch:
- **An assertion that joins a name to its value is B-adapter.**
  - Examples: one range per dependency, a `catalog:` specifier that resolves, and a map key that
    must agree with a sibling field.
  - Each needs (key path, value) rows. The structured adapters (`entry`, TOML, XML) are key-only,
    and `extract`'s values carry no pointer.
  - Such a unit records `needs: entry-values` and keeps the assertion family it will need once
    the adapter exists.
  - Uniqueness or inclusion over key paths alone, or over extracted values alone, stays B-config.
- **Disjoint dependency fields are `unique-key`:** (manifest, name) determines the field.
- **A linter rule that asserts a property of the file rather than its code is classified by its
  logic.** Examples are a name, a header, an encoding or a fixed attribute. Import restrictions
  and the rest of the linter run are code analysis (D).
- **A check that only an external service makes, such as a registry at publish time, is D.**
- **A regex read of a JSON key, or a regex that holds for one layout only, is C.**

Rulings from batch 3:
- **Agreement between two release-age settings is family `other`.** Release age is not a
  dependency version, range, protocol or field (section 2.3).
- **A documented requirement counts as a convention** even when a fallback hides its failure.
  Rocket.Chat's `docs/npm-publishing.md` states its publishing requirements, so they count; the
  registry check in tq-B05 is stated nowhere in that repository, so it is D.

Rulings from the audit, applied to every batch (the log lists each unit they moved):
- **The batch 2 regex ruling, made precise.** A regex read of a JSON key is C when it depends on
  the layout (line breaks, key order, one entry per line) or on a hand-kept list. A regex over
  JSON's `"key": value` token pair that holds in any layout can be A, when R4 confirms it.
  Markdown frontmatter and YAML values read by regex are C: a regex reads a value on one line,
  not a list or a nested block. An inclusion that `cross_file` expresses over values read that
  way is C too; uniqueness over them, which no kind expresses, is B-config `unique-key`.
- **Release age is `other` wherever it stands alone**, not only in agreement units.
- **`locale` covers locale files.** A check over translatable strings in source or layout files
  is `other`.
- **`toolchain-pin` is versions agreeing.** A compile under a toolchain version is `other`.
  Install-time package-manager settings (allowed build scripts, patches, cache integrity) are
  `dep-version`, as the batches already tagged them.
- **A schema the repository does not ship** is C: `json_schema_passes` takes a schema file outside
  the repository. A validator with semantics beyond a schema (OpenAPI, template syntax) is
  B-delegate.
- **A comparison with another revision's content** (the merge-base, a tag, another branch) over
  files alint reads is B-adapter `base-revision`, not D. A rule over which paths a change touches
  is different: a git kind expresses it, so it is C, as a commit-message check is.
- **Inline suppressions** that a path exclude approximates make a unit C, as a hand-kept list.
- **A B or D unit may list a partial rule**, one that checks the half alint can express. A rule
  demoted from an A or C unit stays in the configuration at level `info`.
- **A B-adapter unit with `needs: entry-values` names the assertion family it will need.** Other
  B-adapter units may name one; the figures use only the first.

Rulings from the review (2026-11-13). Each states where an earlier ruling already drew the line,
and the log lists the units that did not follow it:
- **A hand-kept list is one derived from repository state,** such as the workspace names
  (strapi-07) or the files that carry an inline suppression. A rule that needs one is C.
  Restating a list the checker itself states (an allowlist or an exemption list) is ordinary
  configuration: yarnpkg-berry-13, rushstack-23, zulip-43, nextcloud-12, jest-11 and jest-20.
- **`locale` covers locale files and the registries that list locales** (forem-b01,
  mattermost-b02). An encoding check over every source or resource file is `other`
  (thunderbird-android-61, -62).
- **`toolchain-pin` covers a toolchain pin's presence, form, integrity and agreement:** a pin that
  must exist (tq-01, wsdk-54), a `yarnPath` that resolves (rocketchat-18, babel-24), a wrapper
  checksum (thunderbird-android-31, -32), and a guard on the pinned package manager (fluentui-56,
  -b01). The audit's "versions agreeing" names the commonest case.
- **`base-revision` covers what a check needs from the pull request's base:** that revision's
  content, or the change set beyond the paths a git kind sees. A check that a change adds some
  file is path-level and C (backstage-36, tq-57, wsdk-25, rushstack-27). A check that the added
  file names each changed package joins the change set to content and is B-adapter (ghost-39,
  yarnpkg-berry-16). A merge-window gate keyed on another branch's state is D (nextcloud-50).
- **A folded sub-check decides the unit's bucket** (the tokio ruling of the v0.12 study), and the
  half alint expresses stays as a partial rule (rushstack-b14).

**Every B-adapter unit records what it needs** (`needs`), so the synthesis can rank capabilities
as well as assertion families. Each unit gets the one capability it lacks most directly:
- `entry-values`: (key path, value) rows, per the batch 2 ruling;
- `version-compare`: range satisfaction, ordering or range validity;
- `order`: key or element order, including order relative to another file;
- `placeholder`: placeholders extracted from a message and compared per key;
- `plural-categories`: the CLDR plural categories of each locale;
- `icu`: the ICU message syntax tree;
- `format-reader`: a format alint cannot read (Markdown frontmatter, CSV, JSON5, a lockfile, data
  in a TypeScript module);
- `base-revision`: the pull request's base or another revision: its content, or the change set
  beyond the paths a git kind sees (added files' content, renames, the workspaces a change
  touches);
- `route-model`: a documentation site's routes;
- `package-graph`: a graph whose edges are package names;
- `link-type`: whether a path is a symbolic link;
- `derived-value`: an expected value computed from other fields, files or paths;
- `normalization`: Unicode or unit normalization;
- `key-predicate`: a predicate over key names;
- `other`: anything else, named in the unit's reason.

The "after slice" figure is also reported with the value-bearing entry adapter added, since the
slice's dependency version policy needs it.

**Before and after the slice.** v0.23 ships `constraint` with the `equality`, `unique-key` and
`cardinality` families (ADR-0032 decision 1). The "after" figure moves each B-config unit tagged
with one of those three families to A, once phase 8 confirms that the slice expresses it.
ADR-0033 then swapped `cardinality` for `set`. The records keep ADR-0032's definition, and
ADR-0033 gives the same units' figures under its slice.

### 2.3 Families

Each unit also carries one family tag:
- **`locale`:**
  - key parity between locale files;
  - placeholder and plural-form agreement;
  - locale-file format, encoding and order;
  - catalog freshness.
- **`dep-version`:**
  - one version or range per dependency across manifests;
  - workspace-protocol and catalog use;
  - allowed and banned ranges and dependency fields;
  - internal packages pinned to the workspace version.
- **`toolchain-pin`:**
  - a toolchain pin's presence, form and integrity (a `yarnPath` that resolves, a wrapper
    checksum);
  - Node, package-manager and language-toolchain versions agreeing across `.nvmrc`,
    `.node-version`, `.tool-versions`, `engines`, `packageManager`, Volta and CI.
- **`other`:** everything else.

Coverage is reported overall and per family.

### 2.4 Rules for analysts

- The clones are read-only. Nothing from a corpus repository runs: no package manager, build,
  test or script of the repository.
- Configurations use no spawning kinds (`command`, `command_idempotent`, `generated_file_fresh`)
  and no custom facts. They also avoid git-history kinds, because the clones are depth 1.
- Every A unit in the `locale`, `dep-version` and `toolchain-pin` families must be shown to fire on
  a scratch copy that breaks the invariant, as well as to pass on the clone (R4 in both
  directions).

## 3. The stratum

Twenty repositories, pinned to their default-branch HEAD on 2026-11-12 and resolved read-only
through the GitHub API. Every one:
- is active (pushed within the last day of the pin);
- is not archived and is not a fork;
- is absent from the 110-repository corpus;
- has the stratum's policy under a gate found in its CI, scripts or tests.

The table shows 10-character prefixes; the full SHAs are in `pins.tsv`. The "gate found"
column is from the selection; each record has the gates its analysts confirmed. workers-sdk
lists `@manypkg/cli` as a dependency but runs no manypkg check.

### 3.1 i18n

| Repository | Pin | Locale format | Gate found |
|---|---|---|---|
| element-hq/element-web | `e84183a6b2` | nested JSON | `i18n_check` reusable workflow |
| signalapp/Signal-Desktop | `848443afc3` | `_locales/*/messages.json`, ICU | `pnpm run lint-intl` |
| TryGhost/Ghost | `ab272fcb60` | i18next JSON | CI `job_i18n` |
| mattermost/mattermost | `69102aba22` | react-intl JSON, go-i18n JSON | `i18n-ci-template.yml` |
| RocketChat/Rocket.Chat | `fb1d4217d8` | `*.i18n.json` | `@rocket.chat/i18n run check` |
| zulip/zulip | `39bd5f2f40` | gettext `.po`, JSON | `tools/check-frontend-i18n`, `makemessages` |
| forem/forem | `75faefce59` | Rails YAML | `spec/i18n_spec.rb` (i18n-tasks) |
| nextcloud/server | `d04a299b92` | l10n JSON and JS | `build/translation-checker.php` |
| immich-app/immich | `7f13ab5e13` | flat JSON, Dart | CI "Test i18n", `check_i18n_keys.py` |
| thunderbird/thunderbird-android | `ac1f0e85f6` | Android `strings.xml` | Android lint `lint.xml` |

### 3.2 JavaScript monorepos

| Repository | Pin | Workspace | Gate found |
|---|---|---|---|
| yarnpkg/berry | `f359f65c1f` | Yarn | `yarn constraints`, `yarn version check` |
| jestjs/jest | `aa88f7cf0b` | Yarn | `yarn constraints`, `yarn dedupe --check` |
| babel/babel | `2bc74adead` | Yarn | `yarn constraints`, dedupe, catalogs |
| microsoft/fluentui | `b4f5c3cca4` | Yarn, Nx | `syncpack list-mismatches`, `beachball check` |
| TanStack/query | `9b528b5e51` | pnpm, Nx | `sherif`, `knip`, catalogs |
| trpc/trpc | `8eff9ff0dd` | pnpm, Turborepo | `manypkg`, `.nvmrc` and `.tool-versions` |
| strapi/strapi | `fcc1df0668` | Yarn, Nx, Lerna | `check-package-versions.mjs`, `syncpack lint` |
| microsoft/rushstack | `a1d41fc46f` | Rush | consistent versions, `rush change --verify` |
| cloudflare/workers-sdk | `3cea40b078` | pnpm, Turborepo | `check:package-deps`, catalogs |
| backstage/backstage | `bc73976a6d` | Yarn | `verify-local-dependencies.js` |

Six i18n repositories are also workspaces: Ghost, Rocket.Chat, element-web, immich,
Signal-Desktop (`packages/*`) and mattermost (`webapp/`). zulip's workspace holds one package.
Their `dep-version` and `toolchain-pin` units count like any other.

## 4. Execution

The study runs in four batches of five repositories, each a workflow:
- Stage A runs once per repository, in parallel.
- Stage B starts as soon as that repository's Stage A finishes.
- Stage C follows Stage B: a separate workflow in batch 1, inside the batch workflow from batch 2
  on.
- Integration and a commit follow each batch.

The batches mix the two strata, so each one gives an early signal on both:
1. forem, zulip, element-web, yarnpkg/berry, trpc.
2. Signal-Desktop, immich, TanStack/query, jest, rushstack.
3. Ghost, Rocket.Chat, babel, strapi, workers-sdk.
4. mattermost, nextcloud/server, thunderbird-android, fluentui, backstage.

## 5. Outputs

- **Per repository:** a JSON record in [`stratum/`](stratum/) (pin, units with buckets, families
  and the rules that check them, counts, `coverage_today`, `coverage_with_tuning`, `gap`, the B
  split, notes), a totals row in the log, and its configuration in the corpus. `cargo test -p
  xtask` checks every record against its pin and configuration and recomputes its metrics.
- **Aggregate:** the bucket shares overall and per family and stratum, before and after the slice,
  recorded in `general-linter.md` section 12.4. Also the assertion families that the B-config
  units and the entry-values B-adapter units need, ranked, as input to the phase 3 design.
