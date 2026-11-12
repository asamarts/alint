# v0.23 corpus stratum: log

Status: batch 1 of 4 complete (2026-11-12). Frame and method:
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
([`corpus-stratum.md`](corpus-stratum.md) section 2.1). "After slice" counts a B-config unit as A
when its assertion family is `equality`, `unique-key` or `cardinality`. Phase 8 confirms each
such unit before the figure is final.

## Running totals

Through batch 1 (5 repositories).

| Repository | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| forem/forem | 58 | 21 | 7 | 4 | 0 | 0 | 4 | 22 | 58% | 69% | 31% | 69% |
| zulip/zulip | 188 | 108 | 1 | 3 | 0 | 3 | 11 | 62 | 86% | 94% | 6% | 87% |
| element-hq/element-web | 48 | 5 | 0 | 4 | 0 | 1 | 4 | 34 | 36% | 64% | 36% | 36% |
| yarnpkg/berry | 43 | 13 | 2 | 1 | 0 | 0 | 6 | 21 | 59% | 86% | 14% | 64% |
| trpc/trpc | 65 | 10 | 7 | 13 | 0 | 1 | 5 | 29 | 28% | 42% | 58% | 36% |
| **Pooled** | 402 | 157 | 17 | 25 | 0 | 5 | 30 | 168 | 67% | 80% | 20% | 71% |

| Family | Units | A | B-config | B-adapter | B-kernel | B-delegate | C | D | Today | Tuning | Gap | After slice |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `locale` | 22 | 4 | 1 | 7 | 0 | 1 | 4 | 5 | 24% | 47% | 53% | 29% |
| `dep-version` | 31 | 7 | 5 | 2 | 0 | 0 | 3 | 14 | 41% | 59% | 41% | 53% |
| `toolchain-pin` | 12 | 6 | 0 | 2 | 0 | 0 | 0 | 4 | 75% | 75% | 25% | 75% |
| `other` | 337 | 140 | 11 | 14 | 0 | 4 | 23 | 145 | 73% | 85% | 15% | 76% |

| Assertion family | B-config units | `locale` | `dep-version` | `toolchain-pin` | `other` |
|---|--:|--:|--:|--:|--:|
| `unique-key` | 6 | 0 | 2 | 0 | 4 |
| `reference` | 4 | 0 | 1 | 0 | 3 |
| `set` | 3 | 0 | 2 | 0 | 1 |
| `cardinality` | 2 | 1 | 0 | 0 | 1 |
| `bounded-count` | 1 | 0 | 0 | 0 | 1 |
| `equality` | 1 | 0 | 0 | 0 | 1 |

## Batch 1 (2026-11-12): forem, zulip, element-web, yarnpkg/berry, trpc

### The three families

- **`locale`** (22 units, all in the three i18n repositories): 4 A, 4 C, 1 B-config, 7
  B-adapter, 1 B-delegate, 5 D.
  - **The B-config unit:** forem's i18n-tasks missing-keys check (forem-01). It compares
    per-locale key trees merged from many files. Per-file `key_parity` needs 114 rules and gives
    8 false positives at the pin, because forem keeps some keys in different files per locale.
    This is cardinality over (locale, key) rows (section 2.2's batch 1 rulings).
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
- **`dep-version`** (31 units): 7 A, 3 C, 5 B-config, 2 B-adapter, 14 D.
  - **B-config:**
    - one range per dependency across workspaces, as `unique-key` (trpc-05, manypkg
      `EXTERNAL_MISMATCH`; yarnpkg-berry-01, `yarn constraints`);
    - peer dependencies also in dev dependencies, and a dependency in only one dependency type,
      as `set` (trpc-07 and -08);
    - catalog specifiers that resolve, as `reference` (trpc-10).
  - **B-adapter:** a semver range or version comparison (forem-07, trpc-06).
  - **D:** mostly lockfile freshness, `dedupe --check` and minimum release age, which need the
    package manager's resolution.
- **`toolchain-pin`** (12 units): 6 A, 2 B-adapter, 4 D.
  - **A:**
    - pins that agree: `.ruby-version` and the base image, `engines.node` and the
      `yarn.config.cjs` floor, the CI matrix and `engines`;
    - pins that are present and well formed: `.nvmrc`, `packageManager`, `yarnPath`.
  - **B-adapter:** an exact pin that must satisfy a declared range (element-web-13, trpc-03).

### Early signals for phase 3

1. **A semver-range operator is the most requested adapter:** 4 units in 3 repositories, 2 in
   `dep-version` and 2 in `toolchain-pin`. Both families depend on it beyond plain equality.
2. **`unique-key` leads the B-config families** (6 of 17). One version per dependency is the
   flagship case.
3. **The locale family's demand is mostly B-adapter, not B-config.** The slice moves it from 24%
   to 29%. Key parity is the one B-config unit; the rest need placeholder extractors per format,
   plural categories, key order, or (key path, value) rows.
4. **Value-bearing entry rows** come up twice in element-web. The `entry` adapter is key-only by
   design (`crates/alint-core/src/relation/adapters/entry.rs`).

### Reading the pooled figure

zulip contributes 188 of the 402 units. 108 of its A units are custom regex rules in its own
linter, each a unit under R1. The pooled 67% therefore leans on zulip. The per-repository median
`coverage_today` is 58%. The family tables are the stratum's measure; the pooled figure is
context.

### Stage B and integration

- **Stage B:** added 27 units, reclassified 17 and removed 4.
- **The integrator's changes:**
  - **zulip-60 removed.** Stage B reclassified it to C. It is a post-merge step that decides
    whether a notification runs, not a validation.
  - **element-web-50 rejected.** Stage B proposed it as a new unit. Translator review happens on
    Localazy, and the clone has about 70 violations at the pin (R5).
- **Stage C:**
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
