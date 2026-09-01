# Design doc: baseline mode (grandfathering existing violations)

Status: Implemented. Shipped across the slice series (engine, fingerprint, CLI),
the per-rule `baseline_key` audit (v3), the `baseline:` config key, and the
per-format SARIF suppression marking + JSON `baselined_suppressed` (§3.8 — the
final piece). The last sub-item — unifying the `gitlab.rs` fingerprint onto
`violation_fingerprint` — is now **RESOLVED** (v0.14 deferral close-out,
2026-07-05; §5, §7). (Draft | Implemented | Superseded by <doc>.)
Decisions: [ADR-0006](../adr/0006-baseline-suppression.md) — **Accepted (2026-06-21)** (new persistent suppression mechanism; affects pass/fail semantics).
Demand evidence: External adoption evaluation, §4.1 — *"the single feature that makes ESLint/RuboCop/etc. adoptable on legacy code, and its absence is the #1 thing that stops a team from flipping alint on as a merge gate."* Reproduced firsthand against alint 0.13.0.
Status detail: Implemented and merged to `main` (#88–#94, 2026-06-24); landed after the v0.13.0 cut, so it ships in the next release. All of §7 is RESOLVED — Q7 (`gitlab.rs` fingerprint unification) closed in the v0.14 deferral close-out (2026-07-05; §5, §7).

> **v3 changelog (implementation-time audit).** Running the (then-draft)
> fingerprint against the *whole firing scenario corpus* (all 89 registered
> kinds) showed the v2 model over-scoped the per-rule key: its §6 predicate
> ("line-anchored-unique **or** keyed") demands a `baseline_key` on ~50 path-only
> rules (`file_exists`, `dir_exists`, …) that are **already** uniquely and stably
> identified by `(rule_id, path)`. v3 (a) makes the **default discriminator for a
> path-bearing, key-less, line-less violation empty** — its identity is
> `(rule_id, path)`, and the message is **no longer hashed** (§3.1); this finally
> makes the message *literally* non-load-bearing (the v2 goal) and fixes
> threshold churn (`file_max_lines` no longer re-fires as the file grows) for free
> with **no** per-rule key; (b) **narrows** the explicit-key requirement to the
> ~15 kinds whose identity genuinely isn't `(rule_id, path)`: structured-query
> (multiple per file / volatile matched value), cross-file & no-path (would
> collapse under the empty default), first-offender (cross-edit churn), and
> line-collision (`markdown_paths_resolve`); (c) recasts the §6 gate as a
> **structural collision-invariant** — *no two distinct live violations of a kind
> may share a fingerprint, and none may fall through to the message* — checked
> across the corpus plus targeted multi-finding fixtures, so it can't false-green
> a kind whose fixture happens not to exercise the unsafe shape. See ADR-0006
> (amended).

> **v2 changelog.** A review found the v1 fingerprint model did not fit several
> shipped rules (`structured_path` emits N path-only violations per file;
> `file_max_lines` is path-only with the threshold in the message; the
> line-ending / trailing-whitespace rules report only the first offender). v2
> (a) **generalizes the per-rule `baseline_key`** to any violation, not just
> no-path ones (§2.4, §3.1); (b) makes suppression **mark-not-remove** so SARIF
> doesn't flap GitHub Code Scanning alerts (§3.3, §3.8); (c) introduces a single
> `violation_fingerprint` in `alint-core`, used by the SARIF `partialFingerprints`
> (the `gitlab.rs` migration onto it is still outstanding — §5); (d) corrects
> the false path-confinement claim and specifies the real `extends:`/nested
> enforcement (§5); (e) guards `baseline --changed` and wholesale regeneration
> (§2.2); (f) switches the file to **JSON Lines** for merge-friendliness and adds
> an advisory, reviewable `message` per entry (§2.1); (g) specifies error and
> exit-code behaviour (§3.9).

## 1. Problem

On a large, long-lived repository, turning on a ruleset surfaces hundreds of
pre-existing violations at once. Today the only levers are `level: off`, `when:`
gates, and `ignore:`/exclude globs — **all of which also suppress future, new
violations of the same rule**. There is no "accept the current state, fail only
on *new* violations" mode. A team that wants alint as a blocking merge gate must
either fix everything first (often hundreds of findings) or disable the rule
(losing the gate). Neither is acceptable, so they don't adopt.

This is the standard grandfathering / baseline capability every mature linter
ships: **RuboCop** (`.rubocop_todo.yml`, per-`(cop, file)` counts), **ESLint**
(bulk suppressions, per-`(rule, file)` counts), **PHPStan / Psalm / detekt**
(committed baseline files of per-issue fingerprints or per-file counts), and
**golangci-lint** (`--new-from-rev`, a diff-based variant; see §3.5).

**Partial workaround today:** `--changed` / `--base` lints only files in the
diff. But it is a *scope* filter, not a baseline: cross-file rules (`pair`,
`for_each_dir`, `file_graph`, `unique_by`, `registry_paths_resolve`, …) and
existence rules (`file_exists` et al.) consult the **full tree** by definition,
so they still fire on legacy state. A baseline *complements* `--changed`; it does
not duplicate it (§3.5).

Without baseline mode, alint is adoptable on greenfield repos and as an advisory
check, but not as a blocking gate on a large existing codebase — the case where a
structural linter has the most value.

## 2. Surface area

No change to how rules are *written* in `.alint.yml`. The additions are: a
committed baseline file, two subcommands/flags, a per-violation `baseline_key`,
and a `baseline:` config key.

### 2.1 The committed baseline file — JSON Lines

Default path `.alint-baseline.json`, written by the tool and committed.
**JSON Lines** (one JSON object per line), not a single JSON array: a sorted,
one-entry-per-line file is **merge-friendly** (two PRs that each grandfather a
disjoint entry don't conflict on shared brackets/commas — the v1 single-array
shape conflicted on every concurrent addition). Line 1 is a header; the rest are
entries, sorted by `(rule_id, path, fingerprint)`:

```
{"schema_version":1,"alint_version":"0.x.y"}
{"rule_id":"no-todo-comments","path":"src/legacy/api.ts","fingerprint":"<64-hex>","count":3,"message":"TODO without an owner"}
{"rule_id":"lockfiles-only-one","path":null,"fingerprint":"<64-hex>","count":1,"message":"Multiple lockfiles found: package-lock.json, yarn.lock"}
```

- `fingerprint` — the full 64-hex SHA-256 (§3.1). **Not truncated** (a fingerprint
  collision is a *silent mis-suppression*, strictly worse than a normal hash
  collision; resolves §7-Q6). It is the **only** field used for matching.
- `count` — collapses identical fingerprints (§3.2).
- `path` — repo-relative, forward-slashed, or `null` for repo-level violations.
  **Advisory only** (for human review and the sort key); not matched on (the path
  is already inside the fingerprint).
- `message` — the rendered violation text. **Advisory only, never matched.** It
  exists so a reviewer reading the `.alint-baseline.json` diff in a PR can see
  *what* is being grandfathered (the v1 hash-only entry made the "PR-reviewed
  artifact" safety argument hollow). Because it is non-matching it may drift
  freely; it is refreshed on the next `alint baseline` and excluded from any
  `--check`-style comparison.
- `schema_version` — gated: a file whose `schema_version` the binary doesn't
  recognise is a hard error (§3.9), never silently ignored (a newer file may use
  a fingerprint scheme this binary computes differently → silent mis-suppression).
- `alint_version` — advisory provenance; **excluded** from byte-identical
  regeneration and from any `--check` comparison so it can't trip a drift gate.

Sort order makes an unchanged tree regenerate byte-identically (modulo the
advisory header).

### 2.2 `alint baseline` — write the baseline

```
alint baseline [PATH] [--output <file>] [--accept-new]
```

Runs the same whole-tree evaluation as `check`, then writes the current
violations to the baseline file. Default output is the `baseline:` config-key
path if set, else `.alint-baseline.json` (so the writer and the reader never
split-brain onto different files; resolves review M5).

- **Rejects `--changed`/`--base`** with a clear error. A baseline must be
  whole-tree; a changed-scope run would capture only the diff's per-file
  violations and silently omit the rest of the legacy tree, producing a baseline
  that suppresses an arbitrary subset (review H3/H4). Whole-tree only.
- **Regeneration guard.** If a baseline file already exists, `alint baseline`
  computes the delta and **refuses to grandfather new violation _occurrences_
  unless `--accept-new` is passed** — a brand-new fingerprint *or* a higher
  `count` on an existing one (both are fresh debt). It always prints
  `+N would be grandfathered / -M stale removed` (N/M counted in occurrences).
  *Pruning stale entries is always safe and happens without the flag; accepting
  NEW debt is explicit.* This closes the happy-path footgun where re-running
  `baseline` (which §3.4 may prompt) silently grandfathers everything introduced
  since the last run (review H5). First-time creation (no existing file) writes
  freely.
- Exits 0 on success; 2 on a write/IO failure (§3.9).

### 2.3 `alint check --baseline <file>` — enforce only the delta

```
alint check --baseline <file> [--strict-baseline] [--show-baselined] [other flags]
```

Suppresses every current violation whose fingerprint is present (up to its
recorded count) in the baseline; reports only **new** violations, which alone
drive the exit code (§3.3). A one-line stderr summary reports the suppressed
count; the global `--show-baselined` lists the suppressed findings in full
(parallel to the existing `--show-notes`); stale entries warn (§3.6).
`--strict-baseline` makes stale entries fail (§3.6). `--show-baselined` /
`--strict-baseline` are global flags (like `--fail-on-warning`/`--show-notes`)
and are no-ops without a baseline in effect.

A `baseline: <path>` key in `.alint.yml` persists the baseline so CI need not
pass the flag; `--baseline` overrides it. There is **no silent auto-detect** of
`.alint-baseline.json` — suppression is always an explicit, reviewable opt-in (a
committed `baseline:` line or an explicit flag), never triggered by a file merely
existing (resolved §7-Q1). `fix` does **not** take `--baseline` in v1 (review
M3 / §3.4).

### 2.4 A per-violation structural baseline key

`Violation` gains an optional `baseline_key: Option<Cow<'static, str>>`. It is the
rule's declaration of *what makes this violation distinct* — used as the
fingerprint discriminator (§3.1) in preference to the default. v1 scoped this to
no-path violations; v2 over-corrected to "any rule whose `(path, line-content)`
isn't a unique identity." v3 (the implementation-time audit) restores the right
scope: with the **default discriminator now `(rule_id, path)`** for a path-bearing
violation (§3.1 branch 3), a rule sets `baseline_key` **only** when its identity
genuinely isn't `(rule_id, path)` — i.e. when it emits *more than one* finding per
`(rule_id, path)`, has *no* path, or is a *first-offender* line rule:

- **Structured-query** (`json/yaml/toml/xml/dotenv/properties/ini/hcl_path_*`): emit N path-only violations
  per file, so the empty default would collapse them — and the message embeds the
  matched *value*, which churns. Key = the JSONPath + operator (+ expected value
  for `_equals`). *(This was the headline review bug.)*
- **No-path / cross-file** (`lockfiles-only-one`, `pair`, `pair_hash`,
  `file_graph`, `unique_by`, `registry_paths_resolve`, `cross_file`,
  `cross_file_value_equals`, `dir_absent`, `generated_file_fresh`): no single
  `path`, or several findings per path, so the default would collapse them.
  Key = the sorted set of involved repo-relative paths (or the per-finding path).
- **First-offender / first-match rules** (`no_trailing_whitespace`,
  `line_endings`, `line_max_width`, `file_content_forbidden`): report only the
  first offending line (or first match) per file. Their identity is
  `(rule, file)`, not a line's content (content-hashing churns when the first
  offender is fixed and the second surfaces, or when the offending line is
  edited but stays an offense). Key = the path. The trade-off — a *file-level*
  acceptance window, wider than content-keying — is disclosed in §4.
- **Line-collision** (`markdown_paths_resolve`): emits several findings on one
  line (e.g. two broken links), so line-content alone collapses them. Key = the
  per-finding target (the unresolved path/link).

What **no longer** needs a key (v3): **threshold / whole-file** rules
(`file_max_lines`, `file_min_lines`, `max_directory_depth`, …) — the empty default
makes their identity `(rule_id, path)`, stable as the magnitude grows (the "same
accepted finding"; see the "ratchet" note in §4), with the volatile magnitude no
longer in the hash. And the bulk of **single-finding path-only** rules
(`file_exists`, `dir_exists`, `file_hash`, `file_content_matches`, …) and the
**line-content** rules (`for_each_match`, `commented_out_code` — several
findings per file, each on its own line): the default discriminator (§3.1)
covers them. (The first-offender `line_max_width` / `file_content_forbidden`
were previously grouped here on a "one finding per file" rationale; v3.1 moves
them to the path-keyed first-offender bucket above for consistency with the
other first-offender rules — see §4.) The §6 collision-invariant enforces
the boundary so a new kind can't silently fall into an unsafe default.

## 3. Semantics

### 3.1 The fingerprint (the crux)

A fingerprint must survive benign code motion (inserting lines elsewhere, moving
content) without churning, yet **re-trigger when the offending code is edited**
and never mask a genuinely *new* violation. It is a SHA-256 (sha2 is already a
workspace dep) over a length-prefixed join of three components:

```
fingerprint = sha256( lp(rule_id) ‖ lp(path_or_empty) ‖ lp(discriminator) )
```

`lp(x)` = the 4-byte little-endian byte length of `x` followed by `x`'s bytes, so
no two distinct component tuples can collide by concatenation (the v1 doc
asserted this without specifying it; the existing `gitlab.rs` fingerprint uses a
bare `|` and *does* have the collision — see §5). The **discriminator** is chosen
per violation, in order:

1. **`baseline_key`** if the rule set one (§2.4) — the rule's own stable identity.
2. else, for a **line-anchored** violation (`line.is_some()`), the **offending
   line's content with its trailing `\r?\n` stripped** (so CRLF↔LF conversion
   doesn't churn the whole repo's baseline; line *number* is excluded so inserts
   above don't churn). The line bytes are hashed as raw bytes (non-UTF-8 safe).
3. else, for a **path-bearing** violation (`path.is_some()`, no line, no key), the
   **empty discriminator** — its identity is `(rule_id, path)` alone (v3). The
   message is deliberately **not** hashed: it is frequently volatile (a magnitude
   like `file_max_lines`' line count, a structured-query matched value, command
   output), and `path` (already hashed as component 2) uniquely identifies a
   rule that emits one finding per path. A rule that emits *multiple* findings per
   `(rule_id, path)`, or no path at all, must instead set a `baseline_key` (§2.4).
4. else (no path, no line, no key) the **normalised message** — a last-resort
   **anti-panic** fallback only. The §6 audit forbids any kind from relying on it
   (such a violation must set a `baseline_key`); it exists so an un-audited kind
   collapses *loudly* (caught by the §6 collision-invariant) rather than panicking.

`column` is never in the hash (line-granular is the right churn/precision balance).
`level` is never in the hash (§4 — a baseline accepts the *finding*, not its
severity). `is_note` violations are notes, not violations, and are never
fingerprinted (the transform runs on `RuleResult.violations` only, after the
existing notes partition).

Self-referential rules (`line_endings`, `no_trailing_whitespace`) take branch 1
via a path key, so the terminator-stripping in branch 2 never erases the bytes
they police.

### 3.2 Counting identical fingerprints

Distinct violations that share a fingerprint (e.g. N matching lines with
byte-identical content, or N matches of one structured query) collapse to one
entry with `count: N`. Matching is count-aware: a run producing *k* violations
with fingerprint *F* suppresses `min(k, baseline[F].count)`; any beyond that are
**new**. This is the RuboCop/ESLint count model applied per-fingerprint.

Residual masking (documented, accepted): at or below the recorded count, a *new*
violation whose content is byte-identical to a baselined one is indistinguishable
from the old and stays suppressed; only the `(count+1)`th such identical
occurrence is reported. Distinct content/keys are never confused — for
content-keyed rules this is the narrowest possible masking window and is the
price of line-content (vs line-number) keying. (The path-keyed first-offender
rules of §2.4 deliberately accept a *wider*, file-level window instead — see
§4.)

`Baseline::load` **sums** the counts of any duplicate fingerprints it reads, so a
file with two entries for one fingerprint (e.g. a git merge of two branches that
each ran `alint baseline` for the same finding, or a hand-edit) suppresses the
*total*, not the last writer's count. A freshly written file never has duplicates
(`from_fingerprints` collapses them), so loading and re-writing is idempotent.

### 3.3 Enforcement pass (`check --baseline`)

A deterministic **post-evaluation report transform**, between `Engine::run` and
the formatters, alongside the notes-partition step:

1. Run the full evaluation → `Report` (unchanged).
2. Sort each `RuleResult`'s violations by `(path, line, column, message)` so the
   match order is deterministic regardless of the engine's parallel
   (`par_iter`) collection order (review H8).
3. Build the current fingerprint multiset.
4. For each fingerprint, **mark** the first `min(k, baseline_count)` occurrences
   (in the sorted order) as suppressed; the rest stay live. Suppressed violations
   are partitioned into a parallel `RuleResult.suppressed: Vec<Violation>` (like
   `notes`) — **marked, not removed** (§3.8). Record the per-fingerprint unfilled
   remainder for stale reporting (§3.6).
5. Exit code is computed on the **live** (non-suppressed) violations only: a repo
   whose only findings are baselined exits 0.

Determinism: suppression is a pure function of `(sorted Report, baseline)`.

### 3.8 Per-format suppression behaviour (mark, not remove)

Suppression *marks* violations rather than deleting them, so each formatter can
do the right thing for its consumer (review C2):

- **sarif** — emit suppressed results **with `result.suppressions: [{ "kind":
  "external" }]`** (and `baselineState: "unchanged"`; live results get
  `baselineState: "new"`). Removing them instead would make GitHub Code Scanning
  mark the alert *fixed* and then *reopen* it when the finding resurfaces — alert
  flapping in the exact blocking-gate scenario this feature targets. Marking keeps
  the alert open-but-dismissed. Every result also carries `partialFingerprints`
  (the baseline fingerprint) so Code Scanning's own correlation aligns with
  alint's.
- **json** — omit suppressed findings from `results` (so the gate sees only new),
  but the envelope carries a `summary.baselined_suppressed` count, and a
  fingerprinted `baselined` list under `--show-baselined`.
- **human** — omit from the primary output; print the suppressed **count** on
  stderr (the full list under `--show-baselined`).
- **github / gitlab / junit / markdown / agent** — emit the live (new) findings
  only, with **no suppressed-count signal in the artifact**. This is deliberate:
  these formats gate or annotate on new findings, and a synthetic "N suppressed"
  record has no natural representation in their schemas. A consumer that needs the
  suppressed count should select `json` or `sarif`.

Only **sarif** and **json** are baseline-aware (they consume the per-result marks
the CLI threads in via `BaselineMarks`); the rest receive the already-filtered
live report and are oblivious to the baseline. The exit code is gated on the live
findings only, in every format.

### 3.4 `fix` and the baseline

`fix` does **not** take `--baseline` in v1. Fixing is always safe — fixing a
baselined (accepted) violation is debt paydown — so `fix` applies every available
fixer and reports every remaining unfixable, baselined or not. After a fix run
that resolves baselined violations, re-run `alint baseline` (it prunes the now
stale entries without `--accept-new`; §2.2). A `fix --only-baselined` "pay down
debt" mode is deferred (§7-Q4). (v1 proposed `fix --baseline` for output parity;
the review showed it was near-cosmetic and risked hiding a fixer-introduced new
unfixable — dropped.)

### 3.5 Relationship to `--changed` / `--base`

Orthogonal filters that compose: `--changed` is a **scope** filter (which files
the per-file rules run on); `--baseline` is a **suppression** filter (which
already-known violations, anywhere — including from the full-tree rules
`--changed` can't scope — are accepted). `check --changed --baseline` lints the
diff *and* suppresses any pre-existing baselined issue that still surfaces. This
is strictly richer than golangci-lint's diff-only `--new-from-rev`.

### 3.6 Stale entries

After enforcement, a fingerprint whose recorded `count` exceeded the suppressed
count matched fewer than recorded — the issue was (partly) fixed or its
discriminator changed. These are **stale**, surfaced as a warning (`N baseline
entr(ies) no longer fire; run \`alint baseline\` to re-tighten`). Stale entries
do **not** fail the build by default (punishing fixes is wrong);
`--strict-baseline` makes them fail (exit 1; §3.9) for teams that want recorded
debt to stay exactly accurate (resolved §7-Q3).

### 3.9 Errors and exit codes

The existing codes are **0** clean · **1** violations (or warnings with
`--fail-on-warning`) · **2** config/IO error · **3** internal. Baseline maps onto
them, fail-closed:

| Condition | Exit |
|---|---|
| Only new (non-baselined) errors | 1 (as today) |
| `--strict-baseline` and stale entries exist (even if 0 new) | 1 (message distinguishes it from a normal violation failure) |
| `--baseline <path>` (or `baseline:` key path) does not exist | **2** — hard error, never "treat as empty" (a typo or a forgotten `alint baseline` must not silently run the gate without suppression) |
| Baseline file malformed, or `schema_version` unknown/newer | **2** — refuse, don't guess |
| Empty baseline (`violations: []` / header only) | valid; suppresses nothing, "0 suppressed" |
| `alint baseline` write/IO failure | 2 |
| `alint baseline` would add new entries without `--accept-new` | 2 (with the `+N/-M` summary) |

## 4. False-positive surface

The risk that matters most: **a baseline masking a genuinely new violation.**
By failure mode:

- **Discriminator too loose → new violation suppressed.** The v1 hole (a
  different new finding sharing `rule_id + path` for path-only rules like
  `structured_path`) is closed by the per-violation `baseline_key` (§2.4): the
  query/structural identity, not just the path, is in the fingerprint. The §6
  audit guarantees every kind that needs a key has one; the message fallback
  (§3.1 branch 3) is never load-bearing. Residual: byte-identical content at or
  below count (§3.2) — the narrowest possible window.
- **Discriminator too tight → churn.** Line *number* and the trailing terminator
  are excluded, so inserts, reflows, and CRLF↔LF conversion don't churn; only
  editing the offending line's own content (or a rule's structural key) re-fires.
  A sweeping reformat that rewrites offending lines re-fires them — acceptable and
  arguably correct (re-run `alint baseline` to re-accept).
- **First-offender rules accept a *file-level* window.** The path-keyed
  first-offender / first-match rules (`no_trailing_whitespace`, `line_endings`,
  `line_max_width`, `file_content_forbidden`; §2.4) emit at most one finding per
  file and are keyed on the path, so once a file is baselined for one of them
  the *rule is suppressed for that file* — a later offense on a **different**
  line is masked, not just a byte-identical recurrence. This is wider than the
  content-keyed window of §3.2, and is the deliberate price of not churning
  every time the first offending line is edited (the alternative, content
  keying, re-fires on any edit to that line even when it stays an offense). It
  is bounded: the window is one rule × one file, the baseline diff names the
  file, `--show-baselined` lists it, and re-running `alint baseline` after the
  file is cleaned drops the entry. Choose the rule's `level`/scope accordingly
  if a per-line guarantee matters more than churn-freedom.
- **Threshold rules and the "ratchet" gap.** A baselined `file_max_lines` keyed on
  path stays suppressed as the file grows further (the magnitude isn't in the
  key). This is correct *baseline* semantics — you accepted "this file is over the
  limit," and it remains over the limit. Preventing *worsening* (a no-regression
  ratchet) is a deliberately separate feature, out of scope here (§7-Q8).
