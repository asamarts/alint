# `pair_hash` — a target file must carry the digest of a source file

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #8 of the case-study coverage push — the
**last** rule kind in the cut; the remaining v0.10 items #9/#10
are bundled rulesets). Was a design draft (2026-05-18). v0.10
demand #8 (3 sources, ROADMAP-canonical). Open questions
resolved on implementation: `contains` + `sums-line` modes,
fixed byte-offset deferred (Q1); `sha256` (default) + `sha512`
via the existing `sha2` dep, no new dependency, sha1/blake3
deferred (Q2); detection-only, no `.sum`-regenerating fix (Q3);
sibling of `file_hash` / `generated_file_fresh`, docs cross-link
(Q4). Cross-file rule (the `pair` dispatch class).

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("`pair_hash` — computed property of file A appears … in file
B", 3 sources: kubernetes, tokio, golang/go FIPS — "golang/go
FIPS is the highest-stakes use case (CMVP submission references
the file format)") and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`pair_hash` row: golang-go, kubernetes, tokio). Canonical
scope: [`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#8; "Hash of file A appears … in file B").

## Problem

A repository commits a **checksum that pins one file's content
inside another file**, and nothing enforces that the digest
still matches:

- **golang/go FIPS** (highest stakes): the FIPS 140-3 module
  ships `src/crypto/internal/fips140/fips140.sum`, a
  `sha256sum`-style manifest (`<hex>  <relpath>` lines) of every
  file in the validated module. If a module source changes and
  the `.sum` is not regenerated — or a file is added to the
  module but not the manifest — the CMVP-submitted module no
  longer matches its own integrity record. This is a compliance
  artefact, not a convenience.
- **kubernetes / tokio**: the same shape at lower stakes —
  generated or vendored content with a committed checksum
  (a `.sum` / `SHA256SUMS` / an embedded hash) that silently
  drifts from the file it pins.

`file_hash` pins **one file** to a **literal hash written in the
`.alint.yml`** — it cannot express "file B must contain the
*current* digest of file A" (the manifest is the source of
truth, and there may be hundreds of entries). `generated_file_fresh`
diffs a *generator's stdout*; there is no generator here, just
two committed files where one carries the other's hash. The gap
is a precise cross-file relation: **the digest of A appears in
B, in the manifest/embedded form the ecosystem uses.**

## Surface area

New cross-file rule kind `pair_hash` in `alint-rules`.
`version: 1` unchanged.

```yaml
- id: fips-sum-pins-module
  kind: pair_hash
  source: "src/crypto/internal/fips140/v1.0.0/**/*.go"  # file(s) A — literal or glob
  target: "src/crypto/internal/fips140/fips140.sum"      # file B — must carry A's digest
  algorithm: sha256                                       # sha256 (default) | sha512
  format: sums-line                                       # contains (default) | sums-line
  level: error
```

- `source` is a literal path or a glob. A glob runs **one check
  per matched file** (mirrors `pair`'s `primary`): zero matches
  ⇒ no violations (nothing to pin), same as `pair`.
- `target` is the single file B that must carry the digest.
  **A missing `target` is always a violation** (the manifest is
  mandatory) — anchored on `target`.
- `algorithm`: `sha256` (default) or `sha512`. Both from the
  `sha2` crate already in the dependency graph (`file_hash` uses
  it) — **no new dependency**.
- `format`:
  - `contains` (default): A's lowercase-hex digest must appear
    as a substring **anywhere** in B (case-insensitive). The
    "embedded hash" form.
  - `sums-line`: B must contain a `sha256sum`-style line whose
    whitespace tokens are `<hex> [*]<path>` where `<hex>` equals
    A's digest **and** `<path>` equals A's repo-root-relative
    path. The coreutils / go-`.sum` manifest form (a leading
    `*` binary marker and the conventional double space are
    tolerated).

### Option naming (v0.10 pre-release harmonization)

`source:` is the authoritative input (the file/glob whose
digest is taken); **`target:`** is the single file that must
carry the digest. The checked-file option was `in:` through the
v0.10 development cycle and was renamed to `target:` before the
v0.10 release as part of the cross-file-family harmonization
(aggressive decision: every cross-file rule's authoritative
input is `source:` and its checked file(s) is `target`/
`targets:`). All v0.10 work is on `[Unreleased]`, so no shipped
release is affected. **`pair_hash` is not a sibling of the
long-shipped `pair` rule** (which keeps `primary:` / `partner:`):
it is a different relation — a checksum-presence check, not a
file-pairing existence check — and the shared cross-file
dispatch class is an implementation detail, not an interface
kinship.

## Semantics

Cross-file (`requires_full_index() == true`, `path_scope() ==
None` — the `pair` dispatch class; `scope_filter` is rejected at
build, like `pair`). One `evaluate`:

1. Read `target` (B) once. Missing/unreadable ⇒ **one**
   violation anchored on `target` ("manifest `B` does not
   exist").
2. For every index file matching `source` (A):
   - compute `algorithm(A bytes)` → lowercase hex.
   - `contains`: hex is a case-insensitive substring of B ⇒
     pass; else one violation anchored on **A** ("sha256 of `A`
     not found in `B`").
   - `sums-line`: scan B's lines for one whose path token equals
     A's path. None ⇒ violation on A ("`A` is not listed in
     manifest `B`"). Found but hex token ≠ A's digest ⇒
     violation on A ("digest mismatch for `A`: manifest has X,
     file hashes to Y"). Found and equal ⇒ pass.
3. One violation per offending source; anchored on the **source**
   (the actionable file) except the missing-`target` case
   (anchored on `target`).

Raw file bytes are hashed, no normalisation (matches
`file_hash`) — a CRLF/LF change *is* a digest change, which is
correct for an integrity pin. Detection-only: no fix op (alint
will not regenerate a checksum manifest — that is the manifest
generator's job; same posture as `file_hash`).

## False-positive surface

- **Path base in `sums-line`.** The path token is compared to
  A's repo-root-relative path (forward slashes) after stripping
  a coreutils binary-mode `*` marker and a `find`-style `./`
  prefix (`<hex>  ./path`, what `find … -exec sha256sum` and Go
  tooling emit). Backslash path separators are **not** normalised
  — the `.sum` formats in scope are forward-slash. A manifest
  that lists module-relative paths needs `source` scoped so the
  paths line up, or `format: contains`. The go FIPS `.sum` is
  repo-root-relative, matching the index.
- **Whitespace / binary marker.** `sums-line` splits on ASCII
  whitespace and tolerates the coreutils double space and a
  leading `*` on the path (binary mode). Anything more exotic ⇒
  use `contains`.
- **Hash casing.** Compared case-insensitively (manifests are
  conventionally lowercase; stay forgiving like `file_hash`'s
  `parse_sha256`).
- **`contains` substring collision.** A full sha256/sha512 hex
  string colliding by accident is cryptographically negligible —
  that is the entire premise of a hash; accepted, not mitigated.
- **`source` glob matching `target`.** For `sums-line` a
  manifest not listing itself is normal; scope `source` to
  exclude `target` (go FIPS does). Documented, not an error.
- **Byte-exactness.** Hashing the raw bytes means an autocrlf or
  trailing-newline change flips the digest. Intended (it is an
  integrity pin); called out so it is not surprising.

## Implementation notes

- Module `crates/alint-rules/src/pair_hash.rs`, modelled on
  `pair.rs`: `impl Rule { rule_common_impl!();
  requires_full_index()->true; evaluate() }`, no `path_scope`,
  `alint_core::reject_scope_filter_on_cross_file(spec,
  "pair_hash")` in `build` (same as `pair`).
- Digest: `sha2::{Sha256, Sha512}` + a small `digest_hex`
  helper; hex via the same lowercase encoder shape as
  `file_hash::encode_hex`. No new crate.
- `source` → `Scope::from_patterns` (glob over the index, like
  `pair`'s `primary_scope`); `target` read via `ctx.root.join`.
- Not spawn-capable (pure read + hash + substring) — the
  `SPAWNING_RULE_KINDS` trust gate is N/A; the "does this kind
  spawn?" checklist item was still evaluated (see
  `feedback_spawn_kinds_must_be_gated`).
- No `include_str!`; nothing leaves the crate.

## Tests

- `contains`: digest present ⇒ silent; absent ⇒ one violation
  on the source; `sha512` variant.
- `sums-line`: matching `<hex>  <path>` line ⇒ silent; wrong
  hash for a listed path ⇒ mismatch violation; path absent from
  manifest ⇒ "not listed" violation; leading `*` binary marker
  tolerated.
- Missing `in` ⇒ one violation anchored on `in`. Glob `source`
  with several files ⇒ one violation per offender, pass-through
  for the matching ones. Zero `source` matches ⇒ silent.
- Case-insensitive hex; raw-byte exactness (a trailing-newline
  diff flips the verdict).
- Lockstep with the codebase invariants (same checklist
  #1–#7 followed): registered (+ in the registry test list);
  `rule_pair_hash` `$def` + dispatch `$ref` in both mirrored
  `config.json` (mirroring `rule_pair`); `all_kinds.yaml` entry;
  regenerated default-options snapshot; **both** a firing and a
  silent `cross_file/` e2e scenario (the
  `coverage_audit_pass_fail` requirement); rule count **78 →
  79** across README ×2 / `docs/site/about` /
  `coverage_audit_readme_claims`; `docs/rules.md` `### pair_hash`
  under `## Cross-file` (so `xtask docs-export --check` stays
  green); CHANGELOG `[Unreleased]` Added (the eighth and final
  v0.10 rule-kind item).
- **Bench-compare threshold:** O(sum of source sizes) hashing +
  one B read; cross-file dispatch (no per-file hot path). Full-
  run S-class wall must not regress vs the pre-phase baseline
  (`xtask bench-gate`, per `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Fixed byte-offset mode.** ROADMAP phrases it "at offset Y";
   none of the 3 demand sources are fixed-offset (all are
   `.sum`-manifest or embedded-substring). `contains` +
   `sums-line` cover them. An `offset:` / `length:` window is
   deferred until a source needs it.
2. **More algorithms.** `sha256` + `sha512` (the `sha2` dep,
   covers go FIPS = sha256) ship. `sha1` (legacy) / `blake3`
   would each add a crate with no current demand — deferred.
3. **Auto-fix.** Regenerating the `.sum` line is the manifest
   generator's job, not a linter's (same stance as `file_hash`).
   Detection-only; revisit only with a strong, safe use case.
4. **Relationship to `file_hash` / `generated_file_fresh`.**
   `file_hash`: one file vs a *literal* hash in the config.
   `generated_file_fresh`: a *generator's* stdout vs a committed
   file. `pair_hash`: cross-file, B carries A's *current*
   digest in a manifest/embedded form. Distinct kinds; docs
   cross-link.
