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
large anchored scalars, and merge aliases. That property boundary—not the third-party parser
internals—is alint's fuzzing responsibility for this like-for-like fork migration. A parser-family
change must add differential parser fuzzing before cutover.

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