- **File rename → re-fire.** `path` is in the fingerprint, so moving a file
  re-surfaces its baselined violations at the new path; for cross-file keys, a
  rename of *any* involved file changes the structural key and re-fires the whole
  cluster. Defensible (the inputs changed) but churny on large refactors;
  rename-following is out of scope for v1 (§7-Q5).
- **Level escalation is not re-surfaced.** The fingerprint excludes `level`, so a
  rule baselined at `warning` and later escalated to `error` stays suppressed for
  its legacy occurrences (only *new* ones fail as errors). This is intended
  (baseline accepts the finding), but it means an escalation is a no-op on
  grandfathered code — documented so it isn't a "did my config take effect?"
  surprise. A level-aware re-surfacing mode is a possible future (§7-Q8).
- **Masking a security violation.** The baseline is committed and PR-reviewed; the
  advisory `message` per entry (§2.1) makes the diff actually reviewable, and
  `--show-baselined` + the stale warning keep it honest. It carries no
  code-execution surface (§5) and cannot be introduced via `extends:`/nested
  configs (§5), so a published or subtree ruleset can never ship a suppression.

## 5. Implementation notes

- **Module:** `crates/alint-core/src/baseline.rs` — `Baseline` (JSON-Lines
  load/save, `schema_version` gate), `violation_fingerprint(&Violation, content:
  Option<&[u8]>) -> [u8; 32]`, and `apply(&mut Report, &Baseline, strict) ->
  Suppressed` (marks `RuleResult.suppressed`, returns the summary + stale list).
  CLI (`crates/alint/src/main.rs`) gains `cmd_baseline`, threads `--baseline` /
  `baseline:` key / `--strict-baseline` / `--show-baselined` into `cmd_check`,
  at the same post-load layer as the existing `apply_only_filter`.
