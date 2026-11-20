# v0.23 corpus stratum: log

Status: complete (2026-11-12; reviewed 2026-11-13): 20 repositories in four batches, then an
audit of every classification and a review of the records. Frame and method:
[`corpus-stratum.md`](corpus-stratum.md).

Each repository has a record in [`stratum/`](stratum/), named by its slug. A record holds:
- every unit, with its evidence, mechanism, family, bucket, assertion family, the rules that check
  it, its reason, and the changes Stage B, the integrator, the audit or the review made to it;
- the units removed and the Stage B suggestions rejected, each with the reason;
- the value-add rules and their ids, absent categories and the "unusual" scan;
- Stage B's configuration issues and summary;
- Stage C's configuration changes and the final `validate-config` and `check` result.

Each configuration is in [`../v0.12/corpus/`](../v0.12/corpus/), with its row in `pins.tsv`.
`cargo test -p xtask` checks every record against its pin and configuration: each rule a unit
names exists, each rule belongs to a unit or is value-add, and the metrics recompute.

The coverage figures apply R7, so they are not comparable with v0.12's 61%
([`corpus-stratum.md`](corpus-stratum.md) section 2.1). The two "after" columns:
- **After slice** counts a B-config unit as A when its assertion family is `equality`,
  `unique-key` or `cardinality`.
- **With values** also counts a B-adapter unit that needs only value-bearing entry rows
  (`needs: entry-values`) and one of those three families.

Phase 8 confirms each such unit before either figure is final.

## Totals

All 20 repositories, 1,558 units. The percentages exclude D: Today is A / (A + B + C).

| Repository | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| forem/forem | 58 | 20 | 3 | 6 | 0 | 0 | 7 | 22 | 56% | 75% | 25% | 61% | 64% |
| zulip/zulip | 190 | 111 | 2 | 3 | 0 | 4 | 12 | 58 | 84% | 93% | 7% | 86% | 86% |
| element-hq/element-web | 49 | 4 | 0 | 4 | 0 | 0 | 7 | 34 | 27% | 73% | 27% | 27% | 33% |
| yarnpkg/berry | 43 | 13 | 1 | 2 | 0 | 0 | 6 | 21 | 59% | 86% | 14% | 59% | 64% |
| trpc/trpc | 65 | 9 | 6 | 14 | 0 | 1 | 6 | 29 | 25% | 42% | 58% | 36% | 39% |
| signalapp/Signal-Desktop | 74 | 12 | 0 | 2 | 0 | 1 | 13 | 46 | 43% | 89% | 11% | 43% | 43% |
| immich-app/immich | 55 | 10 | 1 | 10 | 0 | 0 | 5 | 29 | 38% | 58% | 42% | 38% | 38% |
| TanStack/query | 54 | 14 | 2 | 6 | 0 | 0 | 9 | 23 | 45% | 74% | 26% | 52% | 58% |
| jestjs/jest | 55 | 22 | 3 | 3 | 0 | 0 | 4 | 23 | 69% | 81% | 19% | 75% | 78% |
| microsoft/rushstack | 78 | 30 | 6 | 7 | 0 | 2 | 17 | 16 | 48% | 76% | 24% | 58% | 60% |
| TryGhost/Ghost | 101 | 36 | 2 | 18 | 0 | 0 | 27 | 18 | 43% | 76% | 24% | 45% | 45% |
| RocketChat/Rocket.Chat | 86 | 22 | 1 | 19 | 0 | 0 | 7 | 37 | 45% | 59% | 41% | 47% | 47% |
| babel/babel | 68 | 27 | 2 | 5 | 0 | 3 | 6 | 25 | 63% | 77% | 23% | 65% | 70% |
| strapi/strapi | 78 | 16 | 5 | 7 | 0 | 0 | 24 | 26 | 31% | 77% | 23% | 38% | 40% |
| cloudflare/workers-sdk | 83 | 31 | 1 | 9 | 1 | 0 | 6 | 35 | 65% | 77% | 23% | 65% | 65% |
| mattermost/mattermost | 122 | 33 | 7 | 21 | 0 | 2 | 19 | 40 | 40% | 63% | 37% | 44% | 48% |
| nextcloud/server | 53 | 11 | 0 | 4 | 0 | 0 | 8 | 30 | 48% | 83% | 17% | 48% | 52% |
| thunderbird/thunderbird-android | 68 | 17 | 0 | 8 | 0 | 1 | 16 | 26 | 40% | 79% | 21% | 40% | 43% |
| microsoft/fluentui | 77 | 20 | 4 | 8 | 0 | 0 | 10 | 35 | 48% | 71% | 29% | 55% | 60% |
| backstage/backstage | 101 | 29 | 3 | 12 | 0 | 2 | 30 | 25 | 38% | 78% | 22% | 39% | 42% |
| **Pooled** | 1,558 | 487 | 49 | 168 | 1 | 16 | 239 | 598 | 51% | 76% | 24% | 54% | 56% |

| Family | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice | With values |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 134 | 25 | 9 | 36 | 0 | 5 | 39 | 20 | 22% | 56% | 44% | 26% | 29% |
| `dep-version` | 170 | 45 | 5 | 23 | 0 | 0 | 31 | 66 | 43% | 73% | 27% | 46% | 57% |
| `toolchain-pin` | 81 | 41 | 0 | 13 | 0 | 0 | 11 | 16 | 63% | 80% | 20% | 63% | 63% |
| `other` | 1,173 | 376 | 35 | 96 | 1 | 11 | 158 | 496 | 56% | 79% | 21% | 59% | 60% |

The per-repository median `coverage_today` is 45% (range 25% to 84%). zulip contributes 190 units;
87 of its 111 A units are rules in its own regex linter (`tools/linter_lib/custom_check.py`), each
a unit under R1. The family tables are the stratum's measure; the pooled figure is context.

### By stratum

| Stratum | Family | Units | Today | After slice | With values |
|---|---|--:|--:|--:|--:|
| i18n | `locale` | 129 | 22% | 26% | 28% |
| i18n | `dep-version` | 57 | 50% | 50% | 55% |
| i18n | `toolchain-pin` | 40 | 65% | 65% | 65% |
| i18n | `other` | 630 | 62% | 64% | 65% |
| i18n | **all** | 856 | 53% | 55% | 57% |
| JavaScript monorepo | `locale` | 5 | 20% | 40% | 40% |
| JavaScript monorepo | `dep-version` | 113 | 41% | 45% | 57% |
| JavaScript monorepo | `toolchain-pin` | 41 | 62% | 62% | 62% |
| JavaScript monorepo | `other` | 543 | 48% | 54% | 55% |
| JavaScript monorepo | **all** | 702 | 48% | 53% | 56% |

The JavaScript monorepos' 5 `locale` units are incidental; the stratum's locale evidence is the
i18n half's 129.

### What each capability would add

Each step adds one capability to the previous ones and counts the units it would move to A. The
first two steps are the "after slice" and "with values" columns.

| Step | `locale` | `dep-version` | `toolchain-pin` | `other` | Pooled |
|---|--:|--:|--:|--:|--:|
| Today | 22% | 43% | 63% | 56% | 51% |
| + slice | 26% | 46% | 63% | 59% | 54% |
| + entry values | 29% | 57% | 63% | 60% | 56% |
| + version comparison | 29% | 60% | 83% | 61% | 58% |
| + `set` and `reference` | 35% | 67% | 83% | 63% | 61% |
| + locale format adapters | 53% | 67% | 83% | 63% | 63% |

- **+ slice:** B-config units in `equality`, `unique-key` or `cardinality`.
- **+ entry values:** B-adapter units with `needs: entry-values` in one of those families.
- **+ version comparison:** B-adapter units with `needs: version-compare`.
- **+ `set` and `reference`:** B-config units in those families, and entry-values units that
  need them.
- **+ locale format adapters:** B-adapter units that need `placeholder`, `plural-categories` or
  `icu`.

