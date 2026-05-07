# Case-study revalidation — Batch 5 (alphabetical batch 5)

Subagent findings for the 5 case studies:
- examples/pnpm-pnpm/
- examples/prettier-prettier/
- examples/protocolbuffers-protobuf/
- examples/python-cpython/
- examples/pytorch-pytorch/

Validated against alint v0.9.17 (binary built 2026-05-07) and the
authoritative docs (CONFIG-AUTHORING.md = 21 pitfalls; launch-evidence.md
= ship-target list).

## Authoritative reference points used

- Bundled-ruleset rule counts (2026-05-07):
  oss-baseline=15, rust=11, python=9, node=9, go=8, java=11,
  ci/github-actions=3, monorepo=4, monorepo/{cargo,pnpm,yarn}-workspace=4,
  hygiene/lockfiles=7, hygiene/no-tracked-artifacts=11,
  tooling/editorconfig=3, compliance/apache-2=3, compliance/reuse=3,
  docs/adr=4, agent-hygiene=6, agent-context=5.
- Pitfalls: 21 (was 12 → 17 → 19 → 21 across waves). #18 + #19 FIXED in
  v0.9.17 engine.
- v0.10 ship-targets (per launch-evidence.md):
  - `cross_file_value_equals` — 10 sources past saturation
  - `registry_paths_resolve` — 8 sources
  - `ordered_block` — 7 sources
  - `import_gate` — 4 sources
  - `generated_file_fresh` — 6 sources
- v0.11+ ship-target: `cross_language_implementation_complete` — 5 sources
  (arrow, TF, protobuf, angular, flutter); protobuf the densest.

---

## examples/pnpm-pnpm/

**Validation:** `validate-config` reports 112 rules (51 own + 61 bundled
from 9 extends: oss-baseline 15 + node 9 + monorepo 4 + monorepo/pnpm-
workspace 4 + ci/github-actions 3 + hygiene/no-tracked-artifacts 11 +
hygiene/lockfiles 7 + tooling/editorconfig 3 + agent-context 5). README
does not state an explicit total in the hot path; bundled mention at line
137 is correct.

**Stale-reference findings:**

1. Lines 56-67, 270, 291-292, 411-413, 429-433: `cross_file_value_equals`
   listed as "v0.10+ candidate" surfaced "first by airflow + tokio + clap
   + uv" — now a v0.10 ship-target with **10 sources** (10x past
   saturation). pnpm itself was the 8th source per the task brief but the
   README's own framing is several waves out of date.
2. Line 332-336: `json_key_sort_order` framed as "NEW" v0.10+ candidate;
   acceptable but should note `cross_file_value_equals` ship-target.