- **Unify the fingerprint (review H1). — RESOLVED (v0.14 deferral close-out,
  2026-07-05).** `gitlab.rs` used to ship its own
  `fingerprint(rule_id|path|message|occurrence)` (SHA-256, bare `|` separator)
  for GitLab Code Quality cross-run dedup, which was line-unstable (code motion
  churned it) and differed from the SARIF/baseline fingerprint for the same
  finding. The `alint` GitLab path now computes the canonical
  `violation_fingerprint` (via the same `FileReader` the baseline path uses, so
  it has the file bytes for the line-content discriminator, §3.1 case 2) and
  passes a `[result][violation]` fingerprint grid into `write_gitlab`, so a
  finding carries **one** identity across GitLab, SARIF (`partialFingerprints`),
  and the baseline file. **Caveat (per-report uniqueness):** the canonical
  fingerprint deliberately *collides* on identical content (that is the baseline
  count-collapse), but GitLab Code Quality silently drops findings that share a
  fingerprint. So `write_gitlab` runs a per-report occurrence pass over the grid:
  the FIRST finding with a given canonical identity emits it raw (SARIF/baseline
  parity preserved), and any in-report collision gets a deterministic suffix, so
  every emitted GitLab issue is unique. `gitlab.rs` keeps the old scheme only as
  a `self_fingerprint` *fallback base* for callers without file access (the
  generic `Format` dispatch, benches, unit tests), which the same occurrence pass
  makes unique. Cross-surface parity for the un-collided case is guarded by
  `alint::tests::baseline_cli::gitlab_fingerprint_equals_canonical_baseline_fingerprint`;
  the collision-disambiguation by
  `gitlab::tests::precomputed_canonical_collision_is_disambiguated_but_first_keeps_identity`.