ADR-0033 (2026-11-13) swapped `cardinality` for `set` in the slice and added entry values to it.
Under that slice, with forem-01 counted as per-locale set equality, the "+ slice" row reads 30%,
48%, 63%, 60% and 55%, and the "+ entry values" row 35%, 61%, 63%, 61% and 58%. The rows above
keep ADR-0032's slice, as the records do.

### Assertion families

The assertion families the B-config units need, with the B-adapter units that need only entry
values. The four family columns split the sum of the first two.

| Assertion family | B-config | B-adapter, entry values | locale | dep-version | toolchain-pin | other |
|---|--:|--:|--:|--:|--:|--:|
| `unique-key` | 31 | 11 | 4 | 13 | 0 | 25 |
| `set` | 11 | 6 | 7 | 4 | 0 | 6 |
| `reference` | 5 | 6 | 0 | 4 | 0 | 7 |
| `equality` | 0 | 9 | 3 | 1 | 0 | 5 |
| `group` | 1 | 0 | 0 | 0 | 0 | 1 |
| `cardinality` | 1 | 0 | 1 | 0 | 0 | 0 |

No unit needs `bounded-count` since the audit moved forem-22 to C.

### What the B-adapter units need

Each B-adapter unit records the one capability it lacks most directly (frame section 2.2):

| Need | Units | Repositories | `locale` | `dep-version` | `toolchain-pin` | `other` |
|---|--:|--:|--:|--:|--:|--:|
| `entry-values` | 32 | 16 | 6 | 17 | 0 | 9 |
| `version-compare` | 21 | 12 | 0 | 3 | 13 | 5 |
| `order` | 20 | 12 | 8 | 0 | 0 | 12 |
| `format-reader` | 19 | 7 | 0 | 2 | 0 | 17 |
| `base-revision` | 15 | 7 | 0 | 0 | 0 | 15 |
| `other` | 13 | 10 | 1 | 1 | 0 | 11 |
| `placeholder` | 11 | 7 | 11 | 0 | 0 | 0 |
| `route-model` | 9 | 6 | 0 | 0 | 0 | 9 |
| `derived-value` | 8 | 6 | 0 | 0 | 0 | 8 |
| `plural-categories` | 7 | 5 | 7 | 0 | 0 | 0 |
| `package-graph` | 5 | 5 | 0 | 0 | 0 | 5 |
| `link-type` | 3 | 3 | 0 | 0 | 0 | 3 |
| `normalization` | 3 | 3 | 1 | 0 | 0 | 2 |
| `icu` | 2 | 2 | 2 | 0 | 0 | 0 |
| **Total** | 168 | | 36 | 23 | 13 | 96 |

No unit needs `key-predicate` since the audit moved rushstack-b12 to C.

## Findings

1. **`unique-key` is the assertion family with the most demand; `cardinality` has little.**
   - `unique-key` has 31 B-config units and 11 more once entry values exist. The common cases:
     - unique workspace names (trpc-B05, jest-B02, tq-35, babel-B01);
     - duplicate keys or ids in one file: a locale file (rocketchat-10, mattermost-03), a plugin
       manifest (rushstack-b14) or any linted YAML file (zulip-28);
     - disjoint dependency fields (trpc-08, jest-24, rushstack-51, and babel-12 once entry values
       exist).
   - `cardinality` has 1. Locale key parity is mostly A already, through `key_parity`. The one
     locale unit left for the slice's group-parity form is forem-01, where each locale's keys
     are merged from several files. forem's two count bounds (forem-22, -b03) are C: a
     layout-dependent regex expresses them.
   - `equality` has no B-config unit. Its 9 units all need entry values first.
   - `set` (11 B-config, 6 entry values) and `reference` (5 and 6) come next. Neither is in the
     slice. Together they would move 7 `locale` units (6.1 points), 8 `dep-version` units (7.7
     points) and 28 units pooled (2.9 points).
2. **The slice's dependency version policy needs a value-bearing entry adapter.**
   - One range per dependency across workspaces is enforced in 6 of the 10 JavaScript monorepos:
     trpc-05, yarnpkg-berry-01, tq-10, jest-23, rushstack-01 and fluentui-17.
   - The other four enforce something narrower or nothing:
     - strapi compares exact versions only (strapi-02);
     - babel requires a catalog for a shared dependency, and its named catalogs may differ
       (babel-11);
     - workers-sdk requires `catalog:` only for names already in its catalog (wsdk-09);
     - backstage has 70 dependencies with more than one range at the pin.
   - All of these need (dependency name, range) rows. No adapter emits them: `entry`, TOML and
     XML are key-only, and `extract` drops the pointer.
   - `entry-values` is the most common need: 32 B-adapter units in 16 of the 20 repositories.
   - With it, `dep-version` goes from 43% today to 57%. The slice alone reaches 46%.
3. **Version comparison is the operator with the most demand.**
   - 21 B-adapter units in 12 repositories need range satisfaction, ordering or range validity.
     They include all 13 `toolchain-pin` B-adapter units.
   - The pins that must agree are already A; what remains is "this exact pin satisfies that
     range". With the operator, `toolchain-pin` goes from 63% to 83%. Without it, the slice
     leaves the family where it is.
   - More units are C because a regex or a normalization stands in for the operator. Examples
     are Ghost's version ceilings (ghost-27, -34, -35, -36), rushstack's pnpm floor and Node
     range validity (rushstack-07, -b11), fluentui-07, rocketchat-17, thunderbird-android-b04
     and backstage-15.
4. **The locale family needs format knowledge, not new assertion families.**
   - It goes from 22% to 26% with the slice, and 29% with entry values.
   - Its 41 B-adapter and B-delegate units, by need:

     | Need | Units | Repositories |
     |---|--:|--:|
     | Placeholders kept per key | 11 | 7 |
     | Key or element order | 8 | 6 |
     | Plural categories per locale | 7 | 5 |
     | Values of entries | 6 | 2 |
     | Message syntax (B-delegate) | 5 | 4 |
     | ICU message structure | 2 | 2 |
     | Unicode normalization | 1 | 1 |
     | Every resource name in a folder | 1 | 1 |

   - Adapters for placeholders, plural categories and ICU would take `locale` from 35% to 53%,
     after the steps before them in the capability table.
   - A regex over each message stands in for the ICU syntax tree in three Signal rules
     (signal-desktop-07, -08, -09, all C).
5. **A strict-JSON option would turn C units into A.** alint's JSON reader accepts comments and
   trailing commas, which `JSON.parse`, i18next and rollup's `json()` reject. 10 locale C units in
   8 repositories approximate a strict parse check because of this, such as zulip-04, ghost-10,
   rocketchat-11 and mattermost-01.
6. **JSONPath cannot select member names.** Map keys, such as catalog entries, dependency names
   chosen by a name set and JSON key names, are read with layout-dependent line regexes (C):
   strapi-06, wsdk-74, thunderbird-android-53, signal-desktop-01, ghost-11 and rushstack-b12. The
   relation layer's `entry` adapter already has the key rows, so this is a selector gap more than
   an adapter gap.
7. **Format readers are a common need, but concentrated.** 19 units in 7 repositories. 10 of
   them read Markdown frontmatter, in four repositories (trpc, strapi, backstage and
   Rocket.Chat), and need a list, a nested block or a schema; a single-line frontmatter value is
   read by regex (C). The rest are CSV (3), TypeScript data modules (2), JSON5, CODEOWNERS, a
   lockfile and Markdown tables.
8. **Change-aware checks need the pull request's base.** 15 units in 7 repositories: append-only
   files, an inventory that only shrinks, renames that need redirects, migrations ordered against
   a release or another branch, release-branch guards and one change file per package. A depth-1
   clone cannot confirm them, and no kind compares content with a base revision.

## Batch 1 (2026-11-12): forem, zulip, element-web, yarnpkg/berry, trpc

