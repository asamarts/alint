# v0.23 corpus stratum: log

Status: complete (2026-11-12): 20 repositories in four batches. Frame and method:
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

## Totals

All 20 repositories, 1,546 units.

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
| mattermost/mattermost | 122 | 33 | 7 | 20 | 0 | 3 | 19 | 40 | 40% | 63% | 37% | 44% | 48% |
| nextcloud/server | 53 | 11 | 0 | 5 | 0 | 0 | 7 | 30 | 48% | 78% | 22% | 48% | 52% |
| thunderbird/thunderbird-android | 67 | 17 | 1 | 6 | 0 | 1 | 15 | 27 | 42% | 80% | 20% | 45% | 48% |
| microsoft/fluentui | 77 | 19 | 4 | 10 | 0 | 0 | 9 | 35 | 45% | 67% | 33% | 52% | 57% |
| backstage/backstage | 100 | 30 | 3 | 10 | 0 | 0 | 29 | 28 | 42% | 82% | 18% | 43% | 46% |
| **Pooled** | 1546 | 490 | 55 | 164 | 2 | 16 | 207 | 612 | 52% | 75% | 25% | 56% | 58% |

| Family | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 141 | 30 | 10 | 36 | 0 | 5 | 36 | 24 | 26% | 56% | 44% | 31% | 32% |
| `dep-version` | 172 | 46 | 7 | 21 | 0 | 0 | 30 | 68 | 44% | 73% | 27% | 48% | 58% |
| `toolchain-pin` | 79 | 41 | 0 | 14 | 0 | 0 | 8 | 16 | 65% | 78% | 22% | 65% | 65% |
| `other` | 1154 | 373 | 38 | 93 | 2 | 11 | 133 | 504 | 57% | 78% | 22% | 61% | 62% |

The per-repository median `coverage_today` is 46% (range 28% to 86%). zulip contributes 188 units;
108 of its A units are custom regex rules in its own linter, each a unit under R1. The family
tables are the stratum's measure; the pooled figure is context.

### By stratum

| Stratum | Family | Units | Today | After slice | With values |
|---|---|--:|--:|--:|--:|
| i18n | `locale` | 136 | 26% | 30% | 32% |
| i18n | `dep-version` | 61 | 48% | 48% | 57% |
| i18n | `toolchain-pin` | 37 | 66% | 66% | 66% |
| i18n | `other` | 613 | 65% | 66% | 67% |
| i18n | **all** | 847 | 55% | 57% | 59% |
| JavaScript monorepo | `locale` | 5 | 20% | 40% | 40% |
| JavaScript monorepo | `dep-version` | 111 | 43% | 48% | 58% |
| JavaScript monorepo | `toolchain-pin` | 42 | 65% | 65% | 65% |
| JavaScript monorepo | `other` | 541 | 50% | 55% | 56% |
| JavaScript monorepo | **all** | 699 | 49% | 54% | 57% |

The JavaScript monorepos' 5 `locale` units are incidental; the stratum's locale evidence is the
i18n half's 136.

### What each capability would add

Each step adds one capability to the previous ones and counts the units it would move to A. The
first two steps are the "after slice" and "with values" columns.

| Step | `locale` | `dep-version` | `toolchain-pin` | `other` | Pooled |
|---|--:|--:|--:|--:|--:|
| Today | 26% | 44% | 65% | 57% | 52% |
| + slice | 31% | 48% | 65% | 61% | 56% |
| + entry values | 32% | 58% | 65% | 62% | 58% |
| + version comparison | 32% | 61% | 87% | 62% | 60% |
| + `set` and `reference` | 38% | 68% | 87% | 65% | 63% |
| + locale format adapters | 57% | 68% | 87% | 65% | 66% |

- **+ slice:** B-config units in `equality`, `unique-key` or `cardinality`.
- **+ entry values:** B-adapter units with `needs: entry-values` in one of those families.
- **+ version comparison:** B-adapter units with `needs: version-compare`.
- **+ `set` and `reference`:** B-config units in those families, and entry-values units that
  need them.
- **+ locale format adapters:** B-adapter units that need `placeholder`, `plural-categories` or
  `icu`.