- **`Violation.baseline_key`** (§2.4): with the v3 path-shape default, the
  rule-side work is setting a key on only the ~15 kinds whose identity isn't
  `(rule_id, path)` — structured-query, cross-file/no-path, first-offender, and
  line-collision. Threshold, single-finding path-only, and line-content kinds are
  untouched (the default covers them).
- **Content access (review H6).** `FileEntry` caches no content (`walker.rs`); the
  branch-2 line discriminator requires reading the offending file. Mitigations:
  (a) only files that produced a *line-anchored, key-less* violation are read; (b)
  a small per-file content cache within the fingerprint pass avoids re-reads when
  a file has several such violations; (c) rules that set `baseline_key` (incl. the
  bounded-read `no_bom`, which reads only 4 bytes) need **no** content read.
  Best-effort against TOCTOU: read once during the pass. The v1 "no second read"
  claim was wrong and is removed.
- **Config + trust (review H2/H3).** `baseline: <path>` is parsed on the
  top-level config only. Mirror `allow_out_of_root`: a non-default `baseline:`
  reaching the loader from an `extends:`'d or nested config is **rejected** (it is
  `#[serde(skip)]` on the resolved `Config`, and `nested.rs`'s forbidden-key list
  gains `baseline`). The accurate security statement (replacing v1's false
  "path-confined within root"): *the baseline path is a trusted top-level input,
  like `-c` — honoured only from the user's own config or CLI, never from
  `extends:` or a nested subtree config.* Per-subtree baselines in `nested_configs`
  are a real monorepo want but out of scope for v1 (§7-Q9).
