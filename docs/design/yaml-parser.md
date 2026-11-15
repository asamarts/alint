# YAML parser replacement

Status: Accepted and implemented (2026-10-19).
Tracking: [#334](https://github.com/asamarts/alint/issues/334).

## Decision

Use [`yaml_serde` 0.10.7](https://crates.io/crates/yaml_serde), the maintained
`serde_yaml` fork owned by the [YAML organization](https://github.com/yaml/yaml-serde), with its
[`libyaml-rs` 0.3.0](https://github.com/yaml/libyaml-rs) parser. Depend on it and name it directly;
do not retain a `serde_yaml_ng` compatibility alias.

This removes `unsafe-libyaml` and `RUSTSEC-2024-0449` while preserving the parser and value-model
semantics on which alint depends. It does **not** eliminate unsafe Rust: `libyaml-rs` is the YAML
organization's maintained fork of the same C2Rust libyaml translation. That is an explicit tradeoff.
The immediate defect is an archived parser that cannot receive fixes; changing parser semantics and
eliminating unsafe code at the same time would make this security update substantially riskier.

## Required behavior and call-site inventory

The dependency is not just a config decoder. The supported contract spans these paths:

| Surface | Production owners | Required parser behavior |
|---|---|---|
| Local, nested, drop-in, bundled, and remote configuration | `alint-dsl::{lib,loader,nested}` | Serde typed decode, ordered `Value`/`Mapping`, `from_value`, anchors/aliases, tags, scalar typing, source locations |
| Composition, interpolation, templates, and trust provenance | `alint-dsl::{interp,trust}`; `alint-core::{config,template}` | Mutable generic values, tagged values, stable map insertion/removal, typed decode after mutation |
| Structured YAML query and extraction | `alint-core::{structured_format,extract}` | Direct streaming `Deserializer`, borrowed scalar spans, custom tags, aliases, merge-key adaptation, duplicate-key behavior |
| Resource guards | `alint-core::yaml_depth` | Event replay through Serde, 128-level recursion boundary, scanner token parity, bounded alias materialization |
| Policy tests and tree specs | `alint-testkit::{policy_test,policy_runner,treespec}` | Generic-value validation, typed decode, YAML serialization, user-facing error locations |
| CLI suggestion/export and generated tooling | `alint::{suggest,export_agents_md}`; `xtask` | Generic-value inspection and deterministic serialization |
| Rule/config tests and repository audits | `alint-rules`, `alint-e2e` | The same public scalar, mapping, error, and serialization behavior as production |

The committed compatibility contract in
`crates/alint-core/tests/yaml_parser_contract.rs` pins scalar resolution, custom-tag retention,
literal merge-key representation, alias materialization, duplicate-key rejection, single-document
entry points, and malformed-input locations. Existing focused suites additionally pin:

- custom-tag stripping and YAML 1.1 merge application for structured queries;
- quoted versus plain merge keys;
- the exact 128-level accepted/rejected recursion boundary;
- flow scanner parity across comments, quoted/plain/block scalars, Unicode line breaks, and BOMs;
- byte-weighted alias expansion limits, including large anchored scalars;
- literal `requires.alint` rejection for aliases, anchors, tags, merges, and interpolation;
- end-to-end local, remote, bundled, nested, drop-in, structured-query, extraction, fixer, editor/LSP,
  policy-test, and 578-document scenario behavior.

## Candidate evaluation

Repository facts and candidate metadata were checked through 2026-10-21. MSRV compatibility is
against alint's Rust 1.88 floor.

| Candidate | Maintenance / license / MSRV | Compatibility and security result | Decision |
|---|---|---|---|
| `yaml_serde` 0.10.7 + `libyaml-rs` 0.3.0 | YAML organization; MIT OR Apache-2.0 + MIT; Rust 1.82 + 1.60; `yaml_serde` released seven 0.10.x versions from January-August 2026 and `libyaml-rs` 0.3.0 in March | Source-compatible `Value`, `Mapping`, Serde, streaming, tags, diagnostics, and libyaml semantics; removes the archived crate/advisory; parser still contains maintained C2Rust unsafe code | **Selected** |
| `noyalib-serde-yaml` / `noyalib` 0.0.57 | Active, MIT OR Apache-2.0, `#![forbid(unsafe_code)]`; eight releases from September-October 2026; 0.0.57 compiles on Rust 1.88, while 0.0.58 already requires 1.89; `RUSTSEC-2026-0333` fixed since 0.0.53 | Compatibility shim compiled in isolation, but representative parsing was 2.2x slower and its default alias ratio rejected a currently supported one-anchor/11-alias shape as `repetition limit exceeded`; pre-1.0 patch releases are intentionally breaking, requiring an exact pin | Rejected for this migration |
| `serde-saphyr` 1.0.0 | Active, MIT OR Apache-2.0, unsafe forbidden; 1.0 released in July 2026 and compiles on Rust 1.88; 2.0 followed in October and requires Rust 1.89 | Direct typed Serde and budgets are strong, but it has no `serde_yaml`-compatible `Value`/`Mapping`/`from_value` surface. Its default depth budget rejected level 65, below alint's supported level 128, and the representative parse was 1.7x slower | Rejected for current compatibility |
| `saphyr` / `saphyr-parser` 0.2.2 and `yaml-rust2` 0.13 | Active pure-Rust YAML 1.2 parser/value families; MIT OR Apache-2.0; Rust 1.85; `saphyr-parser` released 0.1 in September and 0.2.2 in October 2026 | Good event and spanned trees, but no drop-in Serde/value/error contract. Alint pins both Saphyr crates to 0.2.2 for source spans and duplicate-aware events; replacing the data-binding parser would require owning a second Serde deserializer and semantic adapter | Keep for span/event use only |
| `yaml_serde` alternatives backed by renamed libyaml forks | Mixed | Either retained the same unsafe implementation with less institutional ownership, were deprecated, carried advisories, or exceeded the MSRV | Rejected |
| In-house full parser or documented subset | Owned, no external parser dependency | Full compatibility means scanner, parser, aliases, merges, tags, Serde, generic values, spans, diagnostics, serialization, resource limits, conformance suite, differential fuzzing, and long-term maintenance. A subset would reject accepted user YAML and is a compatibility change, not a dependency refactor | Not competitive; do not build now |

The selected Serde/value API has no source-span interface; alint therefore retains its pinned
Saphyr 0.2.2 event adapter for YAML edits and duplicate-aware extraction. The RustSec database
reported no advisory for `yaml_serde`, `libyaml-rs`, `saphyr`, or `saphyr-parser` at evaluation
time. The comparison explicitly treated an archived or advised transitive parser as disqualifying
rather than something to waive. (Both Saphyr crates later moved together to 0.3.3, in November
2026, with the event adapter and the compatibility contract unchanged.)

The in-house assessment reused the existing `yaml_depth` lexer and the Saphyr-backed span/event paths
as the smallest realistic prototype boundary. They demonstrate that alint can own narrow lexical
security checks and edits. They do not supply typed Serde decoding, the mutable tagged value model,
serialization, or compatible diagnostics. Building those layers would duplicate mature parser work
without reducing this migration's risk.

## Prototype measurements

A throwaway release-mode harness parsed into `serde_json::Value`. Time cells are the median of seven
runs after warmup. Peak heap is Valgrind Massif's useful heap plus allocator overhead across ten
sequential parses; stack memory is excluded. The representative input was the 20,770-byte bundled
`toolchain-pins.yml`; the flow input was a valid 128-level, 258-byte sequence; the alias input was a
4,043-byte mapping with one anchor and 1,000 aliases.

| Candidate | Representative (ns/parse) | Flow depth 128 (ns/parse) | 1,000 aliases (ns/parse) | Representative peak heap (bytes) |
|---|---:|---:|---:|---:|
| `serde_yaml_ng` 0.10.0 baseline | 491,862 | 54,406 | 1,056,968 | 454,944 |
| `yaml_serde` 0.10.7 | 488,903 | 59,238 | 1,044,641 | 454,944 |
| `noyalib` compatibility shim 0.0.57 | 1,105,158 | 81,891 | rejected at alias 11 | 402,000 |
| `serde-saphyr` 1.0.0 | 819,472 | rejected at depth 65 | not measured after compatibility failure | 181,416 |

`yaml_serde` was within 1% of baseline on the representative and alias workloads, 8.9% slower on
the tiny depth-stress input, and byte-for-byte equal at the measured heap peak. The depth case is
not a normal workload and remains behind alint's cheap pre-scan. No material performance or memory
regression was observed.

## Security and resource invariants

The parser swap must not become a reason to remove defense-in-depth. These limits remain owned by
alint and run before materialization:

- 32 MiB structured-document input cap;
- 128 flow-collection depth cap, matching the deserializer recursion boundary;
- byte-weighted alias expansion allowance capped independently of parser dereference counting;
- literal compatibility metadata checks through the Saphyr event stream;
- depth/event checks for duplicate-aware YAML extraction;
- extends-chain depth, memoization, remote-size, and source-confinement limits.

The pre-scanner continues to mirror libyaml token boundaries. `libyaml-rs` is a direct maintained
fork, and the compatibility/property tests confirm parity. A future pure-Rust parser must replace
that lexer coupling deliberately rather than silently assuming equivalent tokens.

The committed `proptest` differential corpus generates hostile scanner prefixes, separators, and
repeated BOMs, then requires the guard to catch every deep-flow or alias shape accepted by the real
parser. Fixed regressions cover block scalars, comments, document markers, Unicode breaks, tags,
large anchored scalars, and merge aliases. That property boundary, not the third-party parser
internals, is alint's fuzzing responsibility for this like-for-like fork migration. A parser-family
change must add differential parser fuzzing before cutover.

## Differential harness

`crates/alint-core/src/yaml_differential.rs` (#482) compares ayaml with `yaml_serde` 0.10.7, the
parser v0.22 used, which stays as the oracle: a dev-dependency of `alint-core` and of no published
crate.

- **Entry points.** Both parsers decode each input three ways: into a value tree, as every option
  and `extends:` source is; into the `serde_json` tree that the structured-format rules query,
  through alint's own seed, which applies merge keys and drops custom tags; and into `Config`,
  only when the value trees agree, so that a typed difference is about the decode. alint reads no
  ayaml events in v0.23, so the event stream joins the harness when saphyr leaves in v0.24.
- **Inputs.** Seeded token soup; generated documents in block and flow style, with anchors,
  aliases, tags, comments, block scalars and directives; and mutations of them: a line dropped,
  repeated, re-indented, cut or swapped, or a token inserted. Then the repository's committed
  YAML files, read with LF line breaks, and mutations of each, seeded per file so that a new file
  changes no other input. Half the generated inputs hold NEL, LS, PS or a byte-order mark.
- **Classes.** Each pair of results is one of six classes: the same value, different values, both
  rejected at the same line and column, both rejected at different places, rejected by ayaml
  only, and rejected by the oracle only. A panic in either parser fails the run.
- **The allowlist.** `yaml_differential/allowlist.rs` lists each documented difference with the
  inputs and classes it covers, both behaviors, the reason and an example; a test checks that
  each example is explained by its own entry. Location differences are listed by ayaml's error
  kind and message, with where each parser reports. A test checks that every syntax message of
  the pinned ayaml has a location class, so a new message fails until its place is documented.
  Any other difference fails the run.
- **Runs.** The bounded run, 6,000 generated inputs plus the corpus and one mutation per file, is
  an ordinary test, about 7 seconds in a debug build, so every pull request runs it. The long
  run, 300,000 generated inputs and 20 mutations per file, about 30 seconds in a release build,
  runs in `.github/workflows/yaml-differential.yml` on pull requests that touch YAML parsing or
  the lockfile, weekly with a new seed, and on dispatch. It writes a Markdown report with the
  class counts, the entry that explained each difference, and the allowlist.
- **Another ayaml.** To qualify ayaml's main branch, point a `[patch.crates-io]` entry at it and
  run the long run.

The documented differences fall into five groups; the report lists every entry.
- **YAML 1.2 where libyaml follows YAML 1.1.** NEL, LS and PS are content; tabs separate; empty
  keys, wider anchor names, `#` in a tag, a later `%YAML` minor version, reserved directives,
  top-level block scalar content in column 1 and the 1.2 flow forms are accepted; the
  non-specific `!` makes a string; global tags stay on their nodes.
- **ayaml is as strict as YAML 1.2.** A comment needs white space before it, continuation lines
  and block scalars must be indented past their parent, a lone `-` or an unseparated `?` or `:`
  in a flow collection is rejected, and so is a tag handle with characters other than letters,
  digits and `-`.
- **libyaml and `yaml_serde` defects.** A byte-order mark that starts a line counts as a column,
  an alias to a redefined anchor resolves to the wrong node, and a value has no integer beyond 64
  bits.
- **ayaml defects in malformed documents.** A block scalar header under its parent's indentation
  after an anchor is accepted (asamarts/ayaml#132), and an error after a leading byte-order mark
  has no location (asamarts/ayaml#133).
- **Error locations.** ayaml checks a stream's syntax before it builds values, so a later syntax
  error wins over an earlier value error; the oracle reports where its look-ahead stopped, and
  once the first document ends, `yaml_serde` reports "more than one document" with no place.

## Migration and rollback

1. Replace the workspace dependency and lockfile entry.
2. Rename every Rust/Cargo callsite to `yaml_serde`; retain no compatibility alias.
3. Keep all guards and parser adapters intact; update implementation comments to name the new fork.
4. Add the parser-independent compatibility contract and run the full adversarial/corpus matrix.
5. Require `cargo tree -i unsafe-libyaml` to report no package and both advisory gates to pass with
   no YAML waiver.

Rollback is one commit because no persisted format changes. Reverting restores the old dependency
and names; user YAML and generated artifacts do not require migration.

## Re-evaluation triggers

Revisit a pure-Rust cutover when one candidate simultaneously provides:

- Rust 1.88-or-lower support;
- stable Serde plus mutable tagged `Value`/`Mapping` and `from_value` APIs;
- configurable limits that can express alint's accepted depth/alias contract;
- differential compatibility on the committed corpus and error locations;
- comparable representative performance;
- a non-pre-1.0 compatibility policy or sufficient adoption/ownership to justify pinning.

Until then, maintaining alint's narrow lexical/security adapters while using the YAML organization's
maintained libyaml fork is the lower-risk ownership boundary.

## Re-evaluation log

- **2026-11-08: ayaml 0.1.1 and its unreleased main, not adopted yet.** ayaml is a pure-Rust YAML
  1.2.2 parser with a libyaml compatibility mode and default limits equal to alint's constants. A
  cutover rebased onto main passes the build, MSRV, deny, audit and the e2e and corpus suites. It
  removes `libyaml-rs` and `yaml_serde` from the graph, and all 868 committed YAML files parse to the
  same value on ayaml main. It fails these triggers:
  - `from_value` accepts wrong-typed scalars, so `pattern: null` loads as `/null/`;
  - valid YAML is rejected, and error locations differ;
  - compatibility entry points take no limits;
  - small configurations cost 5% to 9% more instructions;
  - releases are pre-1.0 and unattested, with exact `serde`/`indexmap` pins;
  - there is no differential fuzzing yet.

  The re-qualification targets ayaml 0.1.2 and the first minor after it
  ([ADR-0031](../adr/0031-v0221-owner-decisions.md), [`v0.22.1/README.md`](v0.22.1/README.md)
  section 4, #481-#484).
- **2026-11-14: ayaml 0.1.2, on `main` ahead of qualification.** #510 moved alint from
  `yaml_serde` 0.10.7 to ayaml 0.1.2; `yaml_serde` and `libyaml-rs` left the graph, and saphyr
  stays for source spans and structured fixes. The owner kept it and moved the qualification
  (#482, #483) before v0.23.0 ([ADR-0035](../adr/0035-keep-the-early-ayaml-cutover.md)).
  - ayaml's default caps on events, collection entries, anchors and alias uses refused valid
    files that `yaml_serde` read. alint decodes through `alint_core::yaml`, which lifts those
    caps and keeps 32 MiB input, depth 128 and the alias-expansion allowance.
  - Against v0.22.0: the same values on every probe; plain scalars containing U+0085, U+2028 or
    U+2029 parse; a block scalar whose header sits at its parent `-` or `?` indentation is
    rejected; several parse errors are worded differently.
  - Criteria: Y-C1 and Y-C2 are met, Y-C6 and Y-C7 are not, and Y-C3 needs the harness.
- **2026-11-14: gaps filed as ayaml issues; saphyr leaves in v0.24.** With the ayaml
  maintainers' agreement, gaps are now filed as issues on asamarts/ayaml, in generic text that
  names no consumer ([ADR-0036](../adr/0036-ayaml-issues-and-the-saphyr-exit.md), amending
  ADR-0031 decision 3).
  - Filed on asamarts/ayaml:
    - asamarts/ayaml#123: plain-scalar spans and `raw` include trailing whitespace;
    - asamarts/ayaml#124: the default count limits refuse valid documents of 0.3 to 7 MB;
    - asamarts/ayaml#125: the compatibility mode has no lazy event stream;
    - asamarts/ayaml#126: document that an alias to an undefined anchor is reported without an
      error;
    - asamarts/ayaml#127: about twice the bytes allocated per read;
    - asamarts/ayaml#128: a spanned tree with path lookup.
  - saphyr and saphyr-parser, which still give source spans, structured fixes, `yaml_depth`'s
    literal-subtree check and `pinned_references`' scan, move to ayaml's events in v0.24, after
    #482 and #483. The move depends on asamarts/ayaml#123, or on alint trimming the spans itself,
    and benefits from asamarts/ayaml#128.
- **2026-11-14: the parser leaves alint's public API.** #516 replaced the parser's types in the
  public API of `alint-core`, `alint-dsl` and `alint-testkit` with alint's own
  ([ADR-0038](../adr/0038-owned-yaml-types-and-the-api-gate.md)), and
  `ci/scripts/public-api-parser-gate.sh` fails on a public item that names a parser crate. A
  later change of parser, including the saphyr exit, is no longer a breaking change for library
  users. The review after phase 4 (#520) widened the gate to every feature, hidden item and
  exported macro, and gave the owned types the conversions, indexing and iteration that the
  `yaml_serde` types had, apart from non-string mapping lookups, the serde `Deserializer` on
  `Value` and the byte offset of an error.
- **2026-11-14: the differential harness (#482).** The harness that the "Differential harness"
  section describes compared ayaml 0.1.2 with `yaml_serde` 0.10.7.
  - **The corpus.** Of the 904 committed YAML files, 903 decode to the same value and JSON tree,
    and both parsers reject the other at the same place. 254 decode to the same `Config`, and both
    refuse the other 649 at the same place. There is no difference.
  - **Generated and mutated input.** Over 24 seeds, 7.66 million inputs (300,000 generated per
    seed, the corpus, and 20 mutations of each file) left no unexplained difference and no panic.
    At the value entry point, 49.1% decode to the same value, 5.1% to different values, 19.0% are
    rejected at the same place, 21.3% at different places, 0.3% by ayaml only and 5.3% by the
    oracle only.
  - **#482's reference figures.** Without NEL, LS, PS or a byte-order mark, of 300,904 inputs:
    162,034 decode to the same value; 5,869 to different values, 5,445 of them because of global
    tags, which #482's ad hoc generator rarely wrote; 61,937 are rejected at the same place and
    63,456 at different places; 118 by ayaml only and 7,490 by the oracle only.
  - **Y-C3.** The corpus has no difference, and every difference elsewhere is documented in
    the allowlist (23 deviations and 5 location classes); approving it is phase 9's decision
    (ADR-0035). The harness found asamarts/ayaml#132 and asamarts/ayaml#133.
- **2026-11-15: the owned types, corrected.** The 2026-11-14 entry on the public API overstated
  how close alint's YAML types came to those of `yaml_serde` 0.10.7: code that compiled against
  0.22 can fail now. The review of #520 added `Value::get_mut`, indexing by `&String`, index
  assignment on `Mapping` (which adds a missing key), `Mapping::values_mut`, the `MappingKeys`,
  `MappingValues` and `MappingValuesMut` iterators, and `Clone` and `FusedIterator` on the
  iterators; assigning a key on a tagged null now makes it a mapping that keeps its tag. What
  the `yaml_serde` types did that alint's still do not, which the CHANGELOG lists for library
  users as well:
  - `Value` is not an enum, so its variants cannot be matched or built, and there is no
    `Number`, `Tag` or `TaggedValue`, and no `as_null`, `is_i64`, `is_u64`, `is_f64`, `untag`
    or `apply_merge`. A tagged value comes only from a document, and `tag()` gives its tag as a
    string.
  - `Value::get` and `get_mut`, and `Mapping`'s lookups and indexing, take a string key only;
    indexing takes `&str`, `&String` or an integer, not a `Value` or an owned `String`.
  - `mapping["missing"]` is null where `yaml_serde` panicked, and `mapping[key] = value` adds a
    missing key where it panicked. `Mapping::remove` keeps the other keys' order where
    `yaml_serde`'s moved the last key into the gap.
  - `Mapping` has no `entry`, `retain`, `clear`, capacity methods, `swap_remove`,
    `shift_remove`, `*_entry` removals, `into_keys` or `into_values`, and no order.
  - `Sequence` is not a `Vec<Value>`: it has `len`, `is_empty`, `get`, `get_mut`, `push`, `pop`,
    iteration and indexing by position, and no other `Vec` or slice method (`first`, slicing).
  - `&value == 1` does not compile, values have no order, `Value::from` takes no `Vec` or slice
    of another type and no `Cow<str>`, and an iterator collects into a `Sequence`, not a
    `Value`.
  - `Value` is not a serde `Deserializer`, its `{:?}` is ayaml's, and an `Error` has no byte
    offset.

  The public-API gate now also fails on what the documentation cannot show: a `hidden` left in
  any `doc(...)` list, an attribute a macro builds, a `cfg` on `doc` at any depth, a public item
  under a platform, profile or `not(feature)` cfg, an `include!` or `#[path]` outside the
  crate, and a crate without `#![deny(unnameable_types)]`. It reads literals and comments as
  Rust does, and checks itself on every run with a hidden, feature-gated canary that the scan
  must find.
