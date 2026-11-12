# v0.23 corpus stratum: log

Status: batches 1 and 2 of 4 complete (2026-11-12). Frame and method:
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

Through batch 2 (10 repositories).

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
| **Pooled** | 714 | 247 | 24 | 57 | 1 | 9 | 69 | 307 | 61% | 78% | 22% | 65% | 66% |

| Family | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 48 | 7 | 1 | 12 | 0 | 2 | 13 | 13 | 20% | 57% | 43% | 23% | 23% |
| `dep-version` | 79 | 15 | 5 | 10 | 0 | 0 | 9 | 40 | 38% | 62% | 38% | 46% | 59% |
| `toolchain-pin` | 31 | 15 | 0 | 9 | 0 | 0 | 0 | 7 | 62% | 62% | 38% | 62% | 62% |
| `other` | 556 | 210 | 18 | 26 | 1 | 7 | 47 | 247 | 68% | 83% | 17% | 72% | 72% |

The assertion families the B-config units need, with the B-adapter units that need only entry
values:

| Assertion family | B-config | B-adapter, entry values | `locale` | `dep-version` | `toolchain-pin` | `other` |
|---|--:|--:|--:|--:|--:|--:|
| `unique-key` | 14 | 5 | 0 | 8 | 0 | 11 |
| `reference` | 4 | 2 | 0 | 2 | 0 | 4 |
| `set` | 3 | 1 | 1 | 1 | 0 | 2 |
| `cardinality` | 2 | 0 | 1 | 0 | 0 | 1 |
| `bounded-count` | 1 | 0 | 0 | 0 | 0 | 1 |
| `equality` | 0 | 1 | 0 | 0 | 0 | 1 |
| `none` | 0 | 1 | 1 | 0 | 0 | 0 |

The per-repository median `coverage_today` is 52%. zulip contributes 188 of the 714 units; 108 of
its A units are custom regex rules in its own linter, each a unit under R1. The family tables are
the stratum's measure; the pooled figure is context.

## Signals so far

1. **The slice's dependency version policy needs a value-bearing entry adapter.**
   - "One range per dependency across workspaces" is enforced in all five JavaScript monorepos so
     far: trpc-05, yarnpkg-berry-01, tq-10, jest-23 and rushstack-01.
   - Each needs (dependency name, range) rows. No adapter emits them today: `entry`, TOML and XML
     are key-only, and `extract` drops the pointer (batch 2 ruling).
   - With that adapter, `dep-version` moves from 38% today to 59% after the slice. Without it,
     the move stops at 46%.
2. **Version comparison is the most requested operator:** 11 units in 6 repositories.
   - It covers range satisfaction, ordering and range validity: forem-07, element-web-13,
     trpc-03, trpc-06, immich-11, -52 and -53, tq-03, rushstack-06, -55 and -b11.
   - These 11 include all 9 `toolchain-pin` B-adapter units. The pins that must agree are
     already A; the rest is "this exact pin satisfies that range". So the slice leaves
     `toolchain-pin` at 62% unless it adds the operator.
3. **`unique-key` leads the assertion families**, with 14 B-config units and 5 more once entry
   values exist. Workspace names that must be unique (trpc-B05, jest-B02, tq-35) are the common
   B-config case: the values are there, and alint has no uniqueness relation over them.
4. **The locale family barely moves with the slice:** from 20% to 23%. Its B units need format
   knowledge, not new assertion families:

   | Need | Units |
   |---|---|
   | Placeholders kept per key | forem-04, zulip-02, immich-05 |
   | ICU message structure | signal-desktop-06, -07, -09 |
   | Message syntax (B-delegate) | zulip-01 (gettext), signal-desktop-10 (ICU) |
   | Plural categories | forem-03 |
   | Nested key order | element-web-02, immich-01 |
   | Values of entries | element-web-05, -06 |
   | Unicode normalization | zulip-05 |

   Its 13 C units are:
   - regex approximations of ICU rules (5);
   - JSON parse checks that alint relaxes (4);
   - key-name regexes (3);
   - a `.po` header check.
5. **Predicates over JSON key names recur:**
   - expressed today only as layout-dependent line regexes (C): signal-desktop-01, element-web-04,
     immich-49;
   - or not at all: rushstack-b12, B-adapter `key-predicate`.

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