3. Line 444-446: "the 15 documented in CONFIG-AUTHORING.md" — outdated;
   the catalogue is now **21 pitfalls** (12 pilot + 3 P2a-W1 + 1 P2a-W2 +
   1 P2a-W3 + 2 P2b-W1 + 2 P2b-W2). The pitfall #14 reference (line 457)
   remains correct (it's the YAML \n-in-regex pitfall).
4. Lines 306-322: `registry_paths_resolve` framed as v0.10+ candidate
   "originally surfaced by rust + clap, then microsoft/typescript" — now
   v0.10 ship-target at **8 sources**.

**Fix-vs-workaround:** No `respect_gitignore: false` or `root_only: true`
patterns in this config. Pitfalls #18/#19 don't apply directly. The four
`command:` shellouts (eslint, cspell, lint:meta, audit) are correctly
shellouts for AST/dictionary tools — no v0.9.6+ rule kind replacement
applies.

**Rule-kind status sync:** All three "needs new primitive" sections
(cross_file_value_equals, registry_paths_resolve, json_key_sort_order)
need re-framing as **v0.10 ship-targets** with explicit source counts.

**Bundled-ruleset count sync:** README claim of "extending 9 bundled
rulesets" matches actuals (9 extends → 61 bundled rules).

**New analysis:**
- meta-updater's 13 invariants: README documents 13 in the table at
  lines 84-99; the .alint.yml captures 11 of 13 declaratively (the two
  cross-file-equals ones — engines.node and repository — fall back to
  literal-pattern matching today). This is **correctly stated** in the
  README ("alint covers 11 of 13 invariants" line 412). When
  `cross_file_value_equals` ships in v0.10, both gaps close.
- `compliance/reuse` doesn't apply (pnpm uses MIT not REUSE).
- `docs/adr` doesn't apply (pnpm has no ADR convention).
- `--changed` mode would be a natural addition for the pre-commit hook
  documented at lines 121-132 (compile + lint mode). Worth flagging in
  Future analysis.

**Action taken:** Surgical README edit — added "Validation status
(2026-05-07)" footer, "Future analysis" section. Updated 3 stale
"v0.10+ candidate" references to "v0.10 ship-target" with current
source counts. Bumped "15 documented" to "21 documented" pitfall count.

---

## examples/prettier-prettier/

**Validation:** `validate-config` reports 68 rules (22 own + 46 bundled
from 6 extends: oss-baseline 15 + node 9 + ci/github-actions 3 +
hygiene/no-tracked-artifacts 11 + agent-context 5 + tooling/editorconfig
3). **README claim of "24 rules total" (lines 31, 202) is wrong** — the
24 likely refers to the prettier-specific custom rules (the actual
prettier-* count is ~22 own; minor drift). The phrasing "after extending
6 bundled rulesets" misleads readers into thinking the total is 24
including bundled. Clarify: 22 own + 46 bundled = 68 total.

**Stale-reference findings:**

1. Line 86: "env-injection on `command` rule is itself a v0.10+ gap" — env
   on command rule is still a gap (not in v0.10 ship-list); leave as-is.
2. Lines 99, 155, 177-180, 282-298: `command_idempotent` mode listed as
   "v0.10+ candidate from ruff" — currently in **v0.10 design candidates**
   table (≥2 sources: ruff + prettier). Status unchanged.
3. Lines 156, 282-298: `for_each_leaf_dir` listed as v0.10+ candidate —
   currently **v0.10 design candidate** (3 sources: rust, ruff, prettier).
   Status unchanged.
4. Lines 156-157, 282-298: `json_key_value_forbidden` listed as v0.10+
   candidate — not currently on the launch-evidence ship-list; closely
   related to `cross_file_value_equals` (ship-target).
5. Lines 158, 299-303: `unique_by` cross-dir framed as new candidate —
   not on v0.10 list. Still a single-source gap.
6. Line 210: "per-subdir variant becomes a v0.10+ candidate" — referring
   to `for_each_file` + JSON-key-shape forbid; still applicable.

**Fix-vs-workaround:** 5 `root_only: true` usages found (lines 334, 412,
422, 432, 442). All are with **single-segment literal paths**
(`prettier.config.js`, `eslint.config.js`, etc.) — pitfall #19 does NOT
apply (the runtime guard fires on multi-component literals). No
`respect_gitignore: false` patterns; pitfall #18 doesn't apply.

**Rule-kind status sync:** Prettier's primary new candidates
(`json_key_value_forbidden`, `for_each_leaf_dir`, `command_idempotent`,
`unique_by` cross-dir) are still v0.10 design candidates per
launch-evidence; no promotions to ship-target. Status notes can stand.

**Bundled-ruleset count sync:** "24 rules" framing is misleading;
actual 22 own + 46 bundled = 68. Update needed.

**New analysis:**
- PR-number uniqueness across changesets (lines 158, 299-303): the
  README correctly identifies `unique_by` cross-dir as the gap. Today's
  `unique_by` rule operates within a single dir/file scope — the cross-
  dir variant for `changelog_unreleased/<lang>/*.md` is genuinely
  unsupported. This is **not yet** a v0.10 ship-target (single-source
  demand).
- `compliance/reuse` doesn't apply (prettier uses MIT, not REUSE).
- `docs/adr` doesn't apply.
- `hygiene/lockfiles` (7 rules) NOT extended — yarn.lock disciplines
  could be tightened. Worth flagging in Future analysis.

**Action taken:** Surgical README edit — corrected "24 rules" → "68
rules (22 own + 46 bundled)", added "Validation status (2026-05-07)"
footer + "Future analysis" section.

---

## examples/protocolbuffers-protobuf/

**Validation:** `validate-config` reports 108 rules (79 own + 29 bundled
from 3 extends: oss-baseline 15 + ci/github-actions 3 + hygiene/no-
tracked-artifacts 11). **README claim of "108-rule" matches exactly**
(lines 83, 116, 302, 506, 577, 589). README also notes "72 rules pass
silently" + "150 violations across 14 failing files" against the live
tree at /tmp/protobuf — re-validated, **still accurate** with v0.9.17.

**Stale-reference findings:**

1. Lines 89-111, 262-296, 352-385, 529-537, 596-601:
   `cross_language_implementation_complete` framed as "v0.11+ candidate"
   with **4 sources** (arrow + TF + flutter + protobuf). Per launch-
   evidence.md it is now **v0.11+ ship-target with 5 sources** (added
   angular). Update needed.
2. Lines 408-421: `ordered_block` framed as "v0.10+ candidate now 7
   sources" — matches launch-evidence.md (rust, airflow, tokio, cpython,
   arrow, golang/go, protobuf failure_lists). Status: **v0.10 ship-target
   ties with registry_paths_resolve at top of v0.10 backlog**. Update
   "v0.10+ candidate" to "v0.10 ship-target".