The counts below are after the audit. Batch 2's rulings moved trpc-05, yarnpkg-berry-01, trpc-10,
forem-b08 and forem-29 to B-adapter (`needs: entry-values`), and trpc-08 from `set` to
`unique-key`.

### The three families

- **`locale`** (22 units, all in the three i18n repositories): 3 A, 5 C, 1 B-config, 7
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
  - **The B-delegate unit:** gettext's own `.po` validation (zulip-01).
  - **The C units:**
    - two parse checks that alint relaxes (zulip-04, element-web-08);
    - a key charset that holds for one layout only (element-web-04);
    - a `.po` header check (zulip-07);
    - the supported locales equal the locale files that ship (forem-05). The audit moved it from
      A: a `cross_file` set source takes one `files` glob, so the rule needs a layout-dependent
      regex to skip dotted file stems.
- **`dep-version`** (30 units): 6 A, 4 C, 2 B-config, 5 B-adapter, 13 D.
  - **B-config:** peer dependencies also in dev dependencies, as `set` (trpc-07); a dependency
    in only one dependency type, as `unique-key` (trpc-08).
  - **B-adapter:**
    - one range per dependency (trpc-05, yarnpkg-berry-01) and catalog specifiers that resolve
      (trpc-10), which need entry values;
    - a semver range or version comparison (forem-07, trpc-06).
  - **C:** the subtree-synced examples' `workspace:` exceptions (trpc-11, which the audit moved
    from A because a regex over the specifier stands in for the name list), `workspace:*`
    outside peer dependencies (trpc-12), `workspace:^` ranges (yarnpkg-berry-02) and zulip's
    provision version bump (zulip-15).
  - **D:** mostly lockfile freshness and `dedupe --check`, which need the package manager's
    resolution. The audit moved minimum release age to `other` (frame section 2.2).
- **`toolchain-pin`** (12 units): 7 A, 2 B-adapter, 3 D.
  - **A:**
    - pins that agree: `.ruby-version` and the base image, `engines.node` and the
      `yarn.config.cjs` floor, the CI matrix and `engines`;
    - pins that are present and well formed: `.nvmrc`, `packageManager`, `yarnPath`;
    - zulip's pnpm pin (zulip-18), which the audit moved from D: the pin and its use are files
      alint reads.
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

