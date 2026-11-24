---
status: superseded by ADR-0048
date: 2026-11-17
decision-makers: asamarts
---

# 0045. Count the walk's per-directory probes in the reference gate

## Status

Superseded by [ADR-0048](0048-v024-owner-decisions.md) decision 5a (2026-11-24, #573): the owned
parallel walker inspects ignore files directly from each directory's listing with open-then-`fstat`
checks, eliminating the per-directory `stat` probes and retiring the per-directory `probe`
allowance in `ci/scripts/reference-perf-gate.sh`. Originally accepted (2026-11-17), amending
[ADR-0024](0024-relation-ir-decisions-after-review.md) decision 6.

## Context

The second audit round after v0.22.1 stops the walk from reading an ignore file that is not a
regular file of at most 32 MiB (#560, advisory item 17): a committed `.gitignore` symlinked to
`/dev/zero` exhausted memory, and a FIFO hung the walk. `ignore` 0.4.34 lists a directory and
reads its `.ignore`, `.gitignore` and repository files in one step, with no hook in between, so
the walk stats `.ignore`, `.gitignore` and `.git` itself first: in each directory it enters, in
the root, and in each directory above the root. Upstream declined to bound the reads in the
crate (BurntSushi/ripgrep#3569, closed as not planned on 2026-10-23).

The reference gate allows 32 calls per family over v0.18.2 on the 10k-file trees. The check adds
about 1,230 `stat` calls there (402 directories), and #560 failed the gate. Measured on S1 to S3,
with the tree inside a repository as CI places it, the stats of `.git`, `.jj`, `.gitignore` and
`.ignore` that HEAD makes beyond v0.18.2's are:

- 3 for each directory in the tree, the root included: the check;
- 5 for each directory above the root: the check, and `ignore`'s own probes for `.git` and
  `.jj`, which it makes under `require_git` (set in a repository since ADR-0028) and which
  v0.18.2 did not make;
- a few near the root, from the repository-boundary check.

Before the check, main was already 23 to 29 calls over v0.18.2, nearly all of them `ignore`'s
probes above the root. A flat allowance therefore fails at any depth, and one that grows by 3
for every directory in and above the tree leaves about 8 calls of headroom, less for a deeper
checkout.

The owner chose to keep the check in v0.23.0 and amend the gate (2026-11-17), and to look at
removing the cost by reading ignore files from the walk's own listing (#573).

## Decision

1. `ci/scripts/syscall-counts.py` counts a stat-family call on a file named `.git`, `.jj`,
   `.gitignore` or `.ignore` in a fifth family, `probe`, instead of `stat`. A trace with no
   probes is still a measurement.
2. The `probe` family may exceed v0.18.2 by 32 calls, plus 3 for each directory in the tree
   (outside `.git`) and 5 for each directory above it. The gate counts the directories from the
   tree's path, not from HEAD's run, so HEAD cannot raise its own allowance.
3. Every other family keeps the allowance of 32 calls.
4. The walk canonicalizes the root once and passes the result to the check, which had
   canonicalized it a second time.

## Consequences

- #560 passes with about the headroom the gate had before: on S1 to S3 the `probe` family is
  about 26 calls under its allowance, and `stat` is at most 12 calls over v0.18.2.
- The `stat` family no longer carries `ignore`'s probes above the root, so it regains the
  headroom they took.
- A change that probes a fourth name in every directory, or the same names twice, still fails:
  the allowance is the check's cost, not a multiple of it.
- The rates describe what `ignore` 0.4.34 and the check do. A walk without the check (#573), or
  an `ignore` that probes differently, changes them through a new ADR.

## Considered Options

- **Bound only the files read before the walk** (the root's and its parents' ignore files,
  `info/exclude`, the global excludes file): no cost per directory, but a symlink committed in a
  subdirectory would still exhaust memory. Not chosen.
- **Carry a patched `ignore`** whose existing existence check also checks the file's type and
  size: no extra calls, but a published fork to keep up to date. Left to #573.
- **A flat higher allowance** (for example 1,500 calls): simple, but it says nothing about what
  it allows, and it would hide a per-file regression on a tree with fewer directories.
- **3 for each directory in and above the tree:** one rate, but `ignore`'s probes above the root
  would stay in the fixed 32, leaving about 8 calls of headroom.

## More Information

- [ADR-0024](0024-relation-ir-decisions-after-review.md): the reference gate.
- [`docs/benchmarks/REFERENCE-v0.18.2.md`](../benchmarks/REFERENCE-v0.18.2.md): what the gate
  measures.
- `ci/scripts/reference-perf-gate.sh`, `ci/scripts/syscall-counts.py` and
  `ci/scripts/test-reference-perf-gate.sh`.