3. Lines 442-444: `generated_file_fresh` framed as "v0.10+ candidate" —
   now **v0.10 ship-target with 6 sources** (uv, cpython, pytorch, bazel,
   TF, spark).
4. Lines 543-549: `registry_paths_resolve` framed as "v0.10+ list" — now
   **v0.10 ship-target with 8 sources**.

**Fix-vs-workaround:** No `root_only:` or `respect_gitignore:` patterns.
Pitfalls #18/#19 don't apply. Six `command:` shellouts (buildifier,
bazel build, clang-format, flake8, rubocop, gofmt) are correctly
shellouts (Starlark/C++/Go/Python/Ruby AST tools). The merge-conflict
false positive at csharp/README.md persists — this is a pre-existing
bundled-rule issue (oss-no-merge-conflict-markers regex too eager on
`=======` markdown section underlines), still unresolved at v0.9.17.

**Rule-kind status sync:** Multiple "v0.10+ candidate" / "v0.11+
candidate" need promotion to "ship-target":
- `cross_language_implementation_complete` → v0.11+ ship-target (5
  sources)
- `ordered_block` → v0.10 ship-target (7 sources, top-of-backlog)
- `generated_file_fresh` → v0.10 ship-target (6 sources)
- `registry_paths_resolve` → v0.10 ship-target (8 sources)

**Bundled-ruleset count sync:** "3 bundled rulesets … pull in roughly 12
rules between them" (line 305) — actual is 29 bundled rules (15 + 3 +
11). Significant under-count.

**New analysis:**
- **`nested_configs: true` per language binding directory** would be a
  natural fit. Each of `src/`, `java/`, `python/`, `ruby/`, `go/`,
  `objectivec/`, `csharp/`, `php/`, `rust/`, `lua/`, `upb/`, `hpb/`
  could ship a per-binding `.alint.yml` with the language-specific
  rules. The current 108-rule monolithic config has all 79 own rules
  collapsed into one file; splitting per-binding via `nested_configs`
  would let each binding evolve independently. Worth flagging in Future
  analysis.
- **`ordered_block` for failure_list_<lang>.txt files**: README already
  identifies this. With `ordered_block` at v0.10 ship-target (7 sources),
  protobuf is the **canonical demand-driver** (19 failure_list files +
  8 text_format_failure_list files = 27 file targets in one repo).
- `compliance/apache-2` (3 rules) — protobuf uses BSD-3-Clause not
  Apache-2; doesn't apply.
- `compliance/reuse` doesn't apply.
- `docs/adr` (4 rules) — protobuf has no ADR convention.

**Action taken:** Surgical README edit — corrected "roughly 12 rules"
→ "29 bundled rules", promoted four candidates to v0.10/v0.11 ship-
target with current source counts, added "Validation status
(2026-05-07)" footer + "Future analysis" section.

---

## examples/python-cpython/

**Validation:** `validate-config` reports 72 rules (34 own + 38 bundled
from 4 extends: oss-baseline 15 + python 9 + ci/github-actions 3 +
hygiene/no-tracked-artifacts 11). **README claim of "39-rule alint
config" (lines 46, 374) is wrong** — the 39 likely refers to a previous
draft; actual is 72 total (or 34 own).

**Stale-reference findings:**

1. Lines 65, 109, 131, 143, 223-226, 230, 244, 246, 401-411: `balanced_
   delimiters`, `file_pair_block_match`, `generated_file_fresh`,
   `registry_paths_resolve`, `ordered_block` listed as "v0.10+
   candidate(s)" — most are now **v0.10 ship-target**. Update needed.