1. **A nested `key_parity` stops with an internal error** ([#502]): "key_parity requires the
   relation read cache". The run enables its relation caches only when a top-level rule uses
   relations, and the nesting kinds don't report their nested rules. It reproduces under
   `for_each_file`, `for_each_dir` and `every_matching_has` with a one-file reference and peer.
   A top-level `key_parity` in the same config masks it until `--only` builds a one-rule engine.
2. **`git_tracked_only: true` misses force-added ignored files** ([#503]). Under the default
   `respect_gitignore: true`, `file_absent` and `dir_absent` stay silent on a file that is
   force-added but excluded by `.gitignore`, and `file_exists` and `dir_exists` report it
   missing.
   - `docs/rules.md` and `concepts/targeting/the-walker-and-git.md` say it fires; it fires only
     with `respect_gitignore: false`.
   - It reproduces with the walker page's own `target/` example. Bundled `java@v1` rules and a
     shipped example have the same blind spot.
3. **A templated `reference:` in a nested `key_parity` fails to load** ([#504]). Nested rules
   are built with the placeholder path `_`, so `{path}` has no extension and format inference
   rejects the config. The audit found it while reproducing #502.

[#502]: https://github.com/asamarts/alint/issues/502
[#503]: https://github.com/asamarts/alint/issues/503
[#504]: https://github.com/asamarts/alint/issues/504

### Sharp edges

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

- **`locale`** (24 units): 3 A, 11 C, 3 B-adapter, 1 B-delegate, 6 D.
  - **Signal-Desktop's ICU linter alone gives 10 units:**
    - eight rules alint approximates with a regex over each message are C (signal-desktop-01 to
      -05 and -07 to -09). The audit moved -07 and -09 from B-adapter, because a regex over each
      message expresses them as it does -08;
    - the emoji rule needs the ICU syntax tree and is B-adapter (signal-desktop-06);
    - the parse step itself is B-delegate (signal-desktop-10).
  - **Moved to `other` by the audit:** a ban on hard-coded UI strings in source
    (signal-desktop-14) and a ban on untyped translation helpers in Dart (immich-08). Both read
    source, not locale files.
  - **immich:**
    - recursive key order (immich-01) and placeholder overrides (immich-05) are B-adapter;
    - strict JSON (immich-03) and Dart-safe key names (immich-49) are C.
- **`dep-version`** (47 units): 7 A, 7 C, 2 B-config, 6 B-adapter, 25 D.
  - **A:**
    - `workspace:` and `patch:` specifiers that resolve (jest-B01, jest-B03, tq-14);
    - dev-only dependencies (jest-10);
    - pinned specifiers (signal-desktop-16).
  - **B-config:** disjoint dependency fields (jest-24, rushstack-51).
  - **B-adapter:**
    - one range per dependency (tq-10, jest-23, rushstack-01);
    - sherif's similar-package families (tq-11, `unique-key` once entry values exist);
    - approved packages (rushstack-02, `reference`). The audit moved it from B-config: an
      `npm:` alias must resolve to its target name, which needs the dependency's value;
    - lockfile rows (signal-desktop-18).
  - **C:** among others, sherif's empty-dependencies check (tq-12). The audit moved it from A,
    because its regex also matches a nested empty map that sherif ignores.
  - **Release age** (signal-desktop-21, tq-17, immich-17) moved to `other`, and two install-time
    pnpm settings (signal-desktop-43, -44) moved from `other` to `dep-version`.
- **`toolchain-pin`** (18 units): 8 A, 2 C, 6 B-adapter, 2 D.
  - **B-adapter:** every unit is a version comparison: range satisfaction (immich-11, -52, -53,
    tq-03, rushstack-06) and ordering (rushstack-55).
  - **C:** rushstack's pnpm floor (rushstack-07, from A) and its Node range validity
    (rushstack-b11, from B-adapter). A regex expresses each; a fixed floor and a range grammar
    are regular, so they are awkward rather than missing.
  - **jest-36 moved to `other`:** it compiles under a TypeScript version rather than comparing
    pins.

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
  - **tq-B05 to D:** the npm registry checks the repository URL at publish time. Its rule stays
    as a partial rule at level `info`.
  - **tq-B04 and jest-27 to C:** both are regex reads that hold for one layout.
  - **rushstack:**
    - rushstack-b09 to A, which Stage C expressed;
    - rushstack-b12 to B-adapter, a predicate over key names (the audit moved it to C: a
      layout-dependent regex over the key lines expresses it);
    - rushstack-c01 added, the plugin-command check that rushstack-09b's rule really expresses.
  - **Kept:**
    - jest-18, -21 and -28: the Docusaurus build fails the Netlify production deploy as well as
      the deploy preview;
    - jest-12: the changelog check runs on pull requests;
    - signal-desktop-b03: the beta-build check runs in CI for every beta version.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  immich's 271 `info` findings from its value-add stale-key rule. `xtask corpus-replay --only`
  replays all five.

### Sharp edges

- `json_schema_passes` takes no inline schema. A schema the repository does not ship needs a
  shim schema outside the repository, as in batch 1's yarnpkg-berry-23, so such a unit is C
  (rushstack-09b).
- `unique_by` keys on path templates only, and `cross_file` has no uniqueness relation. Unique
  workspace names therefore wait for the slice's `unique-key` (trpc-B05, jest-B02, tq-35).

## Batch 3 (2026-11-12): Ghost, Rocket.Chat, babel, strapi, workers-sdk

### The three families

- **`locale`** (32 units): 10 A, 2 B-config, 11 B-adapter, 7 C, 2 D.
  - **Ghost:**
    - key parity over five namespaces with CLDR plural groups is A (ghost-02). So are the
      locale directories against `locale-data.json` (ghost-05) and a 255-character cap on one
      translation that becomes a Stripe field label (ghost-06);
    - the plural categories each locale must carry (ghost-03) and its linter's placeholder rules
      (ghost-12, -13, -14) are B-adapter;
    - `context.json`, regenerated from the en key union in canonical order and compared byte
      for byte, is one B-adapter unit (ghost-07, key order);
    - the shape of `i18n-ignores.json` (ghost-15) moved from A to C in the audit. JSONPath
      cannot tell an empty object from an empty array, so the rule misses a case the check
      catches.
  - **Rocket.Chat's `i18n check` task gives 12 locale units:**
    - extra keys, legacy `__placeholder__` values, trailing whitespace and the file set are A
      (rocketchat-03, -06, -07, -12);
    - key order (rocketchat-01, -02), plural categories (-04, -05) and placeholders (-08, -09)
      are B-adapter;
    - duplicate keys are B-config `unique-key` (rocketchat-10);
    - strict JSON with an object at the root is C (rocketchat-11).

    The base locale the i18n build requires is A too (rocketchat-82). The audit moved
    Rocket.Chat's ban on hard-coded strings in client code (rocketchat-14) to `other`.
  - **strapi does not enforce locale key parity.** `scripts/front/verify-translations` is not in
    CI, and 99 of 261 locale files have extra keys at the pin (R5). Its locale units are narrow
    tests: strapi-22 is A, strapi-21 (no `X.singular` and `X.plural` pair) is B-config
    `unique-key`, and three are C.
- **`dep-version`** (46 units): 18 A, 7 B-adapter, 10 C, 11 D.
  - **A:**
    - babel's `yarn.config.cjs` rules on `workspace:^`, `@babel/core` placement and peer ranges
      (babel-01, -13 to -16);
    - `workspace:` and `patch:` specifiers that resolve (babel-18, -19, rocketchat-25, -26,
      strapi-05, wsdk-73);
    - exact catalog and public-package pins (wsdk-10, -11, strapi-03);
    - allowed `yarn.lock` resolution schemes (strapi-04).
  - **B-adapter:** six units need entry values:
    - exact versions that agree (strapi-02) and a catalog for shared dependencies (babel-11);
    - a `catalog:` specifier that resolves (babel-17) and the catalog-or-pin rule (wsdk-09);
    - an allowlist of a package's non-workspace dependencies (wsdk-06);
    - disjoint dependency fields with an exemption that depends on the range (babel-12). The
      audit moved it from B-config.

    wsdk-07 reads one array of a TypeScript module.
  - **C:**
    - in Ghost, two version ceilings hand-encoded as regexes (ghost-35, -36), a major.minor
      equality (ghost-30), and `workspace:*` for compliant internal packages (ghost-32);
    - in strapi, three `catalog:` or lockstep checks that rest on a line regex or a hand-kept
      list.

    The audit moved ghost-32 from B-adapter: a line regex over hand-listed workspace names
    expresses it, as for yarnpkg-berry-02, trpc-12, rushstack-03, strapi-07 and backstage-07.
    It moved Ghost's Tinybird CLI ceiling (ghost-34) to `toolchain-pin`, next to the pin it
    bounds, and Rocket.Chat's release-age gate (rocketchat-23) to `other`.
- **`toolchain-pin`** (28 units): 15 A, 4 B-adapter, 3 C, 6 D.
  - Ghost alone has 7 A units. Its Node pins (CI, `devEngines`, `engines`, the Dockerfile,
    `.nvmrc`, `.node-version`) move together, `@types/node` follows the Node major,
    `packageManager` is exact, and the Tinybird CLI version agrees in two files.
  - All four B-adapter units are version comparison (rocketchat-16, -80, wsdk-55, -56).
  - The C units are version ceilings or comparisons spelled as regexes (ghost-27, -34,
    rocketchat-17).

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
    ghost-32 and strapi-02 move to B-adapter with it. Each keeps its assertion family. (The
    audit later moved ghost-32 to C and babel-12 to B-adapter.)
  - **Release age is `other`:** agreement between two release-age settings is not a dependency
    version (section 2.3), as jest-14 was. This moves ghost-b14, ghost-b15 and rocketchat-24.
  - **strapi-07 to C:** its pinned-exact half needs a hand-kept list of 43 workspace names.
  - **rocketchat-76 to -79 stay A, as a convention:** `docs/npm-publishing.md` states each
    requirement. tq-B05 went to D in batch 2 because nothing in that repository stated it.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  4 `info` findings from value-add rules (Rocket.Chat 3, babel 1) and 2 notes from Ghost's
  key-extraction rule. `xtask corpus-replay --only` replays all five.

### Sharp edges

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

- **`locale`** (56 units): 9 A, 6 B-config, 15 B-adapter, 3 B-delegate, 16 C, 7 D.
  - **mattermost alone has 37 locale units.** It has two catalogs, server go-i18n and webapp
    react-intl, plus a glossary and a style guide with their own checker:
    - B-config: duplicate ids and glossary forms (mattermost-03, -27, `unique-key`), and locale
      lists that must match the catalog files (mattermost-09, -28, -33, -b01, `set`);
    - B-adapter: placeholders (mattermost-06, -19), plural categories (-08, -10), ICU argument
      roles (-20), key order (-16, -30), and values of entries (-07, -21, -32, -35). The audit
      moved -16 from D: the catalogs must be exactly `JSON.stringify` of the sorted keys, and
      the formatting half alone would be D. It moved -21 (no new unescaped apostrophe) from
      `icu` to entry values with `set`, since it compares each translation with its source
      text;
    - C: JSON parse checks (mattermost-01, -13, -23). Also blank-value and string-type checks
      that cannot skip keys absent from en.json, which both checkers only warn about
      (mattermost-05 and -17 for blank values, -14 for the type).
  - **thunderbird-android runs Android lint over `strings.xml`.**
    - 34 rules over the modules' string files (32 `key_parity`, 2 `cross_file`) are A
      (thunderbird-android-01, -02). So are the cs, lt and
      sk plural subset (-09) and the legacy locale folder names (-06).
    - Duplicate resource names across a folder's files are B-adapter (-b01). The audit moved it
      from B-config: the XML adapter emits only `string`, `string-array` and `plurals`, so it
      needs an adapter that emits every resource name before `unique-key` applies.
    - Plural categories (-03) and placeholder types (-08) are B-adapter.
    - Weblate's own checks are D (-b05).
    - Android lint's HardcodedText (thunderbird-android-07) moved to `other` in the audit.
  - **nextcloud's `translation-checker.php`** gives A units: an l10n directory and Transifex
    resource per app (nextcloud-01), and no RTL-limited characters in non-RTL catalogs (-02).
    The ellipsis check over `t()` and `n()` calls in source (nextcloud-04) moved to `other`.
- **`dep-version`** (47 units): 14 A, 1 B-config, 5 B-adapter, 10 C, 17 D.
  - **backstage's React peer policy is six A units** (backstage-01 to -06). Its package-role
    dependency rules are A where the rule names its targets (-12, -13). They are C where the
    target's role lives in the target's manifest, so the targets are hand-listed (-08 to -11).
  - **fluentui:**
    - syncpack's one version per dependency (fluentui-17) and `*` for workspace dependencies
      (-18) need entry values;
    - peer-range compatibility (fluentui-31) needs version comparison;
    - valid peer ranges (-07) and the `tar-fs` floor (-08) are regex stand-ins (C);
    - a committed file for every `patch:` descriptor (fluentui-06) moved from C to A in the
      audit: the regex takes the path from the descriptor's value, as babel-18 and jest-B03 do.
  - nextcloud's override of a direct dependency (nextcloud-b02) needs entry values.
  - **backstage's patch checks:** the audit added backstage-a01 (one source descriptor per patch
    file, B-adapter `other`), which it split out of the D unit backstage-60. It moved
    backstage-60 and -61 from `other` to `dep-version`, and fluentui's release-age quarantine
    (fluentui-30) to `other`.
- **`toolchain-pin`** (23 units): 11 A, 1 B-adapter, 6 C, 5 D.
  - **mattermost's Node and Go pins** agree across `.nvmrc`, `mise.toml`, the buildenv
    Dockerfiles, the Cursor image and `package-lock.json` (mattermost-38 to -40, -b03, -b04,
    -b10).
  - **fluentui** runs Yarn from the committed release (fluentui-01, -02, -b01).
  - **backstage's C units need version comparison:**
    - `@types/node` follows the lowest supported major, which the rule reads from the first
      alternative of `engines.node` (backstage-15);
    - the CI matrices list the supported majors (-17).
  - **D:** the audit moved the Gradle wrapper and distribution checksums (thunderbird-android-31,
    -32) and fluentui's package-manager guard (fluentui-56) to `toolchain-pin`, and the
    TypeScript-floor compile (fluentui-32) to `other`.

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
  - **`needs` tags for every B-adapter unit,** 51 at the time (53 after the audit, 12 of them
    needing entry values), each entry-values unit with the assertion family it will need.
  - **fluentui-16 to C:** `*.api.md text=auto` keeps the committed blob LF, but alint reads the
    working tree, which is CRLF on a Windows checkout with `core.autocrlf`.
  - **mattermost-88 and fluentui-31 reasons rewritten** to match Stage B's B-adapter
    reclassification: base-revision and version comparison.
  - **Kept:**
    - mattermost-b10: the lockfile is a separate pin site that `mise.toml` names;
    - backstage-b06 as C: the OpenAPI meta-schema is not in the clone (the rushstack-09b
      precedent). The audit moved it to B-delegate: swagger-parser also resolves `$ref`s and
      checks OpenAPI semantics beyond the schema;
    - fluentui-b16 as A. That Nx rejects an unknown implicit dependency was not confirmed
      offline.
- **Validation.** Every configuration validates and passes on its clone. The only findings are
  4 `info` findings from value-add rules (mattermost 1, nextcloud 1, backstage 2).
  `xtask corpus-replay --only` replays all five.

### Sharp edges

- **A value check cannot be scoped to keys the reference file has.** mattermost's checkers skip
  ids absent from en.json, and alint's rules apply to every entry (mattermost-05, -14, -17).
- **Raw-text kinds read Markdown code blocks.** backstage's link checker strips them first
  (backstage-49, -53, -55). `markdown_fragments_resolve` has one slug style, GitHub's
  (backstage-46).
- **Working-tree line endings depend on the checkout** when `.gitattributes` does not pin
  `eol` (fluentui-16).
- **The lowest major of a range** needs version comparison (backstage-15).

## Audit (2026-11-12)

After batch 4, the whole stratum was reviewed again before it was scored. Every figure above is
after the audit and the review (section "Review (2026-11-13)").

### What was reviewed

- **Every unit** (1,546), grouped by the family it had before the audit: the 141 `locale`
  units, the 172 `dep-version` and 79 `toolchain-pin` units, and the `other` units in three
  groups (373 A, 277 B and C, 504 D). Each
  reviewer checked the bucket, family, assertion family and `needs` against the frame's rules
  and the precedents in the other repositories, and read the evidence in the clone where a
  record was in doubt.
- **The rule mapping.** Each unit now lists the configuration rules that express it, and each
  rule belongs to a unit or to the record's value-add rules. Building that map found rules
  attached to the wrong unit, A units with no rule, and `alint` texts that named rules a unit
  did not have.
- **The defects.** Both batch 1 defects were reproduced on the 0.22.1 binary and filed (#502,
  #503). The reproduction found a third (#504).
- **Vacuity.** `alint explain --trace` over the 1,191 rules at the start of the audit: 19 select
  no file. 17 are absence rules, which pass vacuously by design. The other two are
  zulip-unmerged-changelog-no-feature-level-heading, whose record already says it is vacuous at
  the pin, and zulip-cc-scripts-no-sudo-bash (no file matches its `**/scripts/**/*.bash` glob at
  the pin).
- **The prose** of the frame, this log and general-linter section 12, against the records.

### Rulings

[`corpus-stratum.md`](corpus-stratum.md) lists the audit's rulings under "Rulings from the
audit". These moved units, in every batch:

- when a regex read is C and when it is A;
- release age is `other` wherever it stands alone;
- `locale` covers locale files only;
- `toolchain-pin` is versions agreeing, and install-time package-manager settings are
  `dep-version`;
- a schema the repository does not ship is C, and a validator beyond a schema is B-delegate;
- a comparison with another revision's content is B-adapter `base-revision`, while a rule over
  the paths a change touches stays C;
- inline suppressions approximated by a path exclude make a unit C.

### Units moved

82 units changed bucket, family, assertion family or `needs`; 13 were added and 1 removed, for
1,558 units (1,546 before). Every change is in the unit's `changes` list with its reason.

**Bucket** (50 units):

| From | To | Units |
|---|---|---|
| A | C | forem-05, ghost-15, rushstack-07, tq-12, trpc-11 |
| C | A | fluentui-06 |
| D | A | zulip-18 |
| B-config | B-adapter | babel-12, ghost-67, ghost-68, rushstack-02, thunderbird-android-b01 |
| B-config | C | forem-22, forem-b03, ghost-62, ghost-70, ghost-b09 |
| B-adapter | B-config | strapi-59, trpc-27 |
| B-adapter | C | backstage-54, fluentui-23, fluentui-b17, ghost-32, ghost-61, nextcloud-25, rushstack-b11, rushstack-b12, signal-desktop-07, signal-desktop-09, strapi-56, strapi-58 |
| B-kernel | B-adapter | rushstack-b07 |
| B-delegate | C | element-web-20, tq-34 |
| B-delegate | D | mattermost-99 |
| C | B-delegate | backstage-b06 |
| D | B-adapter | backstage-64, backstage-89, ghost-39, ghost-56, ghost-71, immich-22, mattermost-16, rushstack-37, thunderbird-android-44 |
| D | B-config | zulip-52 |
| D | B-delegate | backstage-63, zulip-53 |
| D | C | ghost-79, zulip-58 |

**Family** (23 units, bucket unchanged):

| To | Units | Why |
|---|---|---|
| `other` | element-web-11, fluentui-30, immich-17, rocketchat-23, signal-desktop-21, tq-17, zulip-14 | release age |
| `other` | immich-08, nextcloud-04, rocketchat-14, signal-desktop-14, thunderbird-android-07 | strings in source or layout files |
| `other` | fluentui-32, jest-36 | a compile under a toolchain version |
| `dep-version` | backstage-60, backstage-61, signal-desktop-43, signal-desktop-44, yarnpkg-berry-27 | install-time package-manager settings |
| `toolchain-pin` | fluentui-56, ghost-34, thunderbird-android-31, thunderbird-android-32 | a pin or a bound on one |

**Assertion family or `needs`** (9 units): element-web-05 and mattermost-06 gain `equality`;
mattermost-21 becomes entry values with `set`; tq-11 becomes entry values with `unique-key`;
fluentui-b09, mattermost-76 and rocketchat-44 need `derived-value`; ghost-b14 needs
`normalization`; jest-26 needs `package-graph`.

**Added** (13 units): the D review found behaviors that Stage A had folded into a D unit.

| Unit | Bucket | What | Split from |
|---|---|---|---|
| tq-a01 | C | no dangerous workflow triggers | tq-40 |
| tq-a02 | A | no `secrets: inherit` | tq-40 |
| zulip-a01 | A | `actions/checkout` does not persist credentials | zulip-82 |
| zulip-a02 | A | third-party actions pinned by hash | zulip-82 |
| immich-a01 | A | CLI file names in kebab case | immich-38 |
| immich-a02 | B-adapter (`route-model`) | docs links resolve to a site route | immich-43 |
| ghost-a01 | A | markdownlint MD052: reference labels defined | ghost-79 |
| ghost-a02 | B-adapter (`format-reader`) | markdownlint MD056: table column counts | ghost-79 |
| ghost-a03 | C | a line cap on the i18n index | ghost-77 |
| element-web-a01 | C | new source files are TypeScript | element-web-46 |
| rushstack-a01 | C | a copyright header in every source file | rushstack-39 |
| thunderbird-android-a01 | C | new code is Kotlin (no rule: it would hand-list 287 Java files) | thunderbird-android-71 |
| backstage-a01 | B-adapter (`other`) | one source descriptor per patch file | backstage-60 |

**Removed** (1 unit): rushstack-b15 duplicated rushstack-09, the same schema check over three
more files (R1). Its three rules joined rushstack-09's.

**Text only:** 34 more units had a summary, evidence, `alint` text or reason corrected, where it
contradicted the unit's own classification or its rules.

### Configuration changes

- **26 rules added** for the new and moved units, each confirmed by R4 (passes on the clone,
  fires on a mutation): Ghost 7, TanStack/query 4, zulip 3, rushstack 3, Signal-Desktop 2,
  strapi 2, forem 2, backstage 1, element-web 1, immich 1.
  - 14 of them express 11 of the 19 units the audit moved from B to C: 11 rules for rushstack-b12,
    ghost-62, ghost-70, backstage-54, strapi-56, strapi-58, forem-22 and forem-b03, and one
    each for rushstack-b11, signal-desktop-07 and -09. Each was fire-tested twice: by the
    reviewer who wrote it, and again with different mutations.
  - nextcloud-25's commit-trailer ban needs a generated complement pattern of about 9,600
    characters. It matched the workflow's grep patterns on 4,560 generated commits, but stays
    out of the configuration, because the clone is depth 1.
- **Comments relabelled** where a unit's bucket changed.
- **rocketchat-25's name regex** now accepts any whitespace around the colon, so it holds in any
  layout (the regex ruling).
- **Every configuration** validates and passes on its clone; `xtask corpus-replay --only`
  replays all 20.

### Not changed

The reviewers raised these and the audit kept them:

- **babel-50 and strapi-62** look alike but are not: one is a negated reference, the other
  set disjointness.
- **thunderbird-android-55** is a negated equality, consistent with its family.
- **A B-adapter unit without entry values may still name an assertion family** (backstage-a01,
  fluentui-20, -31, -b09, mattermost-06, nextcloud-18, -19, thunderbird-android-b01; the audit
  gave mattermost-06 its family). The figures count only entry-values units.
- **strapi-21, zulip-02 and thunderbird-android-09** are low-confidence calls that the evidence
  supports as recorded.
- **signal-desktop-51 and -52** mix knip checks and stay `other`.
- **rocketchat-18, yarnpkg-berry-11, trpc-51, signal-desktop-49 and wsdk-b06** match their
  precedents.
- **Frontmatter units keep `format-reader`**, including trpc-28 and -32. Those two also need a
  keyword filter over the manifests, which was not re-examined.
- **Rules at level `info` on A or C units** (jest-19, -22, yarnpkg-berry-24, -25, backstage-45)
  check an alternate input form or a demoted half, which the frame allows.

### Gates added

`cargo test -p xtask` now checks every record
(`committed_stratum_records_match_pins_configs_and_metrics`):

- the record's commit and file name match `pins.tsv`;
- bucket, family, assertion family, `needs` and origin take only the frame's values, and only
  B units name an assertion family;
- a B-config unit and an entry-values unit name an assertion family, and only a B-adapter unit
  has `needs`;
- every rule a unit or the value-add list names exists in the configuration, every A unit names
  one, and every configuration rule belongs to a unit or is value-add;
- every rule a unit's `alint` text names is one of its rules or a value-add rule;
- `metrics` and `by_family` recompute from the units.

## Review (2026-11-13)

After the audit was merged (#501), a review checked the records against the frame's rulings, the
configurations' comments and the published figures, and recomputed every table. The review's
own rulings are in [`corpus-stratum.md`](corpus-stratum.md) under "Rulings from the review"; each
says where an earlier ruling already drew the line.

### Units moved

Eight units moved, each recorded in its `changes` list with `by: review`:

| Unit | From | To | Why |
|---|---|---|---|
| backstage-65 | A | C | one file is excluded for its inline eslint-disable |
| element-web-18 | A | C | one generated file is excluded for its eslint-disable |
| wsdk-b02 | A | C | the target regex reads the test's list in one layout |
| rushstack-57 | A | C | the default subspace's half needs a line-anchored regex |
| jest-20 | C | A | its package list is the checker's own literal, not a hand-kept list |
| rushstack-b14 | A | B-config `unique-key` | it folds in the duplicate plugin-name check |
| thunderbird-android-61 | `locale` | `other` | a BOM check over every source and resource file |
| thunderbird-android-62 | `locale` | `other` | an encoding check over every XML resource |

The moves change `locale` to 22% today, 26% after the slice and 29% with entry values (from
23%, 28% and 30%). The pooled figures stay at 51%, 54% and 56%, and `unique-key` gains one
B-config unit (rushstack-b14).

### Text and records

- **Text from before Stage B or the audit** that contradicted the unit's bucket or rules:
  thunderbird-android-53, ghost-44, ghost-b09, jest-27, strapi-61 and rushstack-47 (their
  reasons or `alint` texts were still B-config or D text), fluentui-06's `fire_verified`, and
  thunderbird-android-50, whose texts left out its strict-JSON rule.
- **Rules a unit listed wrongly:** rushstack-09b listed rushstack-c01's rule, wsdk-b02 listed
  wsdk-19's, and thunderbird-android-02 listed one of the 34 rules it shares with -01.
- **`alint` texts** that left out listed rules (14 units), named a rule id that does not exist
  (tq-31), gave a proposed or scratch id for a rule now in the configuration (26 units), or were
  empty (14 units).
- **mattermost-06's audit change** now records the classification it replaced (`from`), as the
  other 81 audit moves do.
- **Partial rules** on tq-B05 and wsdk-17 were called value-add; they are partial rules at level
  `info`. element-web-48's `alint` text still called its rule value-add at level `info`; the rule
  runs at level `error`.
- **Texts a move left behind:** 34 units whose `alint`, `reason` or `fire_verified` text still
  described the bucket that Stage B or the audit had moved them out of:
  - 12 moved from A to C: 11 by Stage B, and backstage-55, which Stage B moved by analogy with
    backstage-49 and the review reproduced on a scratch file;
  - 14 moved out of D, whose `alint` text named the repository's tool (ghost-39, backstage-64,
    fluentui-31, trpc-57 and others) or whose reason still argued for D (ghost-80, strapi-35,
    zulip-55);
  - 4 moved from B-adapter to C (ghost-61, fluentui-23, -b17, yarnpkg-berry-23), whose `alint`
    text still gave the old need;
  - nextcloud-18 and -19, moved from B-config to B-adapter, whose reasons did not name the missing
    SPDX expression extractor;
  - signal-desktop-38 and -46, whose `fire_verified` still said C.

### Configurations and prose

- **Comments relabelled:** seven unit labels that disagreed with the record (tq-B04, jest-27,
  rushstack-01, strapi-07, zulip-04, zulip-39, and tq-10 in prose), the eight moved units, and
  the header summaries of nine configurations, which still gave pre-audit buckets.
- **This log:** the parent of ghost-a01 and -a02 (ghost-79, not ghost-77), the count of rules
  written for units moved from B to C, batch 4's `locale` and `needs` counts, the units the
  "Not changed" list names, the vacuity note's glob, and three batch-section wordings.
- **general-linter.md section 12.4:** the families and capability figures above, and
  "normalization (3)", which covers one Unicode and two unit normalizations.

### Gates added

`cargo test -p xtask` now also checks:
- that every recorded change names who made it (`B`, `integrator`, `audit` or `review`), and
  that a change of bucket, family or assertion family records the classification it replaced or
  its new values (`check_stratum_unit_changes`);
- that every configuration comment labelling a unit with its bucket, in the three forms the
  configurations use, matches the record (`stratum_config_comment_labels_match_records`, about
  600 labels).

### Not changed

- **thunderbird-android-50** stays A: its unit is the schema check, which `json_schema_passes`
  expresses. Its strict-JSON rule approximates the build's parser, the gap section 12.4's
  strict-JSON option would close.
- **forem-b01 and mattermost-b02** stay `locale`: they check the registries that list the
  supported locales.
- **immich-36** stays D: it runs project code, besides needing the base revision's spec.
- **nextcloud-50** stays D, with its reason rewritten: it gates when merges may happen, keyed on
  another branch's state, rather than checking content.

## Phase 8 re-scoring (2026-11-20)

Phase 8 audited every unit in ADR-0033's projected slice (69 units: 43 B-config and 26 B-adapter
with `needs: entry-values` in `equality`, `unique-key` and `set`, plus `forem-01` under grouped
set equality) against the shipped `version: 2` relation evaluator and `constraint` kind, verifying
non-trivial shapes directly against the release binary. The committed stratum JSON records under
[`stratum/`](stratum/) and the `version: 1` configurations under [`../v0.12/corpus/`](../v0.12/corpus/)
stay unchanged so `xtask corpus-replay` against v0.22.1 and `cargo test -p xtask` keep their
baselines; this section records the Phase 8 re-scoring.

### Totals against Today and ADR-0033 projections

Percentages are `A / (A + B + C)`. "Phase 8 (projected set)" counts only the units from ADR-0033's
69-unit projected set that the shipped slice expresses cleanly as A; "Phase 8 (+C->A)" also counts
four C units that `entries:` promotes to A by replacing layout-dependent key regexes (log finding
6). Units expressible only awkwardly (self-disjoint `set` to assert emptiness, or a regex over
frontmatter/Go source) move to C and stay out of A.

| Family | Units | Non-D | Today | ADR-0033 projected | Phase 8 (projected set) | Phase 8 (+C->A) | Moved B->C |
|---|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 134 | 114 | 25 (22%) | 40 (35%) | 32 (28%, +7) | 32 (28%, +7) | 2 |
| `dep-version` | 170 | 104 | 45 (43%) | 63 (61%) | 56 (54%, +11) | 59 (57%, +14) | 1 |
| `toolchain-pin` | 81 | 65 | 41 (63%) | 41 (63%) | 41 (63%, +0) | 41 (63%, +0) | 0 |
| `other` | 1,173 | 677 | 376 (56%) | 412 (61%) | 397 (59%, +21) | 398 (59%, +22) | 3 |
| **Pooled** | 1,558 | 960 | 487 (51%) | 556 (58%) | 526 (55%, +39) | 530 (55%, +43) | 6 |

By stratum:

| Stratum | Family | Units | Non-D | Today | ADR-0033 projected | Phase 8 (projected set) | Phase 8 (+C->A) |
|---|---|--:|--:|--:|--:|--:|--:|
| i18n | `locale` | 129 | 109 | 22% | 35% | 28% (+7) | 28% (+7) |
| i18n | `dep-version` | 57 | 22 | 50% | 55% | 50% (+0) | 55% (+1) |
| i18n | `toolchain-pin` | 40 | 31 | 65% | 65% | 65% (+0) | 65% (+0) |
| i18n | `other` | 630 | 354 | 62% | 66% | 63% (+3) | 64% (+4) |
| i18n | **all** | 856 | 516 | 53% | 59% | 55% (+10) | 56% (+12) |
| JavaScript monorepo | `locale` | 5 | 5 | 20% | 40% | 20% (+0) | 20% (+0) |
| JavaScript monorepo | `dep-version` | 113 | 82 | 41% | 62% | 55% (+11) | 57% (+13) |
| JavaScript monorepo | `toolchain-pin` | 41 | 34 | 62% | 62% | 62% (+0) | 62% (+0) |
| JavaScript monorepo | `other` | 543 | 323 | 48% | 56% | 54% (+18) | 54% (+18) |
| JavaScript monorepo | **all** | 702 | 444 | 48% | 57% | 54% (+29) | 55% (+31) |

### Unit-by-unit audit of ADR-0033's 69 projected units

#### `locale` (15 projected: 7 confirmed A, 2 moved B->C, 6 remain B)

- **Confirmed A (7: 6 B-config, 1 entry-values):**
  - `forem-01` (B-config, merged per-locale YAML trees): `entries: { format: yaml, at: "$", leaves: true }`, `fields: { locale: entry.root, key: entry.path }`, `transform: { key: [strip-root] }`, `where:` prefix filter, `set: equal_to: { group: en }`, `groups: [fr, pt]`, `plurals: cldr`.
  - `mattermost-03` (B-config): `read: { json: "$[*].id" }`, `unique: { key: [file, id] }`.
  - `mattermost-09` (B-config): `entries` on `server_plurals.json` (`entry.key`) `set: equal_to` `read: { whole_file: {} }` (`stem`) over non-English `server/i18n/*.json`.
  - `mattermost-28` (B-config): `i18n/glossary/*.json` stems `set: equal_to` a `union:` of `server/i18n/*.json` and `webapp/channels/src/i18n/*.json` stems.
  - `mattermost-33` (B-config): `entries` on `i18n/style.json` (`entry.key`) `set: subset_of` the catalog union above.
  - `rocketchat-10` (B-config): duplicate-aware `entries: { format: json, at: "$" }` over flat `*.i18n.json` files, `unique: { key: [file, key] }`.
  - `mattermost-21` (entry-values): `en.json` (`entries` at `"$.*"`, `field == "defaultMessage"`, value matches `(?:^|[^'])(?:'')*'(?:<|\{)`) and translation catalogs (`entries` at `"$"`, `where: { key: { in: [{ relation: en-keys, field: key }] }, val: { matches: ... } }` so extra keys absent from `en.json` are skipped as `check_icu.mjs` does), `set: { field: key, group_by: file, subset_of: { relation: en-apos, field: key } }`.
- **Moved B->C (2, not counted in A):**
  - `mattermost-b01` (B-config): set equality works via `set: equal_to` (plus `unique:` on the slice), but extracting `supportedLocales` from `i18n.go` relies on a one-layout Go slice regex (same ruling as `mattermost-b02`).
  - `element-web-06` (entry-values): exact semantics run on the binary via `where: { value: { matches: '(?i)\belement\b' }, key: { not_in: [...] } }` and a self-disjoint `set: disjoint_from` against the filtered relation itself; asserting emptiness that way is awkward and awaits `bounded-count` (`max: 0`) or an inline literal `set` target in v0.24.
- **Remain B (6: 2 B-config, 4 entry-values):**
  - `mattermost-27` (`union:` shares one `fields:` map, so `entries:` `entry.key` cannot union with `read:` `$.*.aliases[*]` `value`).
  - `strapi-21` (needs suffix-strip transform `.singular`/`.plural` -> prefix, #592).
  - `element-web-05` (needs negated row-level equality `value != key` and `|`-joined key path).
  - `mattermost-07` (array-of-objects rows #592 and distinguishing scalar vs plural-object value kinds).
  - `mattermost-32` (sibling `aliases` array fails `entry.value` before `where:` runs; sibling-field rows need #592).
  - `mattermost-35` (per-entry two-array disjointness with conditional `matchCase` folding).

#### `dep-version` (18 projected: 11 confirmed A, 1 moved B->C, 6 remain B)

- **Confirmed A (11: 3 B-config, 8 entry-values):**
  - `jest-24`, `rushstack-51`, `trpc-08` (B-config): `entries` over the forbidden dependency maps, `unique: { key: [manifest, dependency] }`.
  - `jest-23`, `trpc-05`, `yarnpkg-berry-01` (entry-values): the three flagship dependency-version conformance policies (shipped with firing and silent `alint test` cases; `yarnpkg-berry-01` carries the documented `selectors-and-constraints.md` sections 7 and 16 note that the workspace-package exemption covers all non-peer fields rather than dev-only, agreeing at berry's pin because workspace deps use `workspace:^`).
  - `tq-10` (entry-values): root and `packages/*` minus integrations/examples, ignoring `typescript` and `catalog:/workspace:/npm:`, `unique: { key: [dependency], determines: [range] }`.
  - `strapi-02` (entry-values): prod and dev dependencies excluding `docs/**` and `test-apps/**`, `where:` exact-semver regex, `unique: { key: [dependency], determines: [range] }`.
  - `babel-11` (entry-values): non-root, non-internal workspaces, `where: { range: { not_matches: "^(workspace|catalog|link):" } }`, `unique: { key: [dependency], determines: [manifest] }`.
  - `wsdk-09` (entry-values): non-protocol dependencies (excluding `workerd`) `set: disjoint_from` `pnpm-workspace.yaml` `$.catalog` keys.
  - `fluentui-17` (entry-values): syncpack source manifests outside the carve-out version groups (`unique: { key: [dependency], determines: [range] }`; if any carve-out in `syncpack.config.js` pairs one package with a multi-package dependency, excluding `fluentui-17` puts `dep-version` at 55/104 = 53% on the projected set and 58/104 = 56% with C->A).
- **Moved B->C (1, not counted in A):**
  - `babel-12` (entry-values): exact semantics run on the binary via an inner `join` of `prod-deps` (`range not_matches "^condition:"`) and `dev-deps` on `{ manifest: manifest, dep: dep }` (`unique: right`) plus self-disjoint `set` to assert the join is empty; awaits `bounded-count` (`max: 0`) for clean A.
- **Remain B (6: 2 B-config, 4 entry-values):**
  - `fluentui-24`, `trpc-07` (per-manifest key subset; `set` cannot compare two key sets per manifest and `join` is inner-only).
  - `nextcloud-b02` (the equality half is confirmed via `join` + row-level `equal` `to: { field: spec }`, `selectors-and-constraints.md` section 8, but the `$<name>` reference half needs prefix-strip #592 and `reference` in v0.24; under the tokio ruling the folded unit stays B).
  - `rushstack-01` (per-dependency `allowedAlternativeVersions` pair allowlist).
  - `tq-11` (needs a dependency-to-similar-family mapping transform).
  - `wsdk-06` (allowlist lives in a block-scoped TypeScript array literal beside `IGNORED_DIST_IMPORTS`).

#### `toolchain-pin` (0 projected)

Remains 41/65 (63%), as ADR-0033 projected. All 13 B-adapter units need `version-compare`.
`signal-desktop-28` (already A via v1 `cross_file` `equals`) is re-expressed as a v2 `constraint`
conformance policy for the section 12.1 comparison.

#### `other` (36 projected: 21 confirmed A, 3 moved B->C, 12 remain B)

- **Confirmed A (21, all B-config):**
  - 8 unique workspace/project name units (`read: { json: "$.name" }` + `unique: { key: [name] }`): `babel-B01`, `backstage-69`, `fluentui-25`, `fluentui-26`, `jest-B02`, `strapi-b04`, `tq-35`, `trpc-B05`.
  - 7 unique JSON array-element value units (`read: { json: ... }` + `unique:`): `ghost-45`, `rushstack-13`, `rushstack-60`, `rushstack-b02`, `rushstack-b14` (pairs with its v1 schema rule), `strapi-24`, `tq-33` (`$.fixed[*][*]`).
  - 2 regex-extracted unique-value units: `zulip-52` (template element ids, matching the upstream tool's regex minus `IGNORE_IDS`), `fluentui-b08` (`<Meta title>` with `lower`, per the record's note that exact title uniqueness is the faithful key).
  - 4 cross-file `set` units (each verified on the binary): `immich-20` (`ORDER` lines `equal_to` migration stems), `strapi-62` (`tags.yml` keys `disjoint_from` package leaf folder names, paired with v1 `yaml_path_absent` for the three status literals), `trpc-17` (two-way equality between `.agents/skills/*/SKILL.md` `parent_name` and the union of `skills-lock.json` `$.skills` keys and `.oxfmtrc.json` negated entries via three `set: subset_of` rules: `(skills \ in-repo) subset_of installed`, `installed subset_of skills` and `in-repo subset_of skills`), `wsdk-17` (`optional: true` `$.scripts.build` `dir` `subset_of` `optional: true` `$.tasks.build.outputs[*]` `dir`).
- **Moved B->C (3, not counted in A):**
  - `strapi-59`, `trpc-27` (`unique:` expresses uniqueness, but frontmatter values are still read by regex).
  - `ghost-63` (`followUps` keys `subset_of` `standard` keys and non-empty check are clean A via `set: subset_of`, while the allowed-field check uses self-disjoint `set`).
- **Remain B (12: 5 B-config, 7 entry-values):**
  - `mattermost-56`, `zulip-28`: **caught by binary verification.** `entries: { format: yaml, at: "$", leaves: true }` catches duplicate leaf keys, but misses duplicate non-leaf mapping keys with disjoint children (`jobs: { a: 1 }` / `jobs: { b: 2 }`) and never descends into YAML sequence elements (`steps: [{ name: a, name: b }]`). General duplicate-key detection still needs a dedicated `no_duplicate_keys` (gap B1) or document-wide key stream.
  - `rushstack-47` (`operationName` uniqueness works, but the folded overlapping-output-folders check does not; tokio ruling).
  - `trpc-24` (composite key from two fields of one array element, #592).
  - `forem-48` (block-scoped Ruby `%w[]` array literal).
  - All 7 `other` `entry-values` units (`backstage-20`, `backstage-b07`, `fluentui-19`, `forem-b08`, `ghost-68`, `mattermost-63`, `thunderbird-android-55`): need structured/subtree values, derived keys, array-of-objects pairing (#592), or negated equality (`thunderbird-android-55`).

#### Separate C->A promotions from key selection (4 units)

Because `entries:` reads JSON, YAML and TOML mapping keys directly without layout-dependent line
regexes (finding 6), four units previously classified C promote cleanly to A (verified on the
binary): `strapi-06` (`dep-version`, `.yarnrc.yml` `$.catalog` keys), `wsdk-74` (`dep-version`,
`pnpm-workspace.yaml` `$.catalog` keys), `thunderbird-android-b03` (`dep-version`,
`gradle/libs.versions.toml` `$.versions` and `$.libraries` keys against `$.libraries.*.version.ref`
and `$.bundles.*[*]`), and `thunderbird-android-53` (`other`, `$.overrides.*.*` keys against
`$.flags[*].key`). None of the 11 `reference` B units promotes in v0.23.

### v2 locale parity vs `key_parity` (`selectors-and-constraints.md` section 16)

- **Merged per-locale trees (`forem-01`):** `key_parity` requires 114 per-file rules and gives 8
  false positives at forem's pin from keys relocated between files across locales. The v2
  `constraint` `set` policy merges every file per `entry.root`, collapses CLDR plural groups,
  filters `date.*`, `devise.*` and `views.*` prefixes before plural collapse, and enforces required
  locales via `groups: [fr, pt]` with zero false positives on relocated keys.
- **Per-file locales:** `key_parity` remains shorter (8 vs 17 non-comment lines), faster and lower
  memory (see `general-linter.md` section 12.4 for the Phase 8 benchmark table). `where:` runs
  before CLDR plural collapse in v2 while `ignore_keys` runs after it in `key_parity`, which agrees
  for whole-subtree prefix ignores and differs only if a filter drops individual members of a
  plural object.

