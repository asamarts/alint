# v0.23 corpus stratum: log

Status: batches 1 to 3 of 4 complete (2026-11-12). Frame and method:
[`corpus-stratum.md`](corpus-stratum.md).

Each repository has a record in [`stratum/`](stratum/), named by its slug. A record holds:
- every unit, with its evidence, mechanism, family, bucket, assertion family and reason, and the
  changes Stage B or the integrator made to it;
- the units removed and the Stage B suggestions rejected, each with the reason;
- the value-add rules, absent categories and the "unusual" scan;
- Stage B's configuration issues and summary;
- Stage C's configuration changes and the final `validate-config` and `check` result.

Each configuration is in [`../v0.12/corpus/`](../v0.12/corpus/), with its row in `pins.tsv`.

The coverage figures apply R7, so they are not comparable with v0.12's 61%
([`corpus-stratum.md`](corpus-stratum.md) section 2.1). The two "after" columns:
- **After slice** counts a B-config unit as A when its assertion family is `equality`,
  `unique-key` or `cardinality`.
- **With values** also counts a B-adapter unit that needs only value-bearing entry rows
  (`needs: entry-values`) and one of those three families.

Phase 8 confirms each such unit before either figure is final.

## Running totals

Through batch 3 (15 repositories).

| Repository | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| forem/forem | 58 | 21 | 5 | 6 | 0 | 0 | 4 | 22 | 58% | 69% | 31% | 67% | 69% |
| zulip/zulip | 188 | 108 | 1 | 3 | 0 | 3 | 11 | 62 | 86% | 94% | 6% | 87% | 87% |
| element-hq/element-web | 48 | 5 | 0 | 4 | 0 | 1 | 4 | 34 | 36% | 64% | 36% | 36% | 36% |
| yarnpkg/berry | 43 | 13 | 1 | 2 | 0 | 0 | 6 | 21 | 59% | 86% | 14% | 59% | 64% |
| trpc/trpc | 65 | 10 | 5 | 15 | 0 | 1 | 5 | 29 | 28% | 42% | 58% | 36% | 39% |
| signalapp/Signal-Desktop | 74 | 12 | 0 | 4 | 0 | 1 | 11 | 46 | 43% | 82% | 18% | 43% | 43% |
| immich-app/immich | 53 | 9 | 1 | 8 | 0 | 0 | 5 | 30 | 39% | 61% | 39% | 39% | 39% |
| TanStack/query | 52 | 14 | 2 | 6 | 0 | 1 | 6 | 23 | 48% | 69% | 31% | 55% | 59% |
| jestjs/jest | 55 | 21 | 3 | 3 | 0 | 0 | 5 | 23 | 66% | 81% | 19% | 72% | 75% |
| microsoft/rushstack | 78 | 34 | 6 | 6 | 1 | 2 | 12 | 17 | 56% | 75% | 25% | 64% | 66% |
| TryGhost/Ghost | 98 | 36 | 7 | 14 | 0 | 0 | 19 | 22 | 47% | 72% | 28% | 50% | 51% |
| RocketChat/Rocket.Chat | 86 | 22 | 1 | 19 | 0 | 0 | 7 | 37 | 45% | 59% | 41% | 47% | 47% |
| babel/babel | 68 | 27 | 3 | 4 | 0 | 3 | 6 | 25 | 63% | 77% | 23% | 67% | 70% |
| strapi/strapi | 78 | 16 | 4 | 10 | 0 | 0 | 22 | 26 | 31% | 73% | 27% | 37% | 38% |
| cloudflare/workers-sdk | 83 | 32 | 1 | 9 | 1 | 0 | 5 | 35 | 67% | 77% | 23% | 67% | 67% |
| **Pooled** | 1127 | 380 | 40 | 113 | 2 | 12 | 128 | 452 | 56% | 75% | 25% | 60% | 61% |

| Family | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 81 | 18 | 3 | 23 | 0 | 2 | 19 | 16 | 28% | 57% | 43% | 32% | 32% |
| `dep-version` | 127 | 33 | 6 | 17 | 0 | 0 | 19 | 52 | 44% | 69% | 31% | 49% | 60% |
| `toolchain-pin` | 58 | 30 | 0 | 13 | 0 | 0 | 2 | 13 | 67% | 71% | 29% | 67% | 67% |
| `other` | 861 | 299 | 31 | 60 | 2 | 10 | 88 | 371 | 61% | 79% | 21% | 64% | 65% |

The assertion families the B-config units need, with the B-adapter units that need only entry
values:

| Assertion family | B-config | B-adapter, entry values | locale | dep-version | toolchain-pin | other |
|---|--:|--:|--:|--:|--:|--:|
| `unique-key` | 22 | 7 | 2 | 11 | 0 | 16 |
| `set` | 8 | 3 | 1 | 3 | 0 | 7 |
| `reference` | 7 | 3 | 0 | 3 | 0 | 7 |
| `cardinality` | 2 | 0 | 1 | 0 | 0 | 1 |
| `equality` | 0 | 2 | 0 | 1 | 0 | 1 |
| `bounded-count` | 1 | 0 | 0 | 0 | 0 | 1 |
| `none` | 0 | 1 | 1 | 0 | 0 | 0 |

The per-repository median `coverage_today` is 48%. zulip contributes 188 of the 1,127 units; 108
of its A units are custom regex rules in its own linter, each a unit under R1. The family tables
are the stratum's measure; the pooled figure is context.

## Signals so far

1. **The slice's dependency version policy needs a value-bearing entry adapter.**
   - "One range per dependency across workspaces" is enforced in 7 of the 8 JavaScript monorepos
     so far: trpc-05, yarnpkg-berry-01, tq-10, jest-23, rushstack-01, strapi-02 and babel-11.
     babel's variant allows a literal range in one workspace only and sends the rest to catalogs.
   - workers-sdk enforces the same policy another way: a dependency named in the pnpm catalog
     must use `catalog:` (wsdk-09).
   - Each needs (dependency name, range) rows. No adapter emits them today: `entry`, TOML and XML
     are key-only, and `extract` drops the pointer (batch 2 ruling).
   - With that adapter, `dep-version` moves from 44% today to 60% after the slice. Without it,
     the move stops at 49%.
2. **Version comparison is the most requested operator:** 15 B-adapter units in 8 repositories.
   - It covers range satisfaction, ordering and range validity: forem-07, element-web-13,
     trpc-03, trpc-06, immich-11, -52 and -53, tq-03, rushstack-06, -55 and -b11, rocketchat-16
     and -80, wsdk-55 and -56.
   - These 15 include all 13 `toolchain-pin` B-adapter units. The pins that must agree are
     already A; the rest is "this exact pin satisfies that range". So the slice leaves
     `toolchain-pin` at 67% unless it adds the operator.
   - Five more units are C because a regex stands in for the operator: version ceilings in
     ghost-27, -34, -35 and -36, and rocketchat-17, which holds only while `engines.yarn` is exact.
3. **`unique-key` leads the assertion families**, with 22 B-config units and 7 more once entry
   values exist.
   - Workspace names that must be unique (trpc-B05, jest-B02, tq-35, babel-B01) are the common
     B-config case: the values are there, and alint has no uniqueness relation over them.
   - Duplicate keys in one locale file (zulip-28, rocketchat-10) and disjoint dependency fields
     (trpc-08, jest-24, rushstack-51, babel-12) are the others.
4. **The locale family barely moves with the slice:** from 28% to 32%. Its B units need format
   knowledge, not new assertion families:

   | Need | Units |
   |---|---|
   | Placeholders kept per key | forem-04, zulip-02, immich-05, ghost-12, -13, -14, rocketchat-08, -09 |
   | Plural categories per locale | forem-03, ghost-03, rocketchat-04, -05 |
   | Key order | element-web-02, immich-01, ghost-07, rocketchat-01, -02 |
   | ICU message structure | signal-desktop-06, -07, -09 |
   | Message syntax (B-delegate) | zulip-01 (gettext), signal-desktop-10 (ICU) |
   | Values of entries | element-web-05, -06 |
   | Unicode normalization | zulip-05 |

   Its 19 C units are mostly:
   - regex approximations of ICU or placeholder rules;
   - JSON parse checks that alint's JSONC-tolerant reader relaxes (zulip-04, element-web-08,
     ghost-10, rocketchat-11, strapi-b01);
   - key-name regexes.
5. **Predicates over JSON key names recur:**
   - expressed today only as layout-dependent line regexes (C): signal-desktop-01, element-web-04,
     immich-49, ghost-11;
   - or not at all: rushstack-b12, B-adapter `key-predicate`.
6. **Map keys as a selector recur in C units.** Catalog entries are YAML or JSON map keys, which
   JSONPath cannot select, so `catalog:` references are checked with line regexes (strapi-06,
   wsdk-74) or wait for entry values (trpc-10, babel-17).

## Batch 1 (2026-11-12): forem, zulip, element-web, yarnpkg/berry, trpc

