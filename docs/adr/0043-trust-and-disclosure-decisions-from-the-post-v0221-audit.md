---
status: accepted
date: 2026-11-16
decision-makers: asamarts
---

# 0043. Trust and disclosure decisions from the post-v0.22.1 audit

## Status

Accepted (2026-11-16). Widens [ADR-0040](0040-the-v023-advisory-and-the-allowlist-approval.md)
decision 1: the advisory it drafted covers the holes this audit found as well, and its "all seven
holes" undercounts the holes it already covered (see "A correction to ADR-0040"). ADR-0040
decision 2's approval stands, and its condition now asks for every ayaml fix of a gap alint filed
(see "The ayaml condition").
[ADR-0042](0042-measure-the-yaml-parser-by-memory-and-wall-time.md) records the audit's decisions
on YAML memory and wall time.

## Context

- **The audit.** Seven reviewers audited everything alint delivered since v0.22.1, #490 to #530,
  on `main` at e7bb08a57: the engine and rules, YAML robustness, the library API, security,
  user-facing YAML, the documentation and release records, and performance. They reported 80
  findings, 10 of them high. The record is the 2026-11-16 entry of the
  [v0.23 plan](../design/v0.23/README.md)'s status log.
- **Security.** The audit found 13 holes beside the ones ADR-0040's advisory covers. Among them:
  - a fix from a ruleset that `extends:` brings in could write alint's own configuration, so the
    next `alint check` ran a `command` rule from it, and a content fix wrote through an in-repo
    symlink into `.git/config`;
  - the language server ran a cloned repository's command rules, `custom:` facts and remote
    `extends:` as soon as a file was opened, and gave a vendored `.alint.yml` (under
    `node_modules/`, say) a top-level config's trust;
  - any rule, from any configuration, could read `.git/config`, which can hold the credentials
    `actions/checkout` persists, and print them in its message;
  - configuration discovery climbed above the repository to `/`, so a `.alint.yml` someone else
    wrote in `/tmp` or a shared home governed a checkout under it;
  - `safe.bareRepository=explicit`, which #522 relies on, does nothing before Git 2.38.

  The same review found the draft advisory inaccurate: it said `alint check` was not affected,
  left `alint-core` out of the affected packages, and claimed patches that `main` did not yet
  have.
- **What a repository's configuration may do.** SECURITY.md said alint was "designed to be safe
  to run against untrusted repositories", while `alint check` in a clone runs that clone's own
  command rules. Rulesets from `extends:` were kept from spawning, but a local one is treated as
  the user's own tree in every other respect.
- **YAML.** Since the ayaml cutover (#510), a one-entry mapping where an enum takes a unit
  variant's name is read as that variant (`level: {error: ~}` loads as `level: error`), which
  v0.22 refused. Three messages got worse: an indentation mistake reports "unexpected content
  after document", an unclosed quote no longer says where it opened, and a type error decoded
  from a value tree names only the kind of what it found.
- **Performance.** ayaml takes more memory and wall time than libyaml on large files, and the
  performance gates count instructions only. ADR-0042 decides what to measure and what to hold
  the release to.

## Decision

1. **Disclosure.** The new holes are fixed in public pull requests now, as ADR-0040 chose for
   #527. Draft advisory GHSA-78jj-xfpm-g6px is widened to cover them, corrected where the
   review found it wrong, and published at the v0.23.0 release. Whether the language server's
   holes get an advisory of their own is left to the release step.
2. **A repository's own top-level configuration is trusted, like a Makefile.**
   - `alint check` in a clone runs that configuration's command rules, spawning fixes and
     `custom:` facts, as before. SECURITY.md says so.
   - A new global `--no-spawn` refuses a configuration whose rules, fixes or facts start a
     process, for linting a repository the user does not trust with its own configuration.
   - The language server runs no spawning rule or fact, fetches no remote `extends:` and reads
     no environment value until the workspace is trusted. VS Code passes its Workspace Trust
     state; other editors opt in with the `trusted` initialization option.
3. **No rule reads inside a Git directory,** whatever configuration it comes from, the user's
   own included. A path a rule names there is a load error, and a path that a symlink leads into
   is refused when it is read.
4. **Configuration discovery stops at the repository's top,** the nearest directory holding a
   `.git` directory or file. Outside every repository it climbs as before, but skips, with a
   warning, a configuration file another user owns in a directory above the one it starts from.
5. **A local `extends:` file stays the user's own tree.** It may read `env.*` in its conditions
   and declare `ignore:`, and the documentation says so. Every extended source (local, bundled
   or remote) is refused what would let it act beyond its rules: spawning, a fix that writes
   alint's configuration, a fix that writes or reads outside the root (whatever
   `allow_out_of_root` says), and reads in a Git directory.
