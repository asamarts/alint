---
status: accepted
date: 2026-11-17
decision-makers: asamarts
---

# 0044. `--untrusted`, Git as a process, and a wider memory gate

## Status

Accepted (2026-11-17). Refines
[ADR-0043](0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md) decisions 2, 4 and 6,
and corrects its decision 8 and "The ayaml condition" (see "A correction to ADR-0043"). Amends
[ADR-0042](0042-measure-the-yaml-parser-by-memory-and-wall-time.md) decisions 1, 2 and 4: the
memory test measures nine document shapes, the release bound holds for each of them, and the
parse budget charges what aliases expand to. The rest of both records stands.
[ADR-0046](0046-a-linear-extends-merge-sealed-options-and-the-rounds-open-questions.md) keeps
decision 3 and adds a hint to its warning.

## Context

- **The audit.** A second round of six reviewers audited what landed after the first audit,
  #538 to #558 on `main` at 6837a4dfa: the first audit's fixes, and phase 6 of the
  [v0.23 plan](../design/v0.23/README.md) (the experimental `version: 2` language). The record is
  the 2026-11-17 entry of the plan's status log.
- **Trust.** ADR-0043 decision 2 made `--no-spawn` the way to lint a repository the user does not
  trust with its own configuration, and kept the language server from running anything until a
  workspace is trusted. The review found three gaps in that:
  - Git runs the commands a repository's own `.git/config` names (`core.fsmonitor` when it reads
    the index, `gpg.program` when it verifies a signature). A rule that runs Git, `--changed` and
    an untrusted language server all ran Git, so a cloned repository's `.git/config` ran a
    command under `--no-spawn` and on opening a folder.
  - `--no-spawn` still fetched a remote `extends:`, and its URL may carry an environment value
    (`{{env.TOKEN}}`), so a configuration under `--no-spawn` could send a secret to a host it
    named. The language server's untrusted mode refuses both; the command line had nothing that
    did.
  - Discovery took any `.git` as the repository's top. An empty `.git` file that another user
    planted above a checkout stopped the climb there, and the owner check that decision 4 put on
    ancestors never ran.
- **Enum values.** ADR-0043 decision 6 refused a one-entry mapping where a unit enum goes. A
  fix op's options are read through an untagged enum, which buffers its input, and the trust gate
  compared `applicability` as a string only. An extended or nested configuration writing
  `applicability: {safe: ~}` promoted its fix to apply without asking, a hole since v0.17.0.
- **Memory.** ADR-0042's memory test measures OpenAPI-shaped documents only. The review measured
  other shapes, and ayaml 0.1.2 is further from libyaml on most of them: from 1.73x on a block
  sequence of strings to 3.06x on a block sequence of short words. The parse budget charged a
  parse its input's length, so many small files whose aliases expand into large trees parsed at
  once: sixteen of 500 KB each peaked at 3.1 GB.

## Decision

1. **`--no-spawn` keeps its name and refuses every process, Git included.** A rule that runs Git
   (the `git_*` kinds, `changeset_requires_path`, `pair_changed_together`, `git_tracked_only`,
   `scope_filter.changed_since`, a `git_untrack` fix, at any `require:` depth) is refused when the
   configuration loads, `--changed` and `--base` are refused, and once the flag is read the process
   runs no Git command at all: each Git helper reads as Git finding no repository. The language
   server skips such rules until the workspace is trusted.
2. **A new global `--untrusted` is the language server's untrusted mode on the command line.** It
   refuses what `--no-spawn` refuses, refuses a remote `extends:` before connecting, and gives the
   configuration an empty environment (`{{env.*}}`, `env.*` in a `when:`, and a legacy `${VAR}`).
   `alint lsp --untrusted` takes every workspace as untrusted, whatever the client says.
   SECURITY.md and the documentation send a repository the user does not trust to
   `--untrusted`.
3. **A `.git` is the repository's top only when the current user owns it and its directory.**
   Another user's `.git` is taken as content, and discovery climbs past it under decision 4 of
   ADR-0043, with its owner check. On Windows, where alint reads no owner, every `.git` counts, as
   before.
4. **Every enum value that names a mode is taken only as its name, written as a string.**
   - A fix op's options decode by the op's key through the checked decoder, so
     `applicability: {safe: ~}` is an error that lists the tiers.
   - The trust gates refuse, in an extended or nested configuration, and strip, from a rule with
     extended provenance, any `applicability` that is not a string or null, and a `{{` placeholder
     anywhere inside one.
   - `file_graph`'s `require:` and `cross_file`'s `normalize:` take their values the same way.
5. **The memory test measures nine shapes, and the release bound holds for each.** It measures
   OpenAPI documents of 37 KB, 1.1 MB and 4.4 MB, and 1 MB documents of a block mapping, a flow
   mapping, a flow sequence of integers, a flow sequence of `1`, a block sequence of strings and a
   block sequence of short words, each in a process of its own. Each shape has a regression bound
   of its ratio today and about 10%. The release bound of 1.2 applies to all nine, so v0.23.0
   waits for an ayaml release that brings every shape within it.
