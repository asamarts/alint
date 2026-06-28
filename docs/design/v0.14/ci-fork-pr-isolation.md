# CI fork-PR isolation — keep untrusted PR code off the self-hosted runner

Status: **Proposed (2026-06-28).** Follows the post-v0.13 audit finding
**H6** (`post_v0.13_audit.md`). The immediate mitigation — the GitHub
*"Require approval for all outside collaborators"* setting — is already in
place; this is the durable, defence-in-depth fix. No workflow change has
landed yet; this doc is the spec to review before one does.

Scope: `.github/workflows/ci.yml`, `.github/workflows/coverage.yml`.
Related: ADR-0004 (trust boundary), `deterministic-perf-gating.md`,
`post_v0.13_audit.md` §H6.

---

## 1. Problem

`asamarts/alint` is a **public** repo. `ci.yml` and `coverage.yml` trigger
on `pull_request` and run almost every job on
`runs-on: [self-hosted, linux, alint]` — which is the maintainer's
**persistent** box (it also hosts the bench baseline and other services,
and on disk holds the `asamarts` + `kaminsod` `gh` tokens, SSH/deploy keys,
and cargo credentials). A pull request from a **fork** checks out the PR's
code and runs it — `ci/scripts/*.sh`, `xtask`, `build.rs` — so a malicious
fork PR can read those secrets, persist, or poison the cache/bench
baseline. This is GitHub's documented "don't use self-hosted runners with
public repos" hazard.

The approval setting stops fork PRs from running *without a maintainer
click*. It does **not** make the box safe once approved (approving still
runs the code there), and "remember to never click approve on anything
fishy" is not a control. The durable fix is: **untrusted fork-PR code must
never execute on the self-hosted runner — only on throwaway,
GitHub-hosted runners.**

## 2. Goals / non-goals

**Goals**
- A fork PR's jobs run only on ephemeral `ubuntu-latest`, never on the box.
- A fork PR still gets the full correctness gate it can get without the
  box (fmt, clippy, test, build, audit, deny, docs, dogfood, examples,
  shell-tests) so contributors get real feedback.
- **Trusted** events keep using the fast, warm self-hosted box: `push` to
  `main`, tags, `workflow_dispatch`, `schedule`, and same-repo
  (collaborator-branch) PRs.
- For a public repo, GitHub-hosted minutes are free, so this costs $0.

**Non-goals**
- Replacing the self-hosted runner (bench, deterministic perf-gate, and
  tuned coverage still need it for trusted events).
- Giving fork PRs the box-only gates (bench, Valgrind perf-gate, the tuned
  coverage run). They are deliberately skipped on fork PRs — see §6.
- Changing the GitHub approval setting (it stays on as the first layer).

## 3. Threat model & the load-bearing GitHub fact

A PR is **untrusted** iff it comes from a fork:
`github.event_name == 'pull_request' && github.event.pull_request.head.repo.fork == true`.
That value is computed by GitHub from the PR's source repo; a fork cannot
forge it.

**The fact this design rests on:** for a `pull_request` event, GitHub
evaluates the workflow **definition from the base repository** (the PR's
target branch), while checking out the **PR's code** for the run. So:
- the **YAML** (jobs, `runs-on`, `if:`) is the base repo's and a fork
  cannot rewrite it for its own run — the routing guards below are
  authoritative;
- the **scripts** the YAML invokes (`ci/scripts/*.sh`, `xtask`, …) are the
  **fork's** version.

The hard consequence: **the fork-vs-trusted decision must be taken in the
base YAML from the immutable `head.repo.fork` context — never derived from
a checked-out script's output.** A fork's `detect-changes.sh` runs with the
PR's code; if routing read *its* output, a fork could emit
`runner=[self-hosted,…]` and land on the box. Routing reads only the
GitHub context.

(`pull_request` from a fork also gets a **read-only** `GITHUB_TOKEN` and
**no repository secrets** — e.g. `CODECOV_TOKEN` is empty on a fork run.
Those are GitHub-enforced and independent of this change.)

## 4. Design (recommended): route once from context, reuse per job