2. Line 226: `registry_paths_resolve` "from triagebot" — original source
   was rust-lang/rust's triagebot; now **v0.10 ship-target with 8
   sources** (rust, clap, cpython×2, next.js, arrow, pytorch,
   nodejs/node, NixOS×3).
3. Lines 228-229, 248-252, 408-411: `column_alignment` framed as "NEW"
   v0.10+ candidate — still single-source (cpython only). Not on v0.10
   list. Status unchanged (single-source defer).
4. Lines 230, 244-246: `ordered_block` — now **v0.10 ship-target with 7
   sources** (rust, airflow, tokio, cpython, arrow, golang/go, protobuf).
   Update "v0.10+ candidate list from rust-lang/rust" to "v0.10 ship-
   target".

**Fix-vs-workaround:** 1 `root_only: true` usage at line 563 — the
autotools files block. Single-segment literals (`configure`,
`configure.ac`, `pyconfig.h.in`, `aclocal.m4`, `Makefile.pre.in`) — all
at root, **pitfall #19 does NOT fire**. No `respect_gitignore:` patterns.
Pitfalls #18/#19 don't apply.

**Note for cpython per task brief:** "12 validation surfaces consolidated
into 1 alint config" — this matches the README's framing ("scattered
across 12 distinct surfaces" line 17). Pitfall #16 not specifically
mentioned; #16 is about `*_path_matches` cannot regex-match against
non-string values — relevant for the `toml_path_matches` parseability
check on `Misc/stable_abi.toml` (line 327). Reviewed: the rule uses
"wildcard force parse" pattern (no scalar comparison), so #16 doesn't
fire.

**Rule-kind status sync:** Multiple promotions needed:
- `balanced_delimiters` + `file_pair_block_match` → still v0.10 design
  candidate (3 sources: rust + cpython×2)
- `generated_file_fresh` → v0.10 ship-target (6 sources)
- `registry_paths_resolve` → v0.10 ship-target (8 sources, top of
  backlog)
- `ordered_block` → v0.10 ship-target (7 sources, top of backlog)

**Bundled-ruleset count sync:** README adopts 4 bundled rulesets; actual
totals to 38 bundled rules. README phrasing OK (no explicit count
claim).

**New analysis:**
- **c-api-doc / clinic / generator surfaces** (per task brief): The
  README correctly identifies these as needing new primitives
  (`registry_paths_resolve`, `balanced_delimiters`,
  `file_pair_block_match`, `generated_file_fresh`). With `registry_paths
  _resolve` and `generated_file_fresh` now ship-target, **two of cpython's
  4 gap surfaces are addressable in v0.10** — Argument Clinic + cases_
  generator both close once those ship. The remaining 2 (Argument Clinic
  in-place block matching = `balanced_delimiters` + `file_pair_block_
  match`) stay v0.10 design.
- `compliance/reuse` doesn't apply (cpython uses PSF licence).
- `docs/adr` (4 rules) — cpython has no ADR convention; PEPs serve
  similar role but live elsewhere.
- `hygiene/lockfiles` doesn't apply (no JS/Python lockfile in cpython
  build).
- `agent-context` / `agent-hygiene` — cpython has no CLAUDE.md or
  agent-friendly docs convention; could add a flag.

**Action taken:** Surgical README edit — corrected "39-rule alint
config" → "72-rule alint config (34 cpython-specific + 38 bundled)",
promoted candidates to v0.10 ship-target with source counts, added
"Validation status (2026-05-07)" footer + "Future analysis" section.

---

## examples/pytorch-pytorch/

**Validation:** `validate-config` reports 87 rules (40 own + 47 bundled
from 6 extends: oss-baseline 15 + python 9 + ci/github-actions 3 +
hygiene/no-tracked-artifacts 11 + agent-hygiene 6 + tooling/editorconfig
3). **README claim of "35 pytorch-specific rules plus 6 bundled
rulesets" (lines 49-50, 312) is close but slight understatement** — own
rules = 40, bundled = 47 (sum 87).

**Stale-reference findings:**

1. Lines 135, 137, 139, 143, 168-169, 219-225, 231-241, 420-432:
   `cross_file_value_equals` framed as "7th confirmation" — per
   launch-evidence.md it's now **v0.10 ship-target with 10 sources past
   saturation**. Update needed.