- **Dependencies:** none new (`sha2`, `serde`/`serde_json` already in-tree).
- **Constitution** (`constitution.md`): *Correctness* — deterministic,
  order-preserving suppression; deterministic, sorted, `schema_version`-gated
  file. *Security* — no new trust surface; not reachable via `extends:`/nested.

## 6. Tests

Unit (`baseline.rs`): fingerprint stability (same tree → identical hash + file);
line-number independence (insert above → same hash); trailing-`\r?\n` stripping
(CRLF↔LF → same hash); edited offending line → different hash; `baseline_key`
takes precedence over content; identical-content count collapse; length-prefix
domain separation (no `a‖bc` vs `ab‖c` collision; `baseline_key` path-sets with
spaces don't collide); `is_note` never fingerprinted; full-hash (no truncation);
JSON-Lines round-trip + `schema_version` gate + malformed/empty handling;
deterministic sort/serialization.

**Coverage-audit test — the structural collision-invariant (anti-drift floor):**
for every registered rule kind, run it against the firing scenario corpus (and
targeted multi-finding fixtures), fingerprint every emitted violation, and assert
two things hold per kind: **(i)** no two *distinct* live violations share a
fingerprint (a collision is a silent mis-suppression — the structured-query /
line-collision failure mode), and **(ii)** no violation falls through to the
message anti-panic branch (§3.1 branch 4) — every no-path/no-line violation must
carry a `baseline_key`. Because the invariant is checked on the *actual emitted
violations* rather than asserting a hand-maintained per-kind classification, it
can't drift out of sync with the rules; the only maintenance is ensuring each
multi-emitting kind has a fixture that exercises its multi-finding shape (a
`FIRST_OFFENDER`/cross-edit allowlist, like the existing `NATIVE_FIRES_ALLOWLIST`,
pins the kinds whose churn is only visible across edits). This is the gate that
would have caught the v1 `structured_path` hole and the v2 `markdown_paths_resolve`
collision.

e2e (`crates/alint-e2e`), firing + silent pairs (constitution 8): suppressed
(baselined-only → 0 reported, exit 0); new detected (added → exit 1, baselined
stay suppressed); **structured_path** (baseline `$.a`; add a different `$.b`
finding on the same file → reported, not masked — the v1-regression guard);
edited line re-fires; count (3 identical baselined, add a 4th → only the 4th
new); first-offender rule (fix the first trailing-whitespace line → the file's
key keeps it suppressed only while *any* offender remains, surfaces clean when
fully fixed); `--changed` compose; `baseline --changed` → rejected;
regeneration without `--accept-new` when new debt exists → exit 2 + summary;
SARIF marks (suppressed result carries `suppressions`/`baselineState`, not
removed); stale warn vs `--strict-baseline` exit 1; opt-in (config key applies;
no key/flag → never suppresses); missing/`schema_version`-mismatch baseline →
exit 2; exit-code matrix.

No bench-compare threshold (suppression is O(violations)); confirm the per-file
content reads for key-less line violations don't regress the hot path (they touch
only files that already produced such a violation, cached per file).