### Assertion families

The assertion families the B-config units need, with the B-adapter units that need only entry
values:

| Assertion family | B-config | B-adapter, entry values | locale | dep-version | toolchain-pin | other |
|---|--:|--:|--:|--:|--:|--:|
| `unique-key` | 30 | 9 | 5 | 12 | 0 | 22 |
| `set` | 13 | 4 | 6 | 4 | 0 | 7 |
| `reference` | 8 | 4 | 0 | 4 | 0 | 8 |
| `equality` | 0 | 9 | 2 | 2 | 0 | 5 |
| `cardinality` | 2 | 0 | 1 | 0 | 0 | 1 |
| `bounded-count` | 1 | 0 | 0 | 0 | 0 | 1 |
| `group` | 1 | 0 | 0 | 0 | 0 | 1 |
| `none` | 0 | 1 | 1 | 0 | 0 | 0 |

### What the B-adapter units need

Each B-adapter unit records the one capability it lacks most directly (frame section 2.2):

| Need | Units | Repositories | `locale` | `dep-version` | `toolchain-pin` | `other` |
|---|--:|--:|--:|--:|--:|--:|
| `entry-values` | 27 | 16 | 5 | 15 | 0 | 7 |
| `format-reader` | 22 | 7 | 0 | 2 | 0 | 20 |
| `version-compare` | 22 | 12 | 0 | 3 | 14 | 5 |
| `order` | 20 | 13 | 7 | 0 | 0 | 13 |
| `other` | 17 | 11 | 0 | 0 | 0 | 17 |
| `placeholder` | 11 | 7 | 11 | 0 | 0 | 0 |
| `base-revision` | 10 | 5 | 0 | 0 | 0 | 10 |
| `route-model` | 9 | 6 | 0 | 0 | 0 | 9 |
| `plural-categories` | 7 | 5 | 7 | 0 | 0 | 0 |
| `derived-value` | 5 | 5 | 0 | 1 | 0 | 4 |
| `icu` | 5 | 2 | 5 | 0 | 0 | 0 |
| `link-type` | 3 | 3 | 0 | 0 | 0 | 3 |
| `package-graph` | 3 | 3 | 0 | 0 | 0 | 3 |
| `normalization` | 2 | 2 | 1 | 0 | 0 | 1 |
| `key-predicate` | 1 | 1 | 0 | 0 | 0 | 1 |
| **Total** | 164 | | 36 | 21 | 14 | 93 |

## Findings

1. **`unique-key` is the assertion family with the most demand; `cardinality` has little.**
   - `unique-key` has 30 B-config units and 9 more once entry values exist. The common cases:
     - unique workspace names (trpc-B05, jest-B02, tq-35, babel-B01);
     - duplicate keys or ids in one locale file (zulip-28, rocketchat-10, mattermost-03,
       thunderbird-android-b01);
     - disjoint dependency fields (trpc-08, jest-24, rushstack-51, babel-12).
   - `cardinality` has 2. Locale key parity is mostly A already, through `key_parity`. The one
     locale unit left for the slice's group-parity form is forem-01, where each locale's keys
     are merged from several files.
   - `equality` has no B-config unit. Its 9 units all need entry values first.
   - `set` (13 B-config, 4 entry values) and `reference` (8 and 4) come next. Neither is in the
     slice. Together they would add 6 points to `locale`, 7 to `dep-version` and 3 pooled.
2. **The slice's dependency version policy needs a value-bearing entry adapter.**
   - One range per dependency across workspaces is enforced in 8 of the 10 JavaScript
     monorepos: trpc-05, yarnpkg-berry-01, tq-10, jest-23, rushstack-01, strapi-02, babel-11 and
     fluentui-17. workers-sdk enforces it through its catalog instead (wsdk-09). backstage does
     not enforce it.
   - Each needs (dependency name, range) rows. No adapter emits them: `entry`, TOML and XML are
     key-only, and `extract` drops the pointer.
   - `entry-values` is the most common need: 27 B-adapter units in 16 of the 20 repositories.
   - With it, `dep-version` goes from 44% today to 58%. The slice alone reaches 48%.
