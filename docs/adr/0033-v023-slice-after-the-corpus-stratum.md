---
status: accepted
date: 2026-11-13
decision-makers: asamarts
---

# 0033. v0.23 slice after the corpus stratum: set instead of cardinality, entry values, and the #503 fix

## Status

Accepted (2026-11-13). Amends [ADR-0032](0032-v023-owner-decisions.md) decision 1 (the slice's
assertion families, and value-bearing entry rows in its relations) and the consequence that a v1
configuration keeps its findings (decision 3 here). The rest of ADR-0032 stands.

## Context

ADR-0032 decision 1 gave v0.23's report-only `constraint` kind the three assertion families the
conformance policies need (equality, unique/key and cardinality) and deferred reference, set,
group and bounded count to v0.24. It also asked for an i18n and JavaScript-monorepo stratum in the
corpus before the design commits to operators. That stratum is measured: 1,558 units in 20
repositories, audited (#501) and reviewed (#505). The log is
[`v0.23/corpus-stratum-log.md`](../design/v0.23/corpus-stratum-log.md), and the synthesis is in
[`general-linter.md`](../design/general-linter.md) section 12.4.

- **`cardinality` has 1 unit of demand** (forem-01, key parity over locales merged from many
  files). Locale key parity is otherwise A already, through `key_parity`, so the family's main
  use in the slice was to re-express it.
- **`set` has 17 units** within reach: 11 B-config, and 6 B-adapter units that need only entry
  values. `reference` has 11 (5 + 6).
- **Locale key parity is also set equality per locale.** General-linter section 7.4's example is
  `sets_equal` with `group_by: locale`. The grouped form covers forem-01 as well.
- **Value-bearing entry rows are the most common capability need:** 32 B-adapter units in 16 of
  the 20 repositories. The relation layer's `entry`, TOML and XML adapters emit key paths without
  values, and `extract` emits values without a pointer.
  - One range per dependency across workspaces, the commonest form of the dependency version
    policy, is enforced in 6 of the 10 JavaScript monorepos. It needs (dependency, range) rows.
  - Without them, the slice's dependency version policy cannot express what those repositories
    enforce.
- **#503:** under the default `respect_gitignore: true`, `git_tracked_only: true` never sees a
  file that git tracks but an ignore file excludes (force-added, or committed before the ignore
  line). The walker prunes it before the git-tracked index is built. The documentation, the
  bundled `java@v1` rules, `fix: git_untrack` and two examples assume the opposite. The v0.23
  plan says a v1 configuration keeps its findings (`v0.23/README.md` section 3), so fixing the
  behavior needs an explicit exception.
- **The stratum's rulings** decide which units the slice can serve. The owner reviewed the audit's
  rulings, including that a comparison with another revision's content is B-adapter
  (`needs: base-revision`) rather than D.

## Decision

1. **The slice's assertion families are equality, unique/key and set.**
   - `set` covers inclusion, equality and disjointness between two named sets, and the same per
     group (`group_by`), as in general-linter section 7.4.
   - The locale conformance policy expresses key parity as per-locale set equality.
   - Cardinality moves to v0.24, with reference, group and bounded count.
2. **The slice's relations include value-bearing entry rows.**
   - The `entry` adapter can emit (key path, value) rows for JSON, YAML and TOML.
   - The dependency version policy expresses one range per dependency as unique/key over
     (dependency, range) rows.
   - The phase 3 design document specifies the row shape, its limits and its provenance, and
     whether XML joins.
3. **#503 is fixed in the behavior, not in the documentation.**
   - A tracked path that the walk pruned only because an ignore file excludes it returns to the
     git-tracked indexes, with its parent directories, when it exists on disk inside the root.
     `git_tracked_only: true` then means what the documentation says. The walker's other
     exclusions still apply.
   - A v1 configuration that uses `git_tracked_only` can gain findings. This is the one exception
     to the plan's "v1 is unchanged". The CHANGELOG lists it, and every difference in the
     release's corpus replay must come from it.
   - `add -f` scenarios cover `file_absent`, `dir_absent`, `file_exists` and `dir_exists`.
   - The kinds that check that a path resolves keep reading the walked index; the fix corrects
     their "on disk" wording.
4. **The stratum's rulings stand as recorded** in
   [`v0.23/corpus-stratum.md`](../design/v0.23/corpus-stratum.md), with the review's statements of
   where they draw the line. Phase 8 re-scores the stratum under them.

## Consequences

- **Projected coverage.** The stratum's records compute "after the slice" with ADR-0032's
  families, and the records keep that definition. The same units under this decision, with
  forem-01 counted under grouped set equality:

  | Family | ADR-0032 slice | with entry values | This slice | with entry values |
  |---|--:|--:|--:|--:|
  | `locale` | 26% | 29% | 30% | 35% |
  | `dep-version` | 46% | 57% | 48% | 61% |
  | `toolchain-pin` | 63% | 63% | 63% | 63% |
  | other | 59% | 60% | 60% | 61% |
  | Pooled | 54% | 56% | 55% | 58% |

  Today's figures are 22%, 43%, 63%, 56% and 51%. "With entry values" also counts B-adapter units
  that need only entry values in one of the slice's families. Phase 8 re-scores the stratum with
  the shipped slice and compares it with the last two columns.
- **More v0.23 scope:** one adapter extension in the relation layer (phase 6) and the #503 fix,
  which is its own pull request before the feature freeze. If the slice cannot be ready, the
  plan's status log records it, as ADR-0032's consequences allow.
- **Cardinality waits.** Its other uses (exactly one, at least one, counts per group) have no
  stratum unit that v0.23 would serve.
- **The documents that name the families** change with this record: the v0.23 plan, the
  roadmap's v0.23 section, general-linter sections 11.6 and 12.4, and the stratum frame.

## Considered Options

- **Assertion families:**
  - keep ADR-0032's three: no new record, but `cardinality` serves 1 unit and `set` waits;
  - swap cardinality for set (chosen): the same number of families, with grouped set equality
    covering key parity;
  - add set and reference and keep cardinality: the most coverage (pooled 59% with entry values),
    with five families to design, test and document in v0.23.
- **Entry values:** in v0.23 (chosen); deferred to v0.24, so v0.23's dependency version policy
  covers only what key rows allow; decided at the design gate after a cost spike.
- **#503:** fix the behavior (chosen); keep the behavior and correct the documentation, pointing
  at `git_no_denied_paths`; an opt-in mode that reads git's index, which changes no finding by
  default but adds an option to document and keep.
- **The rulings:** accept as recorded (chosen); move `base-revision` units to D, which drops 15
  units from the denominator; re-score the stratum before the design.

## More Information

- [`v0.23/README.md`](../design/v0.23/README.md): the v0.23 plan, with these decisions in its
  scope, phases and status log.
- [`v0.23/corpus-stratum-log.md`](../design/v0.23/corpus-stratum-log.md) and
  [`v0.23/corpus-stratum.md`](../design/v0.23/corpus-stratum.md): the measurement and its rulings.
- [`general-linter.md`](../design/general-linter.md): sections 7.4, 11.6 and 12.4.
- Issues #502, #503 and #504; pull requests #501 and #505.
