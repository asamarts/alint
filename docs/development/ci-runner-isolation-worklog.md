# Alint CI runner isolation work log

This append-only internal engineering log records the Minemon-driven isolation
and routing work for alint ordinary CI. Times are America/Toronto. It does not
contain credentials, runner registration material, host serials, wallet data or
raw telemetry. Minemon task status remains in the Minemon aplan; this file
records alint-owned workflow/design/test changes and their immutable commits.

## 2026-09-22

### 01:35 EDT — isolated worktree and baseline

- Created branch `ci/minemon-isolated-runner` in a dedicated clean worktree
  from remote `main` commit
  `a623a08dcdf7b3e21b3145e3d12b41579a5bc0c5`. The primary checkout remains on
  its pre-existing `phase-0-fix-engine` branch and was not modified.
- Read `CONTRIBUTING.md`, `docs/development/README.md` and
  `docs/design/v0.14/ci-fork-pr-isolation.md` before making a repository change.
- The current default-branch workflow already routes an external fork's
  ordinary CI to `ubuntu-latest` and skips its coverage job before self-hosted
  assignment. The new scope is exact repository-ID/push-authority admission,
  disposable local execution for admitted ordinary work, and explicit
  separation of coverage compute from Codecov authority.
- No workflow, GitHub setting, runner, service, package, firmware or host
  configuration changed in this entry.

### 01:36–01:43 EDT — same-repository bot bypass found

- GitHub identifies `asamarts` as user ID `11239806`, `kaminsod` as user ID
  `12991611`, and this repository as ID `1214597864`. The active GitHub CLI
  identity has public pull access only, while the configured SSH transport
  authenticates as `asamarts`; no branch was pushed.
- Dependabot PR 250 has actor ID `49699333` and both its base and head repository
  are this repository. The current `full_name` equality therefore treated it as
  trusted. Coverage run `35592601818` and the executable jobs in ordinary run
  `35592601728` ran on `alint-runner`; the latter retained a queued Summary job
  at discovery.
- This disproves the design assumption that same-repository implies approved
  human authority. The corrective change must require stable numeric repository
  and human author/actor IDs. Dependabot, another bot/App, an unapproved branch
  updater, null identity or API failure must route ordinary CI hosted and skip
  local-only coverage.
- The local listener existed but had no worker. Its last selected logs showed
  network-unreachable reconnect failures, the nightly mutants run had failed,
  and a Summary job remained queued. It was not stopped or restarted because
  doing so could execute the unapproved queued bot job before routing is fixed.
- Minemon task MN-168 was created for this narrow workflow containment. The
  disposable-runner implementation remains separately gated.

### 01:46 EDT — pending temporary hold decision

- Confirmed again that no worker was active and bot-owned Summary job
  `106345164594` was still queued. Requested operator approval to stop only the
  ordinary `alint-runner` until the routing fix reaches the default branch.
  No container or GitHub action has yet been changed.

### 11:13–11:16 EDT — approved temporary hold

- The final read-back found no worker. GitHub reported the formerly queued
  Summary job `106345164594` as completed/cancelled at 09:07:17 EDT without a
  runner assignment. That cancellation preceded this operation and was not
  caused or claimed by it.
- With operator approval, stopped only
  `container-alint-runner.service`/`alint-runner`. The unit became
  `inactive/dead` and remains enabled. Because the generated unit has no
  `ExecStop`, an exact `podman stop --time 30 alint-runner` was required; the
  container is exited with code 143 and PID zero.
- A sorted before/after inventory showed no change to any other running
  container. No runner registration, image, volume, workflow, repository
  setting or automatic-start configuration changed.
- The legacy fixed-label listener stays stopped. A `pull_request` workflow is
  selected in the PR merge context, so a PR can edit an in-workflow predicate
  and request a persistent matching label. The exact-ID patch fixes canonical
  routing but cannot by itself be the scheduling security boundary. Safe local
  capacity therefore waits for the separate base-controlled, unique-label,
  one-job disposable-runner cutover.

### 11:17–11:52 EDT — canonical route correction and validation

- Updated `ci.yml` so routing occurs before checkout and a PR receives the
  local selector only when event, base and head repository IDs are exactly
  `1214597864` and PR author, webhook sender and `github.actor_id` are each in
  the numeric human-ID set `11239806`/`12991611`. Updated `coverage.yml` with
  the identical pre-assignment predicate. Non-PR behavior is deliberately
  unchanged in this containment patch.
