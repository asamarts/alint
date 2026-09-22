# CI PR isolation — keep unapproved code off local runners

Status: **canonical routing corrected; all PRs hosted while local capacity is held.** The
original fork routing landed in #106 after audit finding H6. A September 2026
review found two gaps: same-repository bot PRs passed its repository-name test,
and the design incorrectly treated `pull_request` workflow YAML as immutable
base-branch policy. The current workflow uses exact numeric identity checks,
but the persistent `alint` listener must remain offline until a separately
qualified base-controlled broker provisions a unique disposable one-job
runner. Operational evidence is in
`docs/development/ci-runner-isolation-worklog.md`.

Scope: `.github/workflows/ci.yml`, `.github/workflows/coverage.yml` and
`ci/scripts/test-ci-pr-routing.sh`. Related: ADR-0004 (trust boundary),
`deterministic-perf-gating.md`, `post_v0.13_audit.md` §H6.

## 1. Problem

`asamarts/alint` is public. Its build, test and coverage jobs execute repository
code: shell scripts, Rust build scripts, tests and project tools. A persistent
self-hosted runner on a developer machine is therefore a host-compromise and
cross-job-persistence boundary, even if the runner process itself is rootless.
GitHub explicitly recommends against persistent self-hosted runners for public
repositories because pull requests can execute attacker-controlled code and a
self-hosted machine is not guaranteed to be clean or ephemeral.

The first fix classified only external forks as untrusted. That was
insufficient. Dependabot PR 250 had the same base and head repository ID as the
project, so the former `head.repo.full_name == github.repository` check routed
its ordinary and coverage jobs locally. More generally, a bot, installed App,
deploy key or other principal can update a same-repository branch without being
one of the two approved human development authorities.

There is a deeper boundary: a `pull_request` run uses the PR merge ref and merge
commit. GitHub selects workflow files from the event-associated ref/commit, so
a PR can alter the very YAML that contains `runs-on` and the routing check. An
allowlist inside that file can correct normal/canonical routing but cannot
protect an online persistent runner with predictable labels from a workflow
edit. First-time-contributor approval is also not a durable isolation boundary:
approval allows the proposed workflow/code to run and future approval policy
may change.

## 2. Policy and goals

For a PR, canonical local eligibility requires all of the following:

- event repository ID, base repository ID and head repository ID are exactly
  `1214597864` (`asamarts/alint` at the recorded review);
- PR author ID is one of `11239806` (`asamarts`) or `12991611` (`kaminsod`);
- webhook event sender ID and `github.actor_id` are in the same human-ID set;
  and
- every field exists and compares successfully. Missing/null/unknown values
  fail closed.

Numeric IDs are intentional: login and repository names can change. An
`author_association`, owner-name wildcard, `head.repo.fork` bit, branch name,
label or same-repository test is not authorization. The sender check rejects a
bot/App synchronizing an approved author's branch; the author check rejects an
unapproved author whose event is retriggered by an approved actor. GitHub rerun
identity has additional semantics, so the future broker also validates the live
run attempt and triggering actor through the API.

The desired end state is:

- approved development PRs may use local compute only in a fresh, secretless,
  one-job VM with an independently admitted job and unique non-default runner
  registration;
- all other PRs run portable jobs on GitHub-hosted ephemeral capacity, while
  box-only jobs and local coverage remain skipped/held;
- release, publication, Codecov upload and other write/secret effects stay
  separate from PR compute; and
- a missing identity, API failure, broker mismatch, label collision or absent
  qualified guest leaves the local job queued/held. It never falls back to the
  legacy listener.

This correction deliberately preserves existing non-PR route behavior in the
workflow while the replacement is designed. For PRs, the exact identity
predicate is retained but an explicit false capacity switch sends even admitted
human PRs to hosted portable CI until MN-167 qualifies the replacement. Push,
tag, schedule and manual events need their own exact principal/ref contracts
before local provisioning; “not a PR” is not sufficient admission for the
future broker.