Add a cheap **router** to the existing `changes` job and move it to
`ubuntu-latest` (it is the entry point — it must never touch the box, and
`detect-changes.sh` is just a `git diff`, fine on a hosted runner). It
emits two new outputs, computed from the immutable context in their **own
step** (so the fork's `detect-changes.sh`, which runs in a different step,
cannot influence them):

```yaml
  changes:
    name: Detect Changes
    runs-on: ubuntu-latest            # was [self-hosted, linux, alint]
    outputs:
      rust: ${{ steps.detect.outputs.rust }}
      # …existing change-detection outputs…
      runner: ${{ steps.route.outputs.runner }}
      untrusted: ${{ steps.route.outputs.untrusted }}
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - id: route                      # BEFORE detect; pure context, no PR code
        env:
          IS_FORK: ${{ github.event_name == 'pull_request'
                       && github.event.pull_request.head.repo.fork }}
        run: |
          if [ "$IS_FORK" = "true" ]; then
            echo 'runner=["ubuntu-latest"]'              >> "$GITHUB_OUTPUT"
            echo 'untrusted=true'                        >> "$GITHUB_OUTPUT"
          else
            echo 'runner=["self-hosted","linux","alint"]' >> "$GITHUB_OUTPUT"
            echo 'untrusted=false'                       >> "$GITHUB_OUTPUT"
          fi
      - id: detect
        env: { GH_EVENT: ${{ github.event_name }}, … }
        run: ci/scripts/detect-changes.sh
```

Every downstream **portable** job (fmt, clippy, test, audit, deny, build,
docs, dogfood, examples, shell-tests, summary) changes one line:

```yaml
    runs-on: ${{ fromJSON(needs.changes.outputs.runner) }}
```

→ trusted events resolve to `[self-hosted, linux, alint]` (fast, warm box);
fork PRs resolve to `[ubuntu-latest]` (throwaway VM, no box secrets).

The **box-only** jobs additionally skip on fork PRs (see §6):

```yaml
    if: >-
      … existing conditions …
      && needs.changes.outputs.untrusted != 'true'
```

`editors` is already `ubuntu-latest` — unchanged.

### Per-job disposition

| Job | Needs the box? | Disposition |
|---|---|---|
| `changes` (router) | no | **always `ubuntu-latest`** (entry point) |
| `fmt`, `clippy`, `test`, `build`, `dogfood`, `examples`, `shell-tests`, `summary` | no | dynamic `runs-on` (box when trusted, hosted on fork PR) |
| `audit` | tool only | dynamic + ensure `cargo-audit` is installed on the hosted path (§5) |
| `deny` | tool only | dynamic (`deny.sh` already self-installs `cargo-deny`) |
| `docs` | Node 22 + likec4 | dynamic + add `setup-node` + likec4 install on the hosted path (§5) |
| `bench-smoke` | **yes** (bench) | **skip on fork PRs** |
| `perf-gate` | **yes** (Valgrind) | **skip on fork PRs** (already PR-only + rust) |
| `editors` | no | unchanged (`ubuntu-latest`) |
| `coverage` (coverage.yml) | **yes** (tuned pids/jobs + secret) | **skip on fork PRs** (§6) |

## 5. Ephemeral-path gaps to close

The box has tools pre-installed that a fresh `ubuntu-latest` does not. For
the jobs that run there on a fork PR:

- **`docs`** runs `docs.sh` → `likec4.sh` (`likec4 validate`,
  `gen-mermaid --check`), needing **Node 22 + `@likec4/cli`** and the
  `--no-use-dot` wasm layouter. Add `actions/setup-node@v6` (node 22) +
  a pinned `npm i -g @likec4/cli@<ver>` step, guarded to the hosted path
  (or unconditionally — it's cheap). Without it, the LikeC4/mermaid
  sub-gates would error on a fork PR. The `gen-{schema,facts,arch}` and
  `docs-export` checks are pure cargo and need nothing extra.
- **`audit`** assumes `cargo-audit` is present (`audit.sh` has no install
  step, unlike `deny.sh`). Add a `command -v cargo-audit || cargo install
  cargo-audit --locked` guard to the script, or an install step on the
  hosted path.
- **Caches**: GitHub isolates fork caches — a fork PR gets **read-only**
  access to the base cache and cannot write it (so it can't poison
  `Swatinem/rust-cache`, a bonus), but its builds are colder/slower.
  Acceptable for a correctness gate.
- **`RUSTFLAGS: -D warnings`** etc. are env-level and already portable.

These are the only deltas; the cargo/shell scripts themselves are
platform-portable.

## 6. Why box-only jobs skip fork PRs (and where the gate still runs)

- **`bench-smoke` / `perf-gate`** are meaningless or impossible on a shared
  hosted VM (wall-clock is noisy; the deterministic gate needs Valgrind +
  the merge-base build on the box). They are maintainer-facing perf
  signals, not contributor-blocking correctness — skipping them on fork
  PRs loses nothing a fork could act on. They still run on every
  collaborator PR and on push.
- **`coverage`** needs the box's `pids-limit`/`CARGO_BUILD_JOBS` tuning to
  avoid the instrumented-build OOM/hang (documented in the runner-recovery
  notes), and its Codecov upload secret is withheld from forks anyway. Skip
  it on fork PRs; the `ALINT_COVERAGE_FLOOR` gate still runs on push-to-main
  and on every collaborator PR, which is where regressions must not land.
  (Future option: a reduced-parallelism hosted coverage run for fork PRs —
  see §8.)

Net for a fork PR: full fmt/clippy/test/build/audit/deny/docs/dogfood/
examples/shell-tests on `ubuntu-latest`; bench/perf/coverage skipped. A
contributor still sees a real green/red.

## 7. Rollout & testing

1. Land the workflow change behind the router; keep the box-only jobs'
   existing `if:` plus the `untrusted != 'true'` clause.
2. **Verify with a real fork:** open a PR from a throwaway fork and confirm
   (a) every job lands on `ubuntu-latest`, (b) `bench-smoke`/`perf-gate`/
   `coverage` are skipped, (c) the `docs` job's likec4 gate runs (Node
   installed). Confirm the box's runner shows **no** jobs for that PR.
3. **Verify trusted paths unchanged:** a same-repo branch PR and a push to
   `main` both still run on `[self-hosted, linux, alint]`, full matrix.
4. Keep the GitHub approval setting on throughout — belt **and** suspenders.
5. Update `post_v0.13_audit.md` H6 → done, and add a one-line note to
   `CONTRIBUTING.md` / `RELEASING.md` that fork PRs run on hosted runners.

## 8. Open questions

1. **Coverage on fork PRs** — skip (recommended, simplest) vs a
   reduced-parallelism hosted run (`CARGO_BUILD_JOBS=2`, no Codecov
   upload). Skipping means a fork PR shows no coverage delta; the floor is
   still enforced where it matters. Decide per how much fork-PR coverage
   feedback is worth.
2. **Full-ephemeral vs dynamic** — should *trusted* CI also move to
   `ubuntu-latest` (retire the box for PR/push, keep it only for
   bench/coverage/release)? Simpler YAML, slower maintainer CI, but removes
   the box from the correctness path entirely. The dynamic design keeps the
   fast path; full-ephemeral maximises simplicity. (Recommendation:
   dynamic — the box is fast and warm and the maintainer's own pushes
   benefit.)
3. **`audit`/`deny` on hosted** — fold the tool-install guard into the
   scripts (portable everywhere) vs a workflow step (hosted-only). Folding
   into the scripts is cleaner and helps local runs too.

## 9. Alternatives considered

- **GitHub setting only (status quo + approval).** The first layer, kept.
  Insufficient alone: approving a fork PR still runs its code on the box.
- **Skip all self-hosted jobs on fork PRs, add one combined hosted job.**
  Simpler YAML, but fork PRs get a coarse single check instead of the real
  per-job graph, and it diverges from the trusted path (drift risk). The
  dynamic `runs-on` gives forks the *same* job graph on hosted runners.
- **Per-job inline fork expression** (no router job). Secure but repeats a
  long `${{ … fromJSON … }}` ternary on every job; the single router output
  is DRYer and equally safe (it reads only the immutable context).
- **`pull_request_target`.** Rejected outright — it runs with secrets and
  the base token against PR code; the opposite of what we want. This design
  must never introduce it.