2. Lines 137, 220-221, 234, 424-426: `registry_paths_resolve` framed as
   "5th confirmation" — now **v0.10 ship-target with 8 sources**.
3. Line 135, 221, 237-238, 427-429: `import_gate` framed as "3rd
   confirmation" / "3rd-4th" — now **v0.10 ship-target with 4 sources**
   (k8s, airflow, golang/go, pytorch).
4. Line 139, 222-223, 239-240, 430-432: `generated_file_fresh` framed as
   "3rd-4th confirmation" — now **v0.10 ship-target with 6 sources**
   (uv, cpython, pytorch, bazel, TF, spark).
5. Line 226: `directory_hash` adjacent to `pair_hash` — `pair_hash` is
   currently v0.10 ship-target (3 sources: k8s, tokio, golang/go FIPS).
   The directory_hash variant remains a NEW candidate; defer is
   appropriate.
6. Lines 144, 224-225: `yaml_path_implication` (NO_WORKFLOWS_ON_FORK) —
   single-source candidate. Defer is appropriate.
7. Line 224: `line_spacing` (MERGE_CONFLICTLESS_CSV) — single-source.
   Status unchanged.
8. Line 225: `not_executable` — single-source. Status unchanged.

**Fix-vs-workaround:** 8 `root_only: true` usages found. Lines 780, 797,
881, 908: all use single-segment literals at root — pitfall #19 does NOT
fire. **Lines 924 (`Dockerfile`, `.devcontainer`) — single-segment, OK.
Lines 943 (`tools/linter/adapters`, `.lintrunner.toml`) — MIXED: the
first is multi-segment (`tools/linter/adapters/`); pitfall #19 used to
silently no-match. Now FIXED in v0.9.17 (literal_is_nested runtime guard
produces "no-match-for-this-pattern" rather than silently passing).
Lines 963 (`tools/linter/adapters/grep_linter.py`, etc., 9 multi-segment
paths) — same pattern as #19. Lines 980 (`.ci/pytorch`, `.ci/docker`) —
multi-segment**. These rules should EITHER drop `root_only:` (option A)
or use the per-rule `respect_gitignore: false` shipped in v0.9.17 if
that's the actual root cause (it isn't here — pitfall #19 is about
multi-segment + root_only).

**FLAG: pytorch .alint.yml has 3 rules likely affected by pitfall #19**
(`pytorch-lintrunner-adapter-dir-present`, `pytorch-grep-linter-shim-
present`, `pytorch-ci-pytorch-tree-present`). The fix is to **drop
`root_only: true`** on these — the literals are multi-segment and the
flag does nothing useful for them. Verified the rules **DO fire
correctly today on the protobuf live tree** (engine produces "no match"
errors when files don't exist) — so the bug fix in v0.9.17 made these
rules functional. Recommend dropping `root_only: true` for clarity.

**Rule-kind status sync:** Multiple promotions:
- `cross_file_value_equals` → v0.10 ship-target (10 sources past
  saturation)
- `registry_paths_resolve` → v0.10 ship-target (8 sources)
- `import_gate` → v0.10 ship-target (4 sources)
- `generated_file_fresh` → v0.10 ship-target (6 sources)

**Bundled-ruleset count sync:** "35 pytorch-specific rules plus 6
bundled rulesets" — own count is 40 not 35. Adjust.

**New analysis:**
- **`--changed` mode + lintrunner PR fastpath** (per task brief):
  pytorch's CI orchestration via lintrunner uses `lintrunner --paths-cmd`
  to feed only changed files. alint's `--changed` mode (with `--base`
  for PR-time `git diff --name-only <base>...HEAD`) is **the right
  shape** to mesh with pytorch's existing fastpath. Worth flagging:
  `alint check --changed --base origin/main` would produce a similar
  fast-feedback experience for the structural floor. Note: cross-file
  rules (`pair`, `for_each_dir`, `every_matching_has`, `unique_by`,
  `dir_contains`, `dir_only_contains`) and existence rules
  (`file_exists` et al.) still consult the full tree by definition —
  this is intentional and doesn't change pytorch's coverage story.