## 3. Current containment

The `changes` job always starts on `ubuntu-latest`. Its first step, before any
checkout, evaluates only GitHub context values. It retains the exact admitted-
identity predicate, but `LOCAL_PR_CAPACITY_ENABLED` is explicitly false while
the legacy listener is held. Every PR therefore emits `ubuntu-latest` and
`hosted=true`; portable downstream jobs consume that one output. Box-only jobs
require `hosted != 'true'`. `coverage.yml` repeats the exact admission
predicate behind an explicit false capacity term in its job-level `if`, so
every PR skips before runner assignment.

`hosted` describes the selected executor rather than the contributor's trust.
That distinction is load-bearing: an approved human PR still needs Node setup
on a fresh hosted runner and still must not receive a box-specific benchmark.
The false switch is canonical routing only. Because PR YAML can change it, the
stopped listener remains the actual protection until the base-controlled
broker exists.

This ordering prevents the checked-out `detect-changes.sh` from changing the
canonical route. It does **not** make PR-controlled workflow YAML authoritative.
For that reason the persistent runner remains stopped, even after this patch.
With no fixed-label listener online, a PR that edits the workflow to request
`[self-hosted, linux, alint]` can at worst leave a job queued; it cannot acquire
this host through that label.

### Per-job canonical disposition

| Job | Approved PR | Other PR |
|---|---|---|
| `changes` (routing/change detection) | GitHub-hosted | GitHub-hosted |
| `fmt`, `clippy`, `test`, `audit`, `deny`, `supply-chain`, `build`, `docs`, `dogfood`, `examples`, `shell-tests`, `summary` | GitHub-hosted while capacity is held | GitHub-hosted |
| `bench-smoke`, `perf-gate` | skipped by `hosted` guard while capacity is held | skipped by `hosted` guard |
| `editors` | GitHub-hosted | GitHub-hosted |
| `coverage` | skipped before assignment while capacity is held | skipped before assignment |

The route step is intentionally inline. A checked-out repository script is PR
code and cannot be trusted to choose a runner. The mirrored coverage predicate
is protected against accidental drift by `test-ci-pr-routing.sh`.

## 4. Hosted-path compatibility

Portable jobs on a fresh `ubuntu-latest` host need the tools formerly assumed
from the warm box:

- `docs` installs Node 22 before the LikeC4 checks on every hosted route;
- `audit.sh` and `deny.sh` bootstrap their pinned/locked Cargo tools as already
  documented by their scripts; and
- hosted caches are performance inputs, never authorization. No artifact or
  cache produced by an unapproved route may be executed by a later privileged
  or local job without independent provenance validation.

Coverage remains held for unapproved PRs because the existing instrumented
build depends on local tuning and the combined job also has a Codecov upload
effect. The target design separates secretless coverage computation from an
independently authorized upload. Bench smoke and deterministic perf remain
box-specific signals and are skipped outside an approved route.

### Token authority is independent of runner placement

Hosted execution isolates the maintainer's machine; it does not make an
overpowered `GITHUB_TOKEN` safe. Every workflow therefore has an explicit
top-level permission boundary. Ordinary action, CI, coverage, cross-platform,
editor, issue-26 and mutants workflows receive only `contents: read`. Workflows
whose jobs have different effects use top-level `permissions: {}` and exact
job declarations:

- the benchmark-image job has `contents: read` plus `packages: write`;
- the benchmark guard has `contents: read`, while only the canonical benchmark
  job has `contents: write` and `pull-requests: write`;
- release preflight/build and external-credential publishers have
  `contents: read`; GitHub Release, GHCR, OIDC/attestation and follow-on
  dispatch jobs retain only their respective declared scopes; and
- the docs-bundle workflow retains `contents: write` because its one job pushes
  the dedicated branch. Its Cloudflare hook is a separate credential and does
  not expand `GITHUB_TOKEN`.

