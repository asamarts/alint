# `apache/governance@v1` — Apache TLP governance discipline (bundled ruleset)

Status: **Implemented** — lands with the ruleset in v0.10 (this
commit; item #9 of the case-study coverage push — a **bundled
ruleset**, not a rule kind, so the rule-kind count stays 79).
Was a design draft (2026-05-18). v0.10 demand #9 (3 sources,
ROADMAP-canonical). Open questions resolved on implementation:
graduated-TLP scope, `DISCLAIMER` (podling-only) deferred (Q1);
composes-with-not-duplicates `compliance/apache-2@v1`, namespaced
ids (Q2); the "v0.9.18 A2 prerequisite" is the broadened
ASF-preamble `file_header` pattern, reused verbatim (Q3); levels
mirror the bundled-ruleset "non-blocking, upgrade in your own
config" convention (Q4); off-disk artefacts (branch protection,
release-signing server state) out of scope (Q5). v0.10
post-audit fix (P1 #44, decision D1): `apache-gov-notice-asf-attribution`
relaxed (matches the bare *and* long ASF attribution forms, not
just the parenthetical) and downgraded `error`→`warning` — see
the False-positive surface entry.

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("`apache/governance@v1` — LICENSE+NOTICE+KEYS+RAT discipline",
3 sources: arrow + spark + airflow — "3 Apache TLPs converge on
9 of 12 governance artefacts") and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`apache/governance@v1` row: airflow, arrow, spark). Canonical
scope: [`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#9; "LICENSE + NOTICE + KEYS + RAT discipline … v0.9.18 A2 is a
prerequisite"). `dotnet@v1` (#10) is the only remaining v0.10
item after this.

## Problem

An Apache Top-Level Project (TLP) is required by ASF policy to
ship a specific set of governance / release artefacts, and the
three densest Apache TLPs in the case-study corpus — arrow,
spark, airflow — independently re-implement the same checks
(custom scripts, RAT configs, CI greps). The shape recurs on
**9 of the 12** ASF governance artefacts:

- **LICENSE** + **NOTICE** at the repo root (the NOTICE must
  carry the ASF attribution line — not merely exist).
- **KEYS** — the OpenPGP public keys release managers sign
  source releases with; mandatory for an ASF source release.
- **RAT discipline** — Apache Release Audit Tool territory:
  every source file carries the ASF license header, and the
  source tree contains **no compiled binaries** (an ASF source
  release must be buildable from source, no jars/classes/.so).
- Supporting TLP artefacts: a project **README** and a
  **changelog / release-notes** file (release discipline).

`compliance/apache-2@v1` already exists but is scoped to
*license redistribution* (LICENSE text present, NOTICE exists,
source headers) — it deliberately does **not** assert the
*governance* layer: NOTICE *content*, KEYS, the no-binaries
source-release rule, or release-notes discipline. There is no
ruleset an Apache TLP can adopt to get the governance baseline
in one line.

## Surface area

A new **bundled ruleset** `alint://bundled/apache/governance@v1`
(new `apache/` namespace; file
`crates/alint-dsl/rulesets/v1/apache/governance.yml`, registered
in `bundled.rs`'s `REGISTRY` after the `compliance/*` overlays).
Composed **entirely of existing rule kinds** — no Rust, no new
rule kind, the rule-kind count is unchanged (79). `version: 1`.

```yaml
extends:
  - alint://bundled/apache/governance@v1
```

No fact gate — adopting it is the user's signal that the repo is
an Apache TLP (same convention as `compliance/apache-2@v1`).
Rule ids are namespaced `apache-gov-*` so adopting **both**
`apache/governance@v1` and `compliance/apache-2@v1` is safe (no
id collision; the overlap on LICENSE/headers is intentional and
each id is independently `level: off`-able).

### Rules (v1) — existing kinds only

| id | kind | artefact / pillar | level |
|---|---|---|---|
| `apache-gov-license-exists` | `file_exists` | LICENSE at root | error |
| `apache-gov-notice-exists` | `file_exists` | NOTICE at root | error |
| `apache-gov-notice-asf-attribution` | `file_content_matches` | NOTICE carries the ASF attribution (bare or long form) | warning |
| `apache-gov-keys-exists` | `file_exists` | KEYS at root (release-signing) | warning |
| `apache-gov-source-license-header` | `file_header` | RAT: ASF header on sources | warning |
| `apache-gov-no-binaries-in-source` | `file_absent` | RAT: no compiled binaries in the source tree | warning |
| `apache-gov-readme-exists` | `file_exists` | project README | warning |
| `apache-gov-changelog-exists` | `file_exists` | CHANGES / CHANGELOG / RELEASE_NOTES | info |

Eight rules covering the four ROADMAP pillars (LICENSE, NOTICE
incl. attribution content, KEYS, RAT = headers + no-binaries)
plus the two supporting TLP artefacts (README, changelog) — the
9-of-12 the three TLPs converge on. The artefacts deliberately
**not** in v1 (the ~3 they don't converge on, Open question 1):
`DISCLAIMER`/`DISCLAIMER-WIP` (podling-only; arrow/spark/airflow
are graduated TLPs), and off-disk artefacts (branch protection,
release-signing server-side state) alint cannot see.

## Semantics

Standard bundled-ruleset semantics (offline, no `extends:`/
`facts:`/`custom:` of its own — `bundled.rs`'s constraints). Key
choices:

- **`apache-gov-notice-asf-attribution`** — `file_content_matches`
  on the NOTICE file for the **invariant** ASF attribution
  substring `The Apache Software Foundation`. This deliberately
  matches *both* common real-world forms: the long template
  ("This product includes software developed at / The Apache
  Software Foundation (https://www.apache.org/).") **and** the
  very common bare form ("Copyright <year> The Apache Software
  Foundation", no parenthetical). The `(https://www.apache.org/)`
  parenthetical is LICENSE-appendix boilerplate, **not** a NOTICE
  invariant — requiring it false-positived on legitimate TLP
  NOTICEs (P1 #44 / D1), so it is *not* required. This is the
  governance check `compliance/apache-2@v1` does not do (it only
  asserts NOTICE *exists*). Level `warning` (D1): a wording
  mismatch on a baseline-adoption ruleset should not hard-block.
- **`apache-gov-source-license-header`** reuses the **v0.9.18
  broadened ASF pattern** verbatim — `Licensed (to the Apache
  Software Foundation|under the Apache License,?\s*Version 2)` —
  and the same exclude set as `compliance/apache-2@v1` (vendor,
  node_modules, target, build, dist, generated). Reusing the
  broadened pattern is the **"v0.9.18 A2 prerequisite"**: the
  short-form-only pattern produced 8,228 false positives against
  airflow; governance must not reintroduce that, so it inherits
  A2's resolved pattern rather than re-deriving one.
- **`apache-gov-no-binaries-in-source`** — `file_absent` on
  `**/*.{jar,war,ear,class,so,dll,dylib,a,o,pyc,pyo}` excluding
  `**/{vendor,node_modules,target,build,dist,test,tests,
  testdata,fixtures}/**` (test fixtures legitimately ship
  binaries; an ASF *source release* must not). The ASF
  source-release policy pillar.
- **Levels** mirror `oss-baseline@v1` / `compliance/apache-2@v1`:
  LICENSE / NOTICE *existence* are `error` (unambiguous, an ASF
  legal requirement); NOTICE-attribution wording, KEYS, headers
  and no-binaries are `warning` (real, but content/wording
  checks on a mid-adoption repo should not hard-block — D1
  moved NOTICE-attribution here); README / changelog are `info`.
  The ruleset header documents "upgrade severity in your own
  config when ready".

## False-positive surface

- **NOTICE attribution wording (P1 #44 / D1 — resolved).** The
  original pattern required the `(https://www.apache.org/)`
  parenthetical at `error` level. That parenthetical is
  LICENSE-appendix boilerplate, **not** a NOTICE invariant: many
  legitimate Apache TLP NOTICEs use the bare `Copyright <year>
  The Apache Software Foundation` form and would have
  hard-failed (an `error`-level false positive on the exact
  intended adopters; the original passing fixture was
  written-to-the-regex, masking it). Resolved: the rule now
  matches only the invariant substring `The Apache Software
  Foundation` (covers both the bare and the long template forms)
  at `warning` level. A representative bare-form-NOTICE silent
  e2e scenario was added so the realistic case is covered, not
  just the long template. Projects wanting strict exactness
  tighten in-config.
- **`no-binaries` vs. test fixtures.** Real ASF projects commit
  binary test fixtures (arrow's format test data, spark's test
  jars). The exclude set covers the conventional fixture dirs;
  a project with binaries elsewhere sets that rule `level: off`
  or narrows `paths` in its own config. Documented — better a
  tunable warning than a silent miss of a real source-release
  violation.
- **Header rule on non-ASF-licensed vendored code.** Inherited
  from A2's resolved exclude set; third-party code under
  `vendor/` etc. is excluded. A project vendoring elsewhere
  overrides the path scope (same as A2).
- **Graduated vs. podling.** v1 targets graduated TLPs (the 3
  demand sources). A podling additionally needs `DISCLAIMER`;
  that is Open question 1, not a silent wrong answer (the rule
  simply isn't present, so podlings layer their own).
- **Double-adoption with `compliance/apache-2@v1`.** Intentional
  and safe (namespaced ids); the LICENSE/header overlap fires at
  most one violation per artefact per ruleset, and each id is
  independently silenceable. Documented in the ruleset header.

## Implementation notes

- File `crates/alint-dsl/rulesets/v1/apache/governance.yml`
  (new `apache/` subdir under the in-crate `rulesets/` tree —
  it must live inside the crate so `cargo publish` /
  `include_str!` bundle it; cf.
  `feedback_include_str_stays_in_crate`).
- `bundled.rs`: one `("apache/governance", "v1",
  include_str!("../rulesets/v1/apache/governance.yml"))` row in
  `REGISTRY` (the slash in the name is resolved by the existing
  `<name>@<rev>` splitter — `hygiene/*`, `monorepo/*`,
  `compliance/*` already do this).
- Pure config: no Rust logic, no new rule kind, not
  spawn-capable (the `SPAWNING_RULE_KINDS` checklist item is
  N/A — a bundled ruleset can't introduce `command`-class
  rules anyway; `bundled.rs` forbids it, and
  `reject_command_rules_in` gates extends).
- Bundled-ruleset count `19 → 20` (README ×2 + the enumerated
  list at README:44 + `docs/site/about/index.md`); the
  **rule-kind count is unchanged (79)** — a bundled ruleset is
  composition, not a kind.

## Tests

- `coverage_audit_bundled_rulesets` contract: **one WELL-FORMED**
  scenario (a tree with LICENSE + ASF-NOTICE + KEYS + headered
  source + README + CHANGES ⇒ `violations: []`) and **one
  ILL-FORMED** scenario (missing NOTICE / a `.jar` in `src/` /
  no KEYS ⇒ concrete expected violations) under
  `crates/alint-e2e/scenarios/check/bundled/`, each with
  `given.config.extends: [alint://bundled/apache/governance@v1]`.
- A no-op check: the ruleset only fires on the artefacts it
  declares — a tree already satisfying them is silent (the
  well-formed scenario doubles as this).
- `docs/rules.md` gains an `### alint://bundled/apache/governance@v1`
  subsection under `## Bundled rulesets` and the ruleset is added
  to the README enumerated list; CHANGELOG `[Unreleased]` Added
  (the ninth v0.10 item — first bundled ruleset of the cut).
- Existing bundled-ruleset audits (`coverage_audit_bundled_rulesets`,
  the schema/registry round-trip) stay green; `xtask
  docs-export --check` unaffected (bundled rulesets aren't rule
  kinds). Full preflight + dogfood green.
- **Bench-compare threshold:** the ruleset adds no rule kind and
  no hot-path code; it is config a user opts into. No bench
  impact (`xtask bench-gate` per `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Podling / `DISCLAIMER`.** Incubating podlings must ship
   `DISCLAIMER` (or `DISCLAIMER-WIP`); graduated TLPs (the 3
   demand sources) must **not**. v1 targets graduated TLPs; a
   future `apache/incubator@v1` (or a `DISCLAIMER`
   `file_exists` a podling layers on) covers podlings.
   Deferred — no podling demand source.
2. **Relationship to `compliance/apache-2@v1`.** Resolved:
   distinct, composable, namespaced ids; governance is the
   *governance/release-discipline* superset, apache-2 the
   *license-redistribution* set. Docs cross-link; not merged
   (different adoption signals, different default levels).
3. **NOTICE copyright-year currency.** "NOTICE must name the
   current year" is a real ASF nicety but date-relative checks
   need a fact/clock; deferred to a possible future
   `when:`-gated rule. v1 checks attribution presence, not year.
4. **RAT config presence (`.rat-excludes` / RAT plugin in the
   build).** A build-file check (pom.xml / build.gradle for the
   RAT plugin) is a deeper, build-system-specific signal;
   `xml_path_*` (#7, now shipped) could express the Maven case
   in a future rev. v1 checks the *outcome* (headers,
   no-binaries), not the tooling. Deferred.
5. **Off-disk governance.** Branch protection, signed-release
   server state, PMC membership — not filesystem-visible, out of
   alint's scope by definition. Documented as a non-goal.