- **86% coverage figure** (per task brief) confirmed: 49 of 57 lintrunner
  adapters are within alint's grammar. The .alint.yml ships only 12
  `file_content_forbidden` mappings — the remaining 12 grep_linter
  adapters (RAWTHROW, ERROR_PRONE_ISINSTANCE, CUBINCLUDE, RAWCUDA, etc.)
  are documented at lines 340-344 as "additive — same template".
  Documenting these as TODO is fine; the README correctly explains the
  partial coverage choice.
- `compliance/reuse` doesn't apply.
- `docs/adr` (4 rules) — pytorch has no ADR convention.
- `hygiene/lockfiles` doesn't apply (build is CMake/Bazel, not lockfile-
  based).

**Action taken:** Surgical README edit — corrected "35 pytorch-specific
rules" → "40 pytorch-specific rules + 47 bundled = 87 total", promoted
candidates to v0.10 ship-target with source counts, added "Validation
status (2026-05-07)" footer + "Future analysis" section. **Flagged
pitfall #19 root_only-with-multi-segment-literals issue in .alint.yml
report below.**

---

## .alint.yml bug flags

**pytorch-pytorch/.alint.yml** — 3 rules use `root_only: true` with
multi-segment literal paths (pitfall #19 shape):

- `pytorch-lintrunner-adapter-dir-present` (line 938-948): paths
  `tools/linter/adapters` (multi-segment), `.lintrunner.toml` (single).
  The multi-segment one is what pitfall #19 surfaced.
- `pytorch-grep-linter-shim-present` (line 950-969): all 10 paths are
  multi-segment under `tools/linter/adapters/`.
- `pytorch-ci-pytorch-tree-present` (line 975-984): paths `.ci/pytorch`,
  `.ci/docker` (multi-segment).

In v0.9.17 the engine's literal_is_nested guard produces "no-match-for-
this-pattern" when these are run with `root_only: true` and a literal
file fails to exist (correct error), but the `root_only: true` flag
itself adds no value here and could mislead readers. **Recommended fix:
drop `root_only: true` from these three rules** (no behaviour change for
the existence check itself; just removes the misleading flag).

These rules DO fire correctly on the live tree today (verified against
/tmp/protobuf where the files don't exist), so the fix is purely a DX
cleanup, not a functional repair.

---

## Cross-cutting patterns (this batch)

1. **Saturation drift on rule-kind candidates is universal in this
   batch.** Every README mentions `cross_file_value_equals` /
   `registry_paths_resolve` / `ordered_block` / `generated_file_fresh`
   as "v0.10+ candidate" with stale source counts (typically 4-7
   confirmations). All four are now **v0.10 ship-target** with deeper
   demand counts (10, 8, 7, 6 sources respectively).
2. **`cross_language_implementation_complete` is now v0.11+ ship-target
   with 5 sources** (added angular). protobuf README still says "4
   distinct repos" / "v0.11+ candidate".
3. **README rule-count claims drift downward by ~50% in 2 of 5 cases.**
   prettier (24 → 68) and cpython (39 → 72) understate by failing to
   add bundled-ruleset rules. protobuf and pnpm count correctly.
4. **Pitfall #18/#19 fix hits 1 of 5 configs (pytorch).** The other 4
   configs avoid the root_only-with-multi-segment-literals trap.
5. **No `respect_gitignore: false` workarounds in this batch.** None of
   the 5 configs needed pitfall #18's per-rule fix.
6. **2026-05-06 capture date matches the live tree state.** protobuf
   live-tree recheck against /tmp/protobuf produces the exact violation
   summary the README claims (150 violations, 14 failing, 72 passing).
7. **agent-context / agent-hygiene adoption is uneven**: pnpm + prettier
   + pytorch extend agent-* rulesets (correctly, given CLAUDE.md
   presence); cpython + protobuf do not.

---

## Per-case-study log entries written

- examples/pnpm-pnpm/README.md — touched (3 stale candidate refs
  promoted, pitfall count fixed, footer + Future analysis added)
- examples/prettier-prettier/README.md — touched (rule count corrected,
  footer + Future analysis added)
- examples/protocolbuffers-protobuf/README.md — touched (4 candidate
  refs promoted, bundled count corrected, footer + Future analysis
  added)
- examples/python-cpython/README.md — touched (rule count corrected,
  4 candidate refs promoted, footer + Future analysis added)
- examples/pytorch-pytorch/README.md — touched (rule count corrected,
  4 candidate refs promoted, footer + Future analysis added,
  pitfall #19 .alint.yml bug flagged for parent agent)