The counts below use the current classification. Batch 2's rulings moved trpc-05,
yarnpkg-berry-01, trpc-10, forem-b08 and forem-29 to B-adapter (`needs: entry-values`), and
trpc-08 from `set` to `unique-key`.

### The three families

- **`locale`** (22 units, all in the three i18n repositories): 4 A, 4 C, 1 B-config, 7
  B-adapter, 1 B-delegate, 5 D.
  - **The B-config unit:** forem's i18n-tasks missing-keys check (forem-01). It compares
    per-locale key trees merged from many files. Per-file `key_parity` needs 114 rules and gives
    8 false positives at the pin, because forem keeps some keys in different files per locale.
    This is cardinality over (locale, key) rows.
  - **The adapter gaps:**
    - placeholders kept per key: Ruby `%{var}` (forem-04); `python-format` and
      `python-brace-format` in gettext `.po` (zulip-02);
    - plural categories per locale (forem-03);
    - nested key order (element-web-02);
    - assertions over entry values (element-web-05 and -06);
    - Unicode normalization of the file (zulip-05).
  - **The C units:**
    - two parse checks that alint relaxes;
    - a key charset that holds for one layout only;
    - a `.po` header check (zulip-07).
- **`dep-version`** (31 units): 7 A, 3 C, 2 B-config, 5 B-adapter, 14 D.
  - **B-config:** peer dependencies also in dev dependencies, as `set` (trpc-07); a dependency
    in only one dependency type, as `unique-key` (trpc-08).
  - **B-adapter:**
    - one range per dependency (trpc-05, yarnpkg-berry-01) and catalog specifiers that resolve
      (trpc-10), which need entry values;
    - a semver range or version comparison (forem-07, trpc-06).
  - **D:** mostly lockfile freshness, `dedupe --check` and minimum release age, which need the
    package manager's resolution.
- **`toolchain-pin`** (12 units): 6 A, 2 B-adapter, 4 D.
  - **A:**
    - pins that agree: `.ruby-version` and the base image, `engines.node` and the
      `yarn.config.cjs` floor, the CI matrix and `engines`;
    - pins that are present and well formed: `.nvmrc`, `packageManager`, `yarnPath`.
  - **B-adapter:** an exact pin that must satisfy a declared range (element-web-13, trpc-03).

### Stage B and integration

- **Stage B:** added 27 units, reclassified 17 and removed 4.
- **The integrator's changes:**
  - **zulip-60 removed.** Stage B reclassified it to C. It is a post-merge step that decides
    whether a notification runs, not a validation.
  - **element-web-50 rejected.** Stage B proposed it as a new unit. Translator review happens on
    Localazy, and the clone has about 70 violations at the pin (R5).
- **Stage C** (a fix-up workflow for this batch):
  - applied the reproduced configuration fixes, added rules for the missed A and C units, and
    demoted rules for units that left A or C to value-add at level `info`;
  - flagged four units, which moved after review:

    | Unit | Bucket | Why |
    |---|---|---|
    | zulip-04 | C | JSON parse relaxed |
    | zulip-21 | C | sort folds case the other way |
    | zulip-28 | B-config, `unique-key` | duplicate YAML keys |
    | zulip-39 | C | scope narrowed by a capture trick |

- **Validation.** Every configuration validates and passes on its clone; the findings that
  remain are `info`-level value-add rules. `xtask corpus-replay --only` replays all five at their
  pins.

### alint defects found

1. **`key_parity` nested under `for_each_file` stops with an internal error:** "key_parity
   requires the relation read cache". The nested evaluation context has no relation read cache.
   It reproduces with a one-file reference and peer.
2. **`git_tracked_only: true` on `file_absent` and `dir_absent` misses force-added ignored
   files.** It does not fire on a file that is force-added but excluded by `.gitignore` under the
   default `respect_gitignore: true`.
   - `docs/rules.md` and `concepts/targeting/the-walker-and-git.md` say it fires; it fires only
     with `respect_gitignore: false`.
   - It reproduces with the walker page's own `target/` example.

### Sharp edges (C)

- The JSON reader accepts trailing commas and comments, and has no strict mode (element-web-08,
  zulip-04).
- `ordered_block` `lexical-ci` folds to lowercase. `sort -f` folds to uppercase, so the two
  disagree on `[ \ ] ^ _` and the backtick against letters (zulip-21).
- `json_schema_passes` meta-validates a schema offline only through a shim schema outside the
  root (yarnpkg-berry-23).