3. **Version comparison is the operator with the most demand.**
   - 22 B-adapter units in 12 repositories need range satisfaction, ordering or range validity.
     They include all 14 `toolchain-pin` B-adapter units.
   - The pins that must agree are already A; what remains is "this exact pin satisfies that
     range". With the operator, `toolchain-pin` goes from 65% to 87%. Without it, the slice
     leaves the family where it is.
   - More units are C because a regex or a normalization stands in for the operator. Examples
     are Ghost's version ceilings (ghost-27, -34, -35, -36), rocketchat-17,
     thunderbird-android-b04 and backstage-15.
4. **The locale family needs format knowledge, not new assertion families.**
   - It goes from 26% to 31% with the slice, and 32% with entry values.
   - Its 41 B-adapter and B-delegate units, by need:

     | Need | Units | Repositories |
     |---|--:|--:|
     | Placeholders kept per key | 11 | 7 |
     | Plural categories per locale | 7 | 5 |
     | Key or element order | 7 | 6 |
     | ICU message structure | 5 | 2 |
     | Values of entries | 5 | 2 |
     | Message syntax (B-delegate) | 5 | 4 |
     | Unicode normalization | 1 | 1 |

   - Adapters for placeholders, plural categories and ICU would take `locale` from 38% to 57%,
     after the steps before them in the capability table.
5. **A strict-JSON option would turn C units into A.** alint's JSON reader accepts comments and
   trailing commas, which `JSON.parse`, i18next and rollup's `json()` reject. 10 locale C units in
   8 repositories approximate a strict parse check because of this, such as zulip-04, ghost-10,
   rocketchat-11 and mattermost-01.
6. **JSONPath cannot select member names.** Map keys, such as catalog entries, dependency names
   chosen by a name set and JSON key names, are read with layout-dependent line regexes (C):
   strapi-06, wsdk-74, thunderbird-android-53, signal-desktop-01 and ghost-11. Or they are not
   expressible at all (rushstack-b12, `key-predicate`). The relation layer's `entry` adapter
   already has the key rows, so this is a selector gap more than an adapter gap.
7. **Format readers are a common need, but concentrated.** 22 units in 7 repositories. 14 of
   them read Markdown frontmatter, in four repositories (strapi, trpc, backstage and
   Rocket.Chat). The rest are CSV (3), JSON5 (2), CODEOWNERS, a lockfile and a TypeScript data
   module.
8. **Change-aware checks need the pull request's base.** 10 units in 5 repositories:
   append-only files, renames that need redirects, release-branch migrations and one change file
   per package. A depth-1 clone cannot confirm them, and no kind compares content with a base
   revision.

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

## Batch 4 (2026-11-12): mattermost, nextcloud/server, thunderbird-android, fluentui, backstage

### The three families

- **`locale`** (60 units): 12 A, 7 B-config, 13 B-adapter, 3 B-delegate, 17 C, 8 D.
  - **mattermost alone has 37 locale units.** It has two catalogs, server go-i18n and webapp
    react-intl, plus a glossary and a style guide with their own checker:
    - B-config: duplicate ids and glossary forms (mattermost-03, -27, `unique-key`), and locale
      lists that must match the catalog files (mattermost-09, -28, -33, -b01, `set`);
    - B-adapter: placeholders (mattermost-06, -19), plural categories (-08, -10), ICU argument
      roles and escaping (-20, -21), and values of entries (-07, -32, -35);
    - C: JSON parse checks (mattermost-01, -13, -23). Also blank-value checks that cannot skip
      keys absent from en.json, which both checkers only warn about (mattermost-05, -14, -17).
  - **thunderbird-android runs Android lint over `strings.xml`.**
    - `key_parity` across 34 modules is A (thunderbird-android-01, -02). So are the cs, lt and
      sk plural subset (-09) and the legacy locale folder names (-06).
    - Duplicate resource names across a folder's files are B-config `unique-key` (-b01).
    - Plural categories (-03) and placeholder types (-08) are B-adapter.
    - Weblate's own checks are D (-b05).
  - **nextcloud's `translation-checker.php`** gives A units: an l10n directory and Transifex
    resource per app (nextcloud-01), and no RTL-limited characters in non-RTL catalogs (-02).