- Added `ci/scripts/test-ci-pr-routing.sh`. Its positive and negative fixtures
  cover both admitted humans, login-rename independence, Dependabot ID
  `49699333`, external/wrong repositories, an unapproved author, unapproved
  synchronizer and null head repository. Static checks compare the complete
  normalized predicate across both workflows, require route-before-checkout,
  reject the former name/fork tests and reject direct ordinary-CI runner
  selectors outside the hosted/base-route outputs.
- Replaced the inaccurate immutable-base-YAML claim in
  `docs/design/v0.14/ci-fork-pr-isolation.md`. The corrected design distinguishes
  canonical routing from scheduling authority and specifies the held legacy
  state, fail-closed behavior, broker boundary, verification and rollback.
- Passed: the routing harness; all six shell harnesses; Bash syntax;
  `shellcheck`; PyYAML syntax parsing for both changed workflows; `actionlint`
  v1.7.12; `git diff --check`; and the complete workspace test suite under
  `TERM=xterm-256color` (all test binaries and doctests passed).
- The first full `ci/scripts/preflight.sh` run used the tool session's
  `TERM=dumb`. Nineteen CLI goldens emitted their intentional ASCII fallback
  and differed from Unicode snapshots; the exact CLI suite passed after
  setting `TERM=xterm-256color`. Every docs/export/schema/facts/architecture,
  LikeC4, Node, version-floor and secret-inventory stage passed.
- Dogfood reported the pre-existing `rust-file-max-lines` warnings for
  `crates/alint-core/src/engine.rs` (2,016 lines) and
  `xtask/src/docs_export.rs` (2,384 lines). Both files are byte-identical to
  `origin/main`; this workflow-only change neither caused nor masks that
  baseline debt. No snapshot or dogfood rule was weakened.
- Final read-back kept `container-alint-runner.service` inactive/dead and
  enabled, and `alint-runner` exited with PID zero. It was not started for
  validation; no other runner was changed.
- Committed the workflow, test and documentation change as
  `462ea132ebec0dbb73b1960a28914946129ef028` on
  `ci/minemon-isolated-runner`. Push and PR evidence follow separately so this
  log does not claim remote state before it exists.

### 11:53–11:56 EDT — remote and owner-authority read-back

- Pushed the branch and read it back at
  `dbc179ca0b1a13efa9ec01b9bda67fa8fc4913ae`; GitHub listed no workflow run for
  the branch push. This matches the workflow's main/master/tag-only push scope.
- Used the already configured `asamarts` CLI profile ephemerally, without
  changing the globally active `kaminsod` profile or printing credentials.
  GitHub confirmed admin authority, repository ID `1214597864`, and `main` as
  the default branch.
- The effective collaborator list contained only `asamarts` (admin); no deploy
  keys were returned. This does not prove the absence of installed GitHub Apps,
  which needs a separate installation-authority inventory.
- Repository Actions are enabled with all actions allowed. Default workflow
  token permission is `write`, workflow tokens may approve PR reviews, and the
  fork approval policy covers only first-time contributors. There are nine
  Actions secrets by count, zero Actions variables, zero environments, no
  rulesets and no `main` branch protection.
- GitHub reported `alint-runner` offline and idle, as required by the hold.
  The distinct `kbench-bench` runner remained online and idle; it was neither
  stopped nor reconfigured. No repository setting or runner registration was
  changed during this read-back.
- The write-default/all-actions/approval posture is a separate hardening
  finding. It must be mapped against every workflow's actual permissions and
  actions before a least-privilege change; this containment task does not
  silently alter it.

### 12:01 EDT — normal PR and safe positive-route observation

- Opened owner PR `asamarts/alint#251` from the pushed branch to `main`. Its
  body records the held-runner boundary, validation results and the two
  pre-existing preflight findings; it does not claim that queued local work is
  a passed canary.
- On head `2346a0e756a0806c145c00426fb22aa1663b2312`, hosted CI entry jobs Detect
  Changes and Secrets Inventory completed successfully. The approved-human
  predicate selected local labels for Format, Shell Tests and coverage, which
  remained queued while GitHub independently reported `alint-runner` offline
  and idle. This is the intended bounded positive-route observation before
  disposable capacity exists: policy selection is visible, but no PR code ran
  locally.
- Other PR workflows using GitHub-hosted runners began normally. The separate
  `kbench-bench` runner remained online/idle and was not selected or changed.