- `registry_paths_resolve` cannot narrow the entries it accepts to a glob set (zulip-39).
- A `cross_file` set source takes one `files` glob, so it cannot exclude dotted file stems
  (forem-05).

## Batch 2 (2026-11-12): Signal-Desktop, immich, TanStack/query, jest, rushstack

### The three families

- **`locale`** (26 units): 3 A, 9 C, 5 B-adapter, 1 B-delegate, 8 D.
  - **Signal-Desktop's ICU linter alone gives 10 units:**
    - rules alint approximates with a regex over each message are C;
    - rules that need the ICU syntax tree are B-adapter;
    - the parse step itself is B-delegate.
  - **immich:**
    - recursive key order (immich-01) and placeholder overrides (immich-05) are B-adapter;
    - strict JSON (immich-03) and Dart-safe key names (immich-49) are C.
- **`dep-version`** (48 units): 8 A, 6 C, 3 B-config, 5 B-adapter, 26 D.
  - **A:**
    - `workspace:` and `patch:` specifiers that resolve (jest-B01, jest-B03, tq-14);
    - dev-only dependencies (jest-10);
    - pinned specifiers (signal-desktop-16).
  - **B-config:** disjoint dependency fields (jest-24, rushstack-51), and approved packages
    (rushstack-02, `reference`).
  - **B-adapter:**
    - one range per dependency (tq-10, jest-23, rushstack-01);
    - sherif's similar-package families (tq-11);
    - lockfile rows (signal-desktop-18).
- **`toolchain-pin`** (19 units): 9 A, 7 B-adapter, 3 D. Every B-adapter unit is a version
  comparison:
  - range satisfaction (immich-11, -52, -53, tq-03, rushstack-06);
  - ordering (rushstack-55);
  - range validity (rushstack-b11).

### Stage B and integration

- **Stage B:** added 44 units, reclassified 17 and removed 1.
  - It moved Signal-Desktop's import-restriction rules to D (code analysis).
  - It split structural checks out of D units: rushstack's command-line and rig loader checks,
    immich's workflow permissions.
- **Stage C now runs inside the batch workflow.** It added rules for the missed units, applied
  the reproduced fixes, and demoted rules for units that left A or C.
- **The integrator's changes:**
  - **One range per dependency** moved to B-adapter with `needs: entry-values` (tq-10, jest-23,
    rushstack-01), with the same change applied to batch 1.
  - **Disjoint dependency fields** aligned on `unique-key` (jest-24, rushstack-51, and batch 1's
    trpc-08).
  - **tq-B05 to D:** the npm registry checks the repository URL at publish time. Its rule is now
    value-add.
  - **tq-B04 and jest-27 to C:** both are regex reads that hold for one layout.
  - **rushstack:**
    - rushstack-b09 to A, which Stage C expressed;
    - rushstack-b12 to B-adapter, a predicate over key names;
    - rushstack-c01 added, the plugin-command check that rushstack-09b's rule really expresses.
  - **Kept:**
    - jest-18, -21 and -28: the Docusaurus build fails the Netlify production deploy as well as
      the deploy preview;
    - jest-12: the changelog check runs on pull requests;
    - signal-desktop-b03: the beta-build check runs in CI for every beta version.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  immich's 271 `info` findings from its value-add stale-key rule. `xtask corpus-replay --only`
  replays all five.

### Sharp edges (C)

- `json_schema_passes` takes no inline schema. A corpus configuration must replay standalone, so
  a schema check that the repository does not ship cannot be expressed at all (rushstack-09b,
  rushstack-b12).
- `unique_by` keys on path templates only, and `cross_file` has no uniqueness relation. Unique
  workspace names therefore wait for the slice's `unique-key` (trpc-B05, jest-B02, tq-35).

## Batch 3 (2026-11-12): Ghost, Rocket.Chat, babel, strapi, workers-sdk

### The three families