6. **A one-entry mapping where a unit enum variant goes is refused,** as v0.22 refused it:
   `level: {error: ~}` is an error that lists the levels. A variant with data still reads from
   a mapping or a tag.
7. **YAML message regressions are filed with ayaml** (asamarts/ayaml#160, #161 and #162), and
   alint improves what it can meanwhile: a type error names the value it found, a unit enum's
   error lists the allowed values, and "unexpected content after document" from a likely
   indentation mistake ends with a hint to check the line's indentation. The unclosed quote's
   message waits on ayaml.
8. **Decisions 8 and 9, on YAML memory and wall time,** are recorded in ADR-0042.

### A correction to ADR-0040

ADR-0040 decision 1 says the advisory "covers all seven holes": the four that #515, #519 and
#522 fixed, and the three it names that the review of #522 found. That review found eight, all
fixed in #527: the three ADR-0040 names, and a fix step's `relocate` that moved a symlink out of
the test's repository, Git discovery that climbed above the sandbox, conditions that read the
host's `PATH`, the host's global excludes under the host keyring, and spellings of `.git` that
the Git-directory check missed. The fixer's `.git` write also affected `alint fix` with a ruleset
from `extends:` since v0.2.1, not only `alint test`. The advisory covers the holes fixed in #515,
#519, #522 and #527, and those this audit's pull requests fix. ADRs are immutable, so the
correction is recorded here, and ADR-0040's status points to it.

### The ayaml condition

ADR-0040 decision 2 tags v0.23.0 only on an ayaml release that fixes asamarts/ayaml#132, #133 and
#137. ayaml's main branch now fixes every gap alint filed against the compatibility reader:
#132, #133, #137, #139, #140, #141, #146 and #147, closed by ayaml's pull requests #152 and #154
to #159, none of them released yet. The condition becomes an ayaml release that contains all of
those fixes; ADR-0042 adds one on memory. The bump follows the procedure in
[`yaml-parser.md`](../design/yaml-parser.md) ("Bumping ayaml"), and a difference it adds still
comes back to the owner before the tag.

## Consequences

- The advisory, the CHANGELOG's Security section and SECURITY.md describe the same set of fixes.
  RELEASING.md gains the step that corrects and publishes the advisory and updates SECURITY.md.
- A user who opens a repository in an editor other than VS Code sees no command rule, custom
  fact, remote `extends:` or environment value until they set the `trusted` option. The JetBrains
  plugin does not send it yet (#545).
- A configuration that relied on reading `.git` (to compare a remote's URL, say) fails to load,
  and the user's own `file_create` into `.git/hooks/` is skipped; hooks belong in a tracked
  directory under `core.hooksPath` or with a hook manager.
- A repository without a configuration of its own reports `no .alint.yml found` where a
  configuration above it used to govern it.
- A vendored local ruleset still reads the environment in its conditions: vendor one only as you
  would vendor code, after reading it.
- The YAML messages that alint improves change again when ayaml's own messages improve; the tests
  of decision 7 pin alint's wording.

## Considered Options

- **Disclosure:** fix the new holes privately and publish them only with the release, or open a
  second advisory for them now. The owner chose public fixes and one widened advisory at the
  release, as for #527.
- **A repository's configuration:** treat it as untrusted input, with a trust prompt in the CLI;
  or document it as trusted, with `--no-spawn` for the other case and a trust state in the
  server. The owner chose the second.
- **Rules reading Git directories:** keep it and document it; refuse it to extended sources only;
  refuse it everywhere. The owner chose to refuse it everywhere: the walk already leaves `.git`
  out.
- **A local `extends:` file:** make it a trust boundary that also loses `env.*` and `ignore:`,
  or keep it the user's own and refuse every extended source the actions above. The owner chose
  the second.
- **The one-entry mapping:** accept and document it, since serde reads it as an enum's tagged
  form; or refuse it, as v0.22 did. The owner chose to refuse it.
- **The messages:** file them with ayaml and wait; rewrite them in alint; or document them only.
  The owner chose to file them, and to have alint improve its type, enum and indentation errors
  now.

## More Information

- [ADR-0040](0040-the-v023-advisory-and-the-allowlist-approval.md): the advisory this record
  widens, and the allowlist approval that stands.
- [ADR-0042](0042-measure-the-yaml-parser-by-memory-and-wall-time.md): the audit's decisions on
  YAML memory and wall time.
- [SECURITY.md](../../SECURITY.md): the threat model these decisions set.
- The audit's fixes: #538 to #543, and the security follow-ups, YAML messages, performance gates
  and documentation that follow them. The advisory: GHSA-78jj-xfpm-g6px, a private draft until
  v0.23.0.
