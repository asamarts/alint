---
status: accepted
date: 2026-11-17
decision-makers: asamarts
---

# 0046. A linear `extends:` merge, sealed options, and the round's open questions

## Status

Accepted (2026-11-17). Settles the question
[ADR-0043](0043-trust-and-disclosure-decisions-from-the-post-v0221-audit.md) decision 1 left to
the release step (an advisory of the language server's own), and keeps
[ADR-0044](0044-untrusted-runs-git-as-a-process-and-a-wider-memory-gate.md) decision 3 with a
hint added to its warning. The rest of both records stands.

## Context

The second audit round after v0.22.1 merged its fixes (#560 to #578) and left seven questions to
the owner, each asked on its own with options and a recommendation:

- The `extends:` load budget (#577) refuses a flat list of about 90 rulesets of 1,000 rules each,
  or about 2,900 one-rule files, which 0.22 loaded slowly. The cause is a merge that rebuilds
  what it has merged so far for each entry (#580).
- Since #560, a `.git` another user owns makes no repository top on Unix. Root in a container,
  run from a subdirectory of a checkout the host's user owns, skips that checkout's config with a
  warning and finds none.
- Since #560, a bad ignore file (not a regular file, or over 32 MiB) stops the walk, also one in
  a directory above the root, where someone who can write `/tmp` can put one (#568). Git takes an
  ignore file up to 100 MiB.
- Since #560, a local `extends:` that is absolute, rooted, a drive path or a network path is
  refused from its text unless `allow_out_of_root: true`, also when it points inside the root,
  which 0.22 accepted.
- Whether the language server's holes get an advisory of their own (ADR-0043 decision 1).
- `LspOptions` and `LoadOptions` gain fields in most releases: `LspOptions` had 1, 2, 3 and 3
  fields in v0.18, v0.20, v0.21 and v0.22, and has 7; `LoadOptions` went from 2 to 7 to 8. Each
  new field breaks a caller's struct literal.
- The YAML parse budget charges a parse that copies what aliases name the larger of its length
  and its expanded size over 64 (`EXPANSION_BYTES_PER_COST_BYTE`, #562), ayaml 0.1.2's measured
  peak heap of about 65 bytes per input byte.

## Decision

1. **The `extends:` merge becomes linear before v0.23.0** (#580). After phase 7, a list merges
   into one accumulator updated in place, and a property test checks that it equals today's
   merge, one entry at a time, on generated lists: ids that repeat and override, rules without an
   id, `vars` and `env_vars` overrides, `compatibility`, and version 2 diamonds. The budget then
   bounds what entries load, and any list whose merged config fits `MAX_MERGED_ENTRIES` loads.
   The release waits on ayaml in any case, so this delays nothing.
2. **Discovery keeps climbing past another user's checkout, and its warning says how to use the
   config it skips:** `skipping /work/.alint.yml: owned by another user; to use it if you trust
   it, run alint from /work or pass --config /work/.alint.yml`. alint does not take Git's
   `SUDO_UID` rule (containers do not set it) or follow `safe.directory`, which CI images often
   set to `*`, so a repository planted in `/tmp` would govern what is below it again.
3. **A bad ignore file stops the walk, wherever it is, and the cap stays 32 MiB,** the cap of
   every structured input alint reads; the ignore crate compiles each line to a glob, so a
   100 MiB file costs gigabytes. Skipping a bad ignore file above the root needs alint to read
   those files itself, and is decided with #573.
4. **An absolute local `extends:` stays refused unless `allow_out_of_root: true`,** also inside
   the root. The check reads only the text, so no file-system call (an automounted path on Linux,
   a network share on Windows) comes before it, and a committed absolute path names a file on
   one machine only. The error says to name the file relative to the config.
5. **The language server's holes stay in GHSA-78jj-xfpm-g6px,** in its section on opening a
   repository in an editor. One root cause, the trust model, gets one advisory and one CVE, and
   it already lists `alint-lsp` from v0.11.0.
6. **`LspOptions` and `LoadOptions` are `#[non_exhaustive]` from v0.23.0,** as ADR-0032 decision
   3 sealed `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions`. Each field has
   a `with_*` setter beside the existing constructors (`LoadOptions::with_catalog`, `with_cache`
   and `cache`), and the fields stay public to read and to assign. A `compile_fail` doctest in
   each crate fails if a struct loses the attribute.
7. **The alias factor stays 64.** At the ayaml bump, the memory test's peak heap per input byte
   for the new release sets it again, and `yaml-parser.md`'s bump steps say so.

## Consequences

- #580 joins the work before the v0.23.0 tag, after phase 7.
- A user who runs alint below another user's checkout is told how to proceed; nothing else
  changes there.
- #568 stays open for #573. Someone who can write a directory above an unpacked archive can make
  alint fail there with an error that names the file, but not hang or exhaust memory.
- A configuration that names its own files by absolute path fails to load until it names them
  relative to itself; the CHANGELOG's "Upgrading from 0.22" list says so.
- RELEASING step 5 publishes one advisory for v0.23.0.
- A caller that builds `LspOptions` or `LoadOptions` with a struct literal, even with
  `..Default::default()`, moves to the setters; adding a field later is not a breaking change.