- **`locale`** (33 units): 11 A, 2 B-config, 11 B-adapter, 6 C, 3 D.
  - **Ghost:**
    - key parity over five namespaces with CLDR plural groups is A (ghost-02). So are the
      locale directories against `locale-data.json` (ghost-05) and a 255-character cap on one
      translation that becomes a Stripe field label (ghost-06);
    - the plural categories each locale must carry (ghost-03) and its linter's placeholder rules
      (ghost-12, -13, -14) are B-adapter;
    - `context.json`, regenerated from the en key union in canonical order and compared byte
      for byte, is one B-adapter unit (ghost-07, key order).
  - **Rocket.Chat's `i18n check` task runner gives 11 locale units:**
    - extra keys, legacy `__placeholder__` values, trailing whitespace, the file set and the base
      locale are A (rocketchat-03, -06, -07, -12, -82);
    - key order (rocketchat-01, -02), plural categories (-04, -05) and placeholders (-08, -09)
      are B-adapter;
    - duplicate keys are B-config `unique-key` (rocketchat-10).
  - **strapi does not enforce locale key parity.** `scripts/front/verify-translations` is not in
    CI, and 99 of 261 locale files have extra keys at the pin (R5). Its locale units are narrow
    tests: strapi-22 is A, strapi-21 (no `X.singular` and `X.plural` pair) is B-config
    `unique-key`, and three are C.
- **`dep-version`** (48 units): 18 A, 1 B-config, 7 B-adapter, 10 C, 12 D.
  - **A:**
    - babel's `yarn.config.cjs` rules on `workspace:^`, `@babel/core` placement and peer ranges
      (babel-01, -13 to -16);
    - `workspace:` and `patch:` specifiers that resolve (babel-18, -19, rocketchat-25, -26,
      strapi-05, wsdk-73);
    - exact catalog and public-package pins (wsdk-10, -11, strapi-03);
    - allowed `yarn.lock` resolution schemes (strapi-04).
  - **B-adapter:** one range per dependency (strapi-02, babel-11), a `catalog:` specifier that
    resolves (babel-17), the catalog-or-pin rule (wsdk-09), an allowlist of a package's
    non-workspace dependencies (wsdk-06), and `workspace:*` for compliant internal packages
    (ghost-32). All six need entry values. wsdk-07 reads one array of a TypeScript module.
  - **C:** four version ceilings hand-encoded as regexes in Ghost, and three `catalog:` or
    lockstep checks in strapi that rest on a line regex or a hand-kept list.
- **`toolchain-pin`** (27 units): 15 A, 4 B-adapter, 2 C, 6 D.
  - Ghost alone has 7 A units. Its Node pins (CI, `devEngines`, `engines`, the Dockerfile,
    `.nvmrc`, `.node-version`) move together, `@types/node` follows the Node major,
    `packageManager` is exact, and the Tinybird CLI version agrees in two files.
  - All four B-adapter units are version comparison (rocketchat-16, -80, wsdk-55, -56).

### Stage B and integration

- **Stage B:** added 41 units, reclassified 17 and removed 8.
  - It found test-embedded data assertions Stage A had not read: Ghost's e2e fixture checks,
    Rocket.Chat's npm publishing requirements, and workers-sdk's deployment validators.
  - It merged ghost-07 and ghost-08 (one byte comparison, R1) and moved strapi-20 and -48 from
    A to C, each with a reproduced false positive.
  - Stage C fixed the precision problems Stage B reproduced, such as wsdk-10's catalog exception
    and babel-18's under-scoped manifest read.
- **The integrator's changes:**
  - **Entry values:** babel-11, babel-17, wsdk-06 and wsdk-09 record `needs: entry-values`;
    ghost-32 and strapi-02 move to B-adapter with it. Each keeps its assertion family.
  - **Release age is `other`:** agreement between two release-age settings is not a dependency
    version (section 2.3), as jest-14 was. This moves ghost-b14, ghost-b15 and rocketchat-24.
  - **strapi-07 to C:** its pinned-exact half needs a hand-kept list of 43 workspace names.
  - **rocketchat-76 to -79 stay A, as a convention:** `docs/npm-publishing.md` states each
    requirement. tq-B05 went to D in batch 2 because nothing in that repository stated it.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  4 `info` findings from value-add rules (Rocket.Chat 3, babel 1) and 2 notes from Ghost's
  key-extraction rule. `xtask corpus-replay --only` replays all five.

### Sharp edges (C)

- **alint's JSON reader is JSONC-tolerant.** A trailing comma or a `//` comment passes, though
  `JSON.parse`, rollup's `json()` and i18next reject them (ghost-10, rocketchat-11, strapi-b01).
  A strict-JSON option on the parse check would make these A.
- **A version ceiling has no operator,** so it is a regex over the version string (ghost-27,
  -34, -35, -36).
- **A value cannot name a selector.** strapi-01 hand-keeps the canonical major in a regex, and
  wsdk-12 splits one date equality into three normalizations.
- **`markdown_fragments_resolve` does not know HTML anchors** (rocketchat-29), and
  `markdown_links_resolve` does not read `<a href>` (rocketchat-28).