6. **The parse budget charges a parse the larger of its length and its expansion.** A parse
   reserves its input's length, or the guard's estimate of what its aliases expand to divided by
   64 (about ayaml's peak bytes per input byte), whichever is larger. A small file with a large
   expansion waits for its share, as a large file does.
7. **A structured fix changes only its target.** A `set_value`, `remove_value` or `replace` on
   YAML, JSON or TOML re-reads the whole file after its edits and applies only when the result is
   the original tree with the intended change. When two rules' fixes together would change more
   than their targets, the later rule waits for the next fix pass. A YAML set or removal at a node
   that an alias or merge key names, or on a path through one, is declined, since the edit would
   change every alias. Removing a block mapping's last entries writes `{}`.
8. **What alint reads whole is bounded.** An ignore file the walk reads (in the tree, above it,
   the global excludes file, `info/exclude`, a worktree's `.git` file and `commondir`) must be a
   regular file of at most 32 MiB, or the walk stops with an error naming it. A baseline must be a
   regular file of at most 256 MiB, and one the `baseline:` key names must be inside the root and
   outside `.git`. A local `extends:` that is absolute, rooted, a drive path or a network path is
   refused from its text, before any file-system call, unless `allow_out_of_root: true`.
9. **The advisory covers this round's holes too.** GHSA-78jj-xfpm-g6px gains the fix promotion
   (v0.17.0 to v0.22.1), the structured YAML fix crashes #540 fixed (v0.17.0 to v0.22.1), the
   unbounded ignore-file and baseline reads, and the network path an `extends:` could open on
   Windows. The others this round fixed are gaps in protections added
   after v0.22.1, which the advisory already describes as missing from released versions.

### A correction to ADR-0043

ADR-0043 decision 8 says "Decisions 8 and 9, on YAML memory and wall time, are recorded in
ADR-0042." ADR-0042 numbers those decisions 1 to 4. ADR-0043's "The ayaml condition" lists the gaps
that ayaml's main branch fixes, and the pull requests that close them, but leaves out
asamarts/ayaml#138, which ayaml's #156 fixed on main too; the condition covers it. ADRs are
immutable, so the corrections are recorded here, and ADR-0043's status points to them.

## Consequences

- A repository's Git rules do not run under `--no-spawn`, so a configuration that relies on them
  fails to load there; the error names the rule. `alint test` is unchanged: it runs Git only in
  the repository its sandbox makes.
- A user who lints a repository they do not trust needs `--untrusted`, not `--no-spawn`, to keep
  its configuration from reading the environment or fetching a remote ruleset. `LspOptions` gains
  a field, which breaks a caller that builds it with a struct literal.
- A checkout another user owns, entered from a subdirectory, no longer takes its configuration
  from the top; discovery climbs on, with a warning for each configuration another user owns.
- A configuration that wrote an enum value as a mapping or a tag fails to load, with an error
  that lists the values.
- The release waits on ayaml improving every shape. At the 2026-11-17 measurements, ayaml 0.1.2
  is at 1.73x to 3.06x. Its main branch, at a561ae2, is within 1.2x on the three OpenAPI shapes
  and at 1.53x to 2.32x on the six 1 MB shapes. The shapes and ratios are reported to
  asamarts/ayaml#163 and #178.
- Each structured fix does one more full parse, and a batch of fixes from several rules about
  two more. A fix that the whole-document check declines is reported as declined, not applied.
- A regular ignore file over 32 MiB stops the walk, where Git accepts one up to 100 MiB, and a
  bad ignore file in a directory above the root fails the walk instead of being skipped.
- An absolute local `extends:` that names a file inside the root needs `allow_out_of_root: true`
  or a relative path.

## Considered Options

- **Remote sources and the environment on the command line:** widen `--no-spawn` to refuse them
  too, so its name no longer says what it does; add `--untrusted` beside it; or document the gap.
  The owner chose `--untrusted`, keeping `--no-spawn` for a configuration the user trusts not to
  leak but not to run commands.
- **Git under `--no-spawn`:** allow it, since alint names the repository to Git and reads no hook;
  or refuse it, since Git runs what `.git/config` names. The owner's decision on `--no-spawn`
  ("refuses every process, Git included") chose the second.
- **The release bound's shapes:** keep it on the OpenAPI shapes ADR-0042 measured and track the
  others; apply it to the shapes of real configurations and manifests (OpenAPI, block mappings,
  block sequences of strings); or apply it to every shape the test measures. The owner chose every
  shape.

## More Information

- [ADR-0042](0042-measure-the-yaml-parser-by-memory-and-wall-time.md) and
  [ADR-0043](0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md): the decisions this
  record refines.
- [SECURITY.md](../../SECURITY.md): the threat model, with `--untrusted`.
- The round's fixes: the security, YAML, `version: 2` loader, relation and engine pull requests
  that follow #559, and the round's documentation pull request.
