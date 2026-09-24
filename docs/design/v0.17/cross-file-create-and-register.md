# Cross-file create-and-register (`relation: registered` + `fix: create_and_register`)

Status: **DESIGN / IN PROGRESS** (the last Phase-3 auto-fix op; auto-fix.md 2 "Cross-file
atomic create-and-register" + 5.2.4 "Multi-file transactions"). Work branch
`phase-0-fix-engine`.

## 1. Motivation

A workspace member must both **exist** and be **registered** in a manifest list: a crate
directory listed in `[workspace] members`, a package listed in a `CODEOWNERS` block, a module
added to a barrel file. A half-applied create-without-register (or an existing-but-unregistered
member) leaves the tree inconsistent. `alint` should detect the drift and, on `fix`, converge it.

## 2. The model: two idempotent postconditions

The elegant framing (asamarts, 2026-09-24): `create_and_register` ensures **two independent
conditions** on each member and applies only the edit for whichever is unmet:

- `exists(member)` — repaired by **create** (from the fix's `content:` / `content_from:`).
- `registered(member)` — repaired by **appending** the member's value to the target list.

"Create only if it does not already exist" is therefore not a mode flag: it is just the existence
condition's repair, which is a **no-op whenever the member already exists** (always true for a
glob-discovered member — a glob only matches paths that exist). One rule, one op; the shape of
the work falls out of the current state:

| Current state | Edits applied | Shape |
|---|---|---|
| member exists, not registered | register only | **1 file** (a list append) |
| member missing (named source) | create + register | **2 files** (the 5.2.4 transaction) |
| exists **and** registered | none | no violation |

The multi-file transaction (5.2.4) only engages when a create actually fires; the common case
(an existing-but-unregistered member) degrades to a plain single-file list append.

## 3. Host + config surface

Host: the existing **`cross_file`** rule (which already hosts `sync_from` + value-propagation),
via a new **`relation: registered`**. Consistent with the arc's reuse pattern and the auto-fix.md
grouping of create-and-register with the `cross_file` family.

**Register existing members (glob source — create is inert):**
```yaml
# every crate (identified by its manifest) must be listed in workspace.members
- id: members-registered
  kind: cross_file
  relation: registered
  source: { files: "crates/*/Cargo.toml" }     # each crate, by its manifest
  register_as: "{dir}"                          # register its DIRECTORY (crates/foo)
  targets:
    - { file: Cargo.toml, extract: { toml: "$.workspace.members[*]" } }
  fix: { create_and_register: {} }             # no content -> registers only
```

**Ensure a required member exists + is registered (named source — create-if-missing, Phase 2):**
```yaml
- id: docs-crate
  kind: cross_file
  relation: registered
  source: { file: "crates/docs/Cargo.toml" }   # a specific required member
  register_as: "{dir}"                          # value to add: crates/docs (the parent dir)
  targets:
    - { file: Cargo.toml, extract: { toml: "$.workspace.members[*]" } }
  fix:
    create_and_register:
      content_from: .alint/templates/crate.toml   # used ONLY if the file is missing
```

- **`source`**: a `files:` glob (members = the matched paths) or a `file:` (one named member, may
  be missing → creatable in Phase 2). **Anchor a glob to a manifest** (`crates/*/Cargo.toml`), not
  a bare directory glob (`crates/*`): a bare glob also matches LOOSE files under the directory (a
  `crates/README.md`, a `.gitkeep`), which would be flagged as members and appended as invalid
  entries. `register_as: "{dir}"` then maps each matched manifest to its crate directory.
- **`register_as`**: the value appended to the list, as a `{path}`/`{dir}`/`{stem}` template over
  the matched member path (same templating as the `command` fix). **Defaults to `{path}`**. Note
  `normalize` is REJECTED on `registered` — the member path is registered verbatim, never a
  normalized form.
- **`targets`**: one or more `{ file, extract }`, where `extract` is a structured JSONPath that
  selects the array ELEMENTS with a trailing `[*]` (`$.workspace.members[*]`) over a STATIC array
  path (no extra wildcard / recursive-descent / filter — that would target the wrong array). The
  fixer strips the `[*]` to locate the array. Multiple targets register the same member in several
  manifests.
- **`fix.create_and_register.content` / `content_from`**: the bytes for a *missing* member;
  Phase 2 (create). Absent (or a glob source) → register-only.

## 4. The check (`check_registered`)

Per **member** (each glob match, or the single named source):

1. **Existence** (named source only): the file does not exist → a violation (`create`-repairable
   iff `content`/`content_from` is set; otherwise reported, not fixable).
2. **Registration**: `register_as(member)` is not an element of the target list at `path` → a
   violation (append-repairable).

Emits **one violation PER (target, missing member)** — a registration finding keyed
`registered\0member\0<target>\0<member>`, or an existence finding (named source) keyed
`registered\0exists\0<path>`. A per-MEMBER key (not a per-target list of all missing) is
deliberate: it keeps each member's baseline fingerprint STABLE, so registering or adding one
member never un-grandfathers the others under `--baseline` (audit A#4), AND the key doubles as the
fix channel — the fixer reads back the single member and appends exactly it (no re-glob, so it
never diverges from the check's gitignore-aware member set). Every finding on a path is uniquely
keyed (the value-prop F4 rule, before a rule becomes fixable). Comparison is VERBATIM (`normalize`
is rejected on `registered`, so the fixer registers the member's real path, not a normalized one —
audit A#3). The check skips a member already present (idempotence / convergence), fires on a
zero-match glob source (audit A#7), normalizes a named `source.file` path before the existence
check (audit A#6), and respects confinement on every path read.

## 5. The fix (`CreateAndRegisterFixer`)

Per violation, stage only the unmet repair:

- **create**: the member file is absent and `content`/`content_from` is set → a `CreateFile`
  edit (bytes from the fix spec; `content_from` read at apply time, confined, non-regular-file
  guarded — the `read_for_fix` FIFO lesson). Absent content on a missing named member → an honest
  `Skipped` ("no content to create <member>"), never a false apply.
- **register**: `register_as(member)` is not in the target list → a **list-append** into the
  structured array at `path` (see 6).

`fix_edit` (editor form) mirrors it. Tier: **Unsafe by default** (creates a file / mutates a
manifest), Safe-promotable; **content-injecting** in the W2 partition (a create writes
ruleset-authored bytes, and the register value is ruleset-chosen — an untrusted remote demotes it
to a suggestion). Confinement: both the created path and every manifest path route through
`confine_fix_path` (the dir_create C1 lesson — every config path).

## 6. The list-append structured operation

A new `StructuredOp::Append(value)` alongside `Set`/`Remove`/`Replace`
(`crates/alint-rules/src/fixers/structured.rs`), resolved per format in
`crates/alint-core/src/structured_fix/mod.rs`: locate the array node at the JSONPath, compute the
byte-range insertion point for a new **last element**, and splice the serialized element preserving
the file's existing style (indentation, trailing comma/newline, inline vs multi-line array). It is
idempotent by construction — the check only fires (and the append only runs) when the value is
absent — and the fixer re-verifies the value is present after the splice (the located-regex
re-extract lesson: a located edit with no structured PutGet verifier compares after the splice).
Structured arrays first (TOML/JSON/YAML); a line-based manifest (`CODEOWNERS`, a barrel file) is a
fast-follow via a line-`insert` op, not this structured path.

## 7. The multi-file transaction (5.2.4) — Phase 2 only

When a create AND a register both fire (a missing named member), the fix spans two files with an
inter-edit dependency (the created member must resolve as a workspace member). Per auto-fix.md
5.2.4: **stage** every file's new bytes in memory, **verify** the cross-file postcondition against
the *staged buffers* (the created member now resolves in the staged manifest), and only then
**write** — all-or-nothing *through the verify phase* (any collect/verify failure discards the
whole group, writes nothing). The write phase is honest about its limit: writes are best-effort
per-file (write all temps first, then rename, since a rename failure is far rarer), and a failure
mid-write reports a **loud partial apply** with the changed-file list rather than claiming a
transactionality the filesystem does not provide. A **test seam** — an injectable writer on the
engine's located-edit write step, behind a `test-hooks` cargo feature (testkit enables it as a
dev-dep feature) — makes "mid-batch verify failure writes nothing" and "a real per-file write
failure reports a loud partial apply" testable (auto-fix-implementation-plan.md 8).

## 8. Phasing

Per [[feedback_phased-rollouts]], one commit per phase with a forward `Next:` pointer.

- **Phase 0 (this doc + the completion-plan entry).** Design record.
- **Phase 1 — check + register-only fix (single-file; the high-value common case).** The
  `registered` relation + `check_registered`; the `StructuredOp::Append` list-append; a
  `CreateAndRegisterFixer` that registers an **existing** member (no create yet — a missing named
  member is reported, not created). Full new-fix-op gate cascade (`FixSpec::CreateAndRegister` +
  op_name + `ALL_OP_NAMES` + cases; the `cross_file` build arm; W2 content-injecting partition;
  property net + a planted trigger; fix-coverage e2e; facts 21→22; README; CHANGELOG). No
  transaction infra. Ships "register an unregistered member".
- **Phase 2 — create-if-missing + the multi-file transaction.** The conditional `CreateFile` half;
  the 5.2.4 stage/verify-against-staged/all-or-nothing write group; the injectable-writer
  `test-hooks` seam; the fault-injection tests. Ships "create a missing member and register it".

Each phase ends with the full preflight (fmt, workspace test, clippy `-D`, rustdoc `-D`,
byte-scan, dogfood) + an independent adversarial audit round ([[feedback_repeated-adversarial-audit-rounds]]).

## 9. Open sub-decisions (defaulted here; revisit if needed)

- `register_as` templating defaults to `{path}` (documented above). A source that names a file
  uses `{dir}`.
- Structured arrays first; line-based manifests (`CODEOWNERS`) deferred to a line-`insert` op.
- Build validation: `content`/`content_from` on a **glob** source is inert (a glob can't name a
  missing file) → rejected at load with a clear message ("a create needs a `file:` source").