## 7. Open questions

1. **Discovery / opt-in — RESOLVED (2026-06-21): config key + flag, no
   auto-detect.** See §2.3.
2. **Discriminator — RESOLVED (2026-06-21, revised in v2): a general per-violation
   `baseline_key`** (not no-path-only), preferred over the default line-content
   discriminator, with a `\r?\n`-stripped line-content default and a never-load-
   bearing message fallback, gated by the §6 coverage audit. The v1 answer
   (no-path-only structural key) was insufficient — `structured_path`,
   `file_max_lines`, and the first-offender rules need keys too. See §2.4, §3.1.
3. **Stale handling — RESOLVED (2026-06-21): warn-only default + opt-in
   `--strict-baseline`.** See §3.6, §3.9.
4. **`fix --only-baselined`** ("pay down debt" mode) — deferred; `fix --baseline`
   dropped from v1 entirely (§3.4).
5. **Rename following** — out of scope v1 (path-/key-keyed; churns on rename).
   Note as a known churn source; revisit if refactors prove painful.
6. **Hash truncation — RESOLVED (2026-06-21): no truncation,** full 64-hex
   SHA-256. A fingerprint collision is a silent mis-suppression; the readability
   that motivated truncation is provided by the advisory `message` field (§2.1).
7. **Fingerprint unification with `gitlab.rs` (and SARIF)** — RESOLVED (v0.14
   deferral close-out, 2026-07-05; SARIF made always-on afterward). The single
   `violation_fingerprint` lives in alint-core; the baseline file, the `alint`
   GitLab export, and SARIF `partialFingerprints` all carry it, so a finding has
   one identity everywhere. SARIF originally emitted `partialFingerprints` only
   under an active baseline; it now emits them on every run (a plain `check
   --format sarif` needs no `--baseline` for GitHub Code Scanning to correlate
   alerts across runs), from the same `[result][violation]` grid the GitLab path
   uses. `gitlab.rs` retains the legacy message-keyed hash only as a
   `self_fingerprint` fallback for callers without file access (generic dispatch
   / benches / tests). Parity is guarded by
   `gitlab_fingerprint_equals_canonical_baseline_fingerprint` and
   `sarif_fingerprint_equals_canonical_baseline_fingerprint` (§5).
8. **No-regression "ratchet" + level-aware re-surfacing** — out of scope v1, but
   the two most-requested likely follow-ups (prevent a threshold getting *worse*;
   re-surface a grandfathered finding when its rule is escalated to `error`).
9. **Per-subtree baselines under `nested_configs`** — a real monorepo want;
   forbidden in v1 (§5). Revisit as a dedicated follow-up.