GitHub exposes one repository setting for both creating and approving pull
requests. It remains enabled because `bench-record` must run `gh pr create`;
this is a compatibility exception, not approval authority. Repository code is
forbidden from submitting an approving review. The
`test-workflow-permissions.sh` harness inventories every workflow/job, compares
the exact allowed maps, binds known write indicators to their required scope,
records the Homebrew deploy-key push as an explicit external-credential edge,
and rejects review-approval operations.

## 5. Verification contract

`ci/scripts/test-ci-pr-routing.sh` models and checks:

- approved same-repository events for each admitted human ID;
- stable-ID behavior independent of renamed logins;
- same-repository Dependabot, external fork, wrong base/event repository,
  unknown author, unknown synchronizing actor and null head repository denial;
- presence of every exact-ID term in both workflows;
- route-before-checkout ordering; and
- absence of the former `head.repo.full_name`/`head.repo.fork` trust tests.

Before merging, run the routing harness, all shell harnesses, a YAML parse,
workflow lint where an admitted `actionlint` is available, and the repository
preflight. The owner PR itself must show that an admitted human receives the
complete portable hosted graph while box-only jobs and coverage skip. After
merging, observe an actual bot/unapproved PR with the same executor result and
no local worker. The prior owner-PR run already recorded the admitted local
legs safely queued while the listener was offline; changing the canonical
route to hosted restores portable validation without restarting it.

The disposable cutover later requires live positive and negative canaries. Its
base-controlled broker must independently verify repository, workflow, event,
ref, immutable SHA, run attempt, actor/sender authority and the actual assigned
job before creating a repository-scoped JIT registration with a unique label.
It must destroy the guest and registration after one job and prove a competing
same-label job receives neither the guest nor any credential.

## 6. Rollback and failure behavior

If the exact-ID or capacity-hold change breaks portable hosted CI, revert only
the workflow/test commit and keep the legacy listener stopped. Reverting to the
name-only guard does not authorize local execution. If GitHub payload
semantics, repository ownership or an approved numeric ID changes, the route
fails hosted/held until the new fact is reviewed and both workflow predicates/
tests are updated.

Do not test rollback by starting the persistent container. Do not treat a
green hosted run as proof of local isolation, or a listener process as proof of
runner health. Local stop/start/restart and host-escape tests belong to the
qualified disposable replacement.

## 7. Rejected shortcuts

- **Repository name, fork bit or author login.** Mutable/incomplete identity;
  it failed on a real same-repository bot PR.
- **Workflow predicate plus persistent fixed label.** The PR can edit its
  workflow and target the label directly.
- **Approval setting only.** Approval executes code; it does not isolate the
  host or constrain a same-repository bot/App.
- **Run PR code directly under `pull_request_target`.** That event uses the base
  context and may have write/secrets authority; checking out and executing PR
  code there creates a privilege-confusion path. A narrowly permissioned
  base-controlled metadata broker may observe/dispatch, but it must not execute
  the PR payload or expose credentials.
- **Reusable fixed registration or secret label.** A queued job is not a
  reservation and label secrecy is not authorization. Admission must bind and
  recheck the actual job before any capacity or credential is released.
- **Container-only replacement.** A rootless container shares the host kernel
  and retains cross-job state; it is not the required hostile-code boundary.

## 8. Primary GitHub references

- [Events that trigger workflows (`pull_request` merge-ref semantics)](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
- [Secure use reference (self-hosted runner warning)](https://docs.github.com/en/actions/reference/security/secure-use)
- [Contexts reference (`github.actor_id` and rerun semantics)](https://docs.github.com/en/actions/learn-github-actions/contexts#github-context)
- [Webhook payload reference (`pull_request.user` and `sender`)](https://docs.github.com/en/webhooks/webhook-events-and-payloads#pull_request)