- **`dep-version`** (45 units): 13 A, 1 B-config, 4 B-adapter, 11 C, 16 D.
  - **backstage's React peer policy is six A units** (backstage-01 to -06). Its package-role
    dependency rules are A where the rule names its targets (-12, -13). They are C where the
    target's role lives in the target's manifest, so the targets are hand-listed (-08 to -11).
  - **fluentui:**
    - syncpack's one version per dependency (fluentui-17) and `*` for workspace dependencies
      (-18) need entry values;
    - peer-range compatibility (fluentui-31) needs version comparison;
    - valid peer ranges (-07) and the `tar-fs` floor (-08) are regex stand-ins (C).
  - nextcloud's override of a direct dependency (nextcloud-b02) needs entry values.
- **`toolchain-pin`** (21 units): 11 A, 1 B-adapter, 6 C, 3 D.
  - **mattermost's Node and Go pins** agree across `.nvmrc`, `mise.toml`, the buildenv
    Dockerfiles, the Cursor image and `package-lock.json` (mattermost-38 to -40, -b03, -b04,
    -b10).
  - **fluentui** runs Yarn from the committed release (fluentui-01, -02, -b01).
  - **backstage's C units need version comparison:**
    - `@types/node` follows the lowest supported major, which the rule reads from the first
      alternative of `engines.node` (backstage-15);
    - the CI matrices list the supported majors (-17).

### Stage B and integration

- **Stage B:** added 48 units, reclassified 28 and removed 7.
  - It removed checks nobody runs: nextcloud's opt-in pre-commit hooks (nextcloud-21, -22) and
    thunderbird-android's scheduled security scans (-68). It also removed a collation order the
    pin violates (-11).
  - It reproduced false positives that moved units from A to C:
    - mattermost-05, -14 and -17: extra keys;
    - fluentui-14: a digest that forbids the additions the file allows;
    - backstage-15, -46, -49, -53 and -55: range order, slug style and code blocks.
  - It moved two units the other way. thunderbird-android-62 is A with a select/require pair.
    thunderbird-android-05 moved from B-delegate to C.
- **Stage C** fixed the scope gaps Stage B reproduced:
  - backstage-14 now also checks the create-app templates;
  - backstage-17 now reads three more workflows;
  - nextcloud-01 and -14 now also cover a missing `.tx/config` and `psalm.xml`;
  - fluentui-02 now has a rule that `yarnPath` is present.
- **The integrator's changes:**
  - **`needs` tags for all 51 B-adapter units.** Eleven need entry values, with the assertion
    family each will need.
  - **fluentui-16 to C:** `*.api.md text=auto` keeps the committed blob LF, but alint reads the
    working tree, which is CRLF on a Windows checkout with `core.autocrlf`.
  - **mattermost-88 and fluentui-31 reasons rewritten** to match Stage B's B-adapter
    reclassification: base-revision and version comparison.
  - **Kept:**
    - mattermost-b10: the lockfile is a separate pin site that `mise.toml` names;
    - backstage-b06 as C: the OpenAPI meta-schema is not in the clone (the rushstack-09b
      precedent);
    - fluentui-b16 as A. That Nx rejects an unknown implicit dependency was not confirmed
      offline.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  4 `info` findings from value-add rules (mattermost 1, nextcloud 1, backstage 2).
  `xtask corpus-replay --only` replays all five.

### Sharp edges (C)

- **A value check cannot be scoped to keys the reference file has.** mattermost's checkers skip
  ids absent from en.json, and alint's rules apply to every entry (mattermost-05, -14, -17).
- **Raw-text kinds read Markdown code blocks.** backstage's link checker strips them first
  (backstage-49, -53, -55). `markdown_fragments_resolve` has one slug style, GitHub's
  (backstage-46).
- **Working-tree line endings depend on the checkout** when `.gitattributes` does not pin
  `eol` (fluentui-16).
- **The lowest major of a range** needs version comparison (backstage-15).
