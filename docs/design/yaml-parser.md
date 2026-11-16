# YAML parser replacement

Status: Accepted and implemented (2026-10-19). Superseded by ayaml 0.1.2 (#510,
[ADR-0035](../adr/0035-keep-the-early-ayaml-cutover.md)): the decision below is the one of
2026-10-19, and the re-evaluation log records the move and its qualification.
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
- 128 flow-collection depth cap, the parser's own depth limit;
- byte-weighted alias expansion allowance capped independently of parser dereference counting;
- literal compatibility metadata checks through the Saphyr event stream;
- depth/event checks for duplicate-aware YAML extraction;
- extends-chain depth, memoization, remote-size, and source-confinement limits.

ayaml enforces the input cap, a depth cap and an alias budget itself, in time linear in the input,
and `alint_core::yaml` sets those limits from the guards' constants; a test pins every limit and
ayaml's defaults (#483). The guards and the parser do not refuse the same documents:

- ayaml's depth cap counts block levels and the height of what an alias replays as well as flow
  levels, where the flow guard counts flow levels only, so ayaml refuses documents the guard
  passes;
- ayaml charges its alias budget only for what each alias replays, where the guard's counting pass
  charges every node of the expanded tree, the document's own nodes too. The guard is deliberately
  the stricter of the two on aliases, and refuses some documents ayaml decodes, such as a large
  document with one small alias;
- the lexer counts more than ayaml nests in a few places, each of which can only refuse more: after
  a name ayaml reads two ways, and after the byte-order mark that starts the text (see the lexer
  rules below). `MAX_YAML_FLOW_DEPTH` lists the valid documents this refuses.

The guards stay as defense in depth (decided in #483): they refuse a bomb before any parser reads
it, with alint's own error, whichever parser reads the text next. saphyr, which reads YAML for
source spans and structured fixes, refuses a 256th level of flow nesting of its own (its flow level
is a `u8`), but does not bound block nesting. The cost is one linear pass over text that holds a
`[`, `{` or `*`, and a second parse of a document that uses an alias.

The pre-scanner's lexer began as a port of libyaml-rs 0.3.0's scanner, and #483 moved it to the
token boundaries of ayaml's compatibility reader:

- NEL, LS and PS are content, not line breaks;
- in a flow collection, a `?` is the key indicator, and a `:` is the value indicator before a
  blank, `,`, `]` or `}`, or right after a quoted scalar or a closed collection. ayaml refuses a
  `?` right before a flow indicator (`[?[a]]`) and a `:` right before `[` or `{` (`[a:[b]]`), and
  the lexer reads the indicator in both places, which counts the collection the parser stops at.
  A comment right after a flow `?` ends at a NEL too, as ayaml reads it (asamarts/ayaml#141);
- an implicit key's `:` comes at most 1024 characters after the key starts, as ayaml counts it,
  in characters rather than bytes; ayaml refuses a longer key;
- a tag runs over ayaml's characters: a verbatim tag over URI characters, a shorthand over tag
  characters and the `!` that ends a handle. Both hold a `#`, so `!a# [b]` is a tag and a
  sequence, not a tag and a comment;
- an anchor or alias name runs to a blank or a flow indicator, with the compatibility reader's
  stops at `?` and `:`. Where the name stops at a `:` before a flow indicator, ayaml's line reader
  reads on (asamarts/ayaml#139) and lets later lines continue a value at any indentation; the
  lexer does not follow that reading, but counts every `[`, `{` and `*` after the name. An alias
  name that ends in `:` keeps it only when an anchor before it in its document has that whole
  name: ayaml forgets those names at `---` and `...`, and so does the lexer;
- a byte-order mark that starts a line is read three ways: with a column, as libyaml reads it and
  ayaml reads one on a later line or before `---`; without a width, as saphyr reads the text alint
  strips; and as ayaml reads a text that starts with one, the first mark without a width and a
  second right after it as content (asamarts/ayaml#140). ayaml reads a `...` after the first mark
  as text when more text follows it on its line, as libyaml does; that reading counts what
  follows, which can only refuse more.

Every token the lexer reads consumes input, so one pass over the text ends in linear time: a debug
assertion checks it after each token, and a release build would step over a byte rather than read
it again. A later change of parser must move the lexer the same way, against the property below.

The committed `proptest` differential corpus generates hostile scanner prefixes, separators, and
repeated BOMs, then requires the guard to catch every deep-flow shape that ayaml nests and every
alias it resolves. Fixed regressions cover block scalars, comments, document markers, Unicode
breaks, tags, anchor names, flow indicators, large anchored scalars, and merge aliases. The
differential harness below compares the parser itself with the one v0.22 used.

## Differential harness

`crates/alint-core/src/yaml_differential.rs` (#482) compares ayaml with `yaml_serde` 0.10.7, the
parser v0.22 used, which stays as the oracle. It is a dependency of `alint-core` only behind the
test-only `yaml-reference` feature, and no runtime dependency of any published crate (ADR-0041,
superseding ADR-0039); the parser facade below uses it as its libyaml backend.

- **Entry points.** Both parsers decode each input four ways: into a value tree, as every option
  and `extends:` source is; into the `serde_json` tree that the structured-format rules query,
  through alint's own seed, which applies merge keys and drops custom tags; into `Config`; and,
  rule by rule, each entry of `rules:` from its value tree into `RuleSpec` with `from_value`, as
  alint's loader decodes rules, each rule a case of its own, so that a refused rule hides none
  after it. The two typed decodes run only when the value trees agree, so that a typed
  difference is about the decode. alint reads no ayaml events in v0.23, so the event stream
  joins the harness when saphyr leaves in v0.24.
- **Inputs.** Seeded token soup; generated documents in block and flow style, with anchors (on
  keys too), aliases (also aliases that fan out), merge keys, tags, comments, escapes, block
  scalars with blank lines, complex keys, tabs, CRLF line breaks, control characters, long
  scalars and keys, nesting past four levels, and directives; generated configurations, with
  `version`, `vars`, options and rules of real kinds; and mutations of documents and
  configurations: a line dropped, repeated, re-indented, cut or swapped, or a token inserted.
  Then the repository's committed YAML files, read with LF line breaks, and mutations of each,
  seeded per file so that a new file changes no other input. By default, every other generated
  input and every other corpus mutation may hold NEL, LS, PS or a byte-order mark; with seed 1,
  20.1% of the generated inputs and 1.9% of the corpus mutations hold one, as a test checks.
  `ALINT_YAML_DIFF_SPECIALS=0` leaves them out.
- **Classes.** Each pair of results is one of six classes: the same value, different values, both
  rejected at the same line and column, both rejected at different places, rejected by ayaml
  only, and rejected by the oracle only. A panic in either parser fails the run.
- **The allowlist.** `yaml_differential/allowlist.rs` lists each documented difference with the
  classes it covers, how it tells its cases, both behaviors, the reason and an example; a test
  checks that each example is explained by its own entry. A narrow entry reads a parser's error,
  which names the cause. An input-wide entry looks for a construct anywhere in the input, such as a
  NEL, a tab, a tag or a wide anchor name, so the construct's presence proves nothing: the harness
  rewrites the input without it and compares again, and the entry explains the case only if the
  rewritten input no longer differs, or differs in a way another entry explains. A rewrite changes
  its construct only: a tab or a sigil in a scalar's text stays, a tab that starts a line in a flow
  collection is another entry's, a line of tabs alone that leads a block scalar, which both parsers
  reject, stays, and so does a tab ayaml rejects, found by reading the input again with each one it
  reported as a space: in a malformed document its scanner may open a flow collection where YAML 1.2
  goes on with a plain scalar, and the tab is then a flow line's. A tab that leads its line, past
  spaces, also goes with the white space after it, which keeps the indentation ayaml reads the line
  with. A character YAML allows only in a quoted scalar goes where ayaml keeps it, in what its
  scanner reads as one, and past its scanner's error, where it reads nothing; the one ayaml rejects
  stays, and one where ayaml's error is about something else, as a line not indented enough, becomes
  `x`, so that the line keeps its content. `!<!>` becomes `!<!t>`, which both read as a tag, so that
  a node keeps its properties, where it starts a node or where ayaml rejects it, as in what YAML 1.2
  reads as the second line of a plain scalar of a malformed document. A deletion never leaves a
  byte-order mark at the stream's start, or a `%`, `---` or `...` at a line's start. The oracle's
  error names only the first character its reader does not allow, so that rewrite also removes the
  first alone. A rewrite also keeps keys apart: a wide integer also becomes a number the input does
  not otherwise hold, and a long key is cut short of every other key. An explanation keeps no entry
  the case does not need: with each of its entries left out in turn, the others do not explain the
  case. A test adds each input-wide entry's construct, in a comment, a string or a command, to a
  difference that entry does not explain, at the entry point of the entry's example, and checks that
  it stays unexplained.
- **A fair rewrite.** A rewrite counts only if it hides nothing the case showed. A parser that
  read the case must read the rewrite, or reject it at the place where the other parser, which
  rejected the case, still gives the same error; an error both give only after the rewrite is
  one the rewrite made. A parser that rejected the case may read the rewrite, keep its message,
  or give another error no earlier than the line of the first; an error about a value or an
  alias may come earlier, since a parser reports it where its node starts. An error that moves
  to an earlier line shows something the trigger hid, and the search must then reach a rewrite
  both parsers read alike. A rewrite after which one parser keeps its error and the other gives
  the same value error, or an error at the same place, ends the search. ayaml reads a stream's
  syntax before it builds values, so a rewrite that turns one of its value errors into a syntax
  error hides the value error, wherever each is. It also reads the whole stream's tokens before
  it parses them, so a scanner error hides each parser error before it: a rewrite of a tab that
  alone indents a flow line, or of `!<!>`, which ayaml rejects as it scans, may show a parser
  error on an earlier line, which was the case's. One variant of the flow line's rewrite spaces
  only the tabs ayaml rejects, since the oracle may reject a later one, in a plain scalar's
  line, past an error that ayaml's first hides. A test puts three differences, one that both
  parsers read, one that only ayaml rejects and one that only the oracle rejects, beside the
  constructs of the input-wide entries that a value tree holds and the errors of the narrow
  entries, before and after them, and checks at the value and JSON entry points that each stays
  unexplained without its own entry (a wide integer, which no JSON number holds, at the first
  only); another puts the two typed-decode entries' differences in a configuration beside each
  other's construct. The content of a top-level block scalar is the one construct no difference
  can sit beside, since it takes in the lines around it.
- **Locations.** Location differences are listed by ayaml's error kind and message, with where each
  parser reports. A location class takes two errors for one, found at two places, so it explains a
  case only when both errors are about the syntax or neither is: when one parser reads the syntax
  alone (the oracle reads the input into no value) and its error is about a value, the case is
  explained as if only the other parser rejected it, and two such errors at one place are not one
  either. Nor are two errors one when the parser with the later error reads the input through the
  earlier error's line and the construct it is in, and the other rejects it: each is then explained
  on its own, the rest of the input as a case of its own when it reads as it would after that
  prefix. Where it does not, the prefix through the line alone tells: one that holds ayaml's error
  as it is, which the oracle reads, explains ayaml's whole result, and one that holds the oracle's
  error, which ayaml reads, leaves the case to the entries. The oracle's "more than one document",
  which has no place, is not ayaml's error found elsewhere when the oracle read ayaml's error's line
  in its first document, and a line that starts with a `:` holds the value of the `?` key before it,
  which no split separates. The oracle's error about a character its reader does not allow is one
  with ayaml's about the same character, or about a tag that the character ends or a verbatim tag
  that holds it, or about the first C0 control, which ayaml's reader rejects anywhere before its
  scanner rejects a character YAML allows only in a quoted scalar, outside one; an error about
  another character is another error, and no other class takes that error of the oracle's: its
  reader checks the whole of a short input before its scanner reads a token, so the error says
  nothing of the syntax. ayaml's error about a tab that alone indents a flow line is its own
  wherever the oracle's is, since the oracle reads the tab as white space, and so is its error about
  `!<!>`: those entries' rewrites explain them, not a location class. Its error about `!<>`, which
  names nothing and which both reject, has a class of its own. A location class or a narrow entry
  explains the error it covers, not the lines before it, which must read alike or be explained. Nor
  does a location class explain a case that an input-wide entry's rewrite splits, one parser then
  reading the input while the other keeps its error; the rewrites must explain it. Both parsers
  place a rule's error at the rule, so two errors there are one only when they agree on the problem
  and on the field, which for the oracle, whose errors name none, is the one its error comes and
  goes with. A test checks that every syntax message of the pinned ayaml, taken from its scanner and
  parser sources, has a location class, so a new message fails until its place is documented. Any
  other difference fails the run.
- **Runs.** The bounded run, 6,000 generated inputs plus the corpus and one mutation per file, is
  an ordinary test, about 16 seconds in a debug build. CI's test job runs it when a pull request
  or a push changes the crates, the workspace manifests or CI (`ci/scripts/detect-changes.sh`).
  The long run, 300,000 generated inputs and 20 mutations per file, about 50 seconds in a release
  build, runs in `.github/workflows/yaml-differential.yml` on pull requests that touch
  `alint-core`'s sources or manifest, the workspace manifests or any YAML file, weekly with a new
  seed, and on dispatch, each dispatched run in a concurrency group of its own. It writes a
  Markdown report with the class counts, the entries that explained each difference, and the
  allowlist.
- **Another ayaml.** To qualify ayaml's main branch, point a `[patch.crates-io]` entry at it and
  run the long run. Cargo applies the patch only while main's version satisfies the workspace's
  exact pin (`=0.1.2`); once main's version moves past it, change the pin as well.

The documented differences fall into five groups; the report lists every entry.
- **YAML 1.2 where libyaml follows YAML 1.1.** NEL, LS and PS are content; tabs separate. These are
  accepted: empty keys, a stream that starts with `...`, wider anchor names, `#` in a tag, a
  verbatim one too, or a `%TAG` prefix, any `%YAML 1.x` minor version, reserved directives,
  top-level block scalar content in column 1, the 1.2 flow forms (among them `[? ]`, after which
  libyaml drops the `]`, and an empty implicit key, as `[: x]`, which a rewrite to `~` removes), a
  flow mapping key of more than 1,024 characters, an implicit key whose 1,024-character limit counts
  characters rather than bytes, and DEL, the C1 controls, U+FFFE and U+FFFF in a quoted scalar. In a
  flow sequence, a plain scalar that starts with `:` after properties is that scalar, as in `[!!str
  :a]`, which libyaml reads as a mapping. The non-specific `!` makes a string, and global tags stay
  on their nodes. A core tag on another kind of node is ignored, an empty `!!null` node is null, a
  scalar its core tag does not fit is an error, and a typed decode reads a scalar by its core tag.
- **ayaml is as strict as YAML 1.2.** A comment needs white space before it; continuation lines and
  block scalars must be indented past their parent, a block scalar's leading empty lines no further
  than its content, and a flow collection's continuation line under a block collection by spaces; a
  lone `-` or an unseparated `?` or `:` in a flow collection is rejected, and so are a tag handle
  with characters other than letters, digits and `-`, and the verbatim tag `!<!>`.
- **libyaml and `yaml_serde` defects.** A byte-order mark that starts the stream counts as a
  column, an alias to a redefined anchor resolves to the wrong node, a value has no integer
  beyond 64 bits, in any base, a count of alias uses refuses aliases that fan out, `-0` is no
  number for an unsigned field, and `from_value` refuses a rule key that is not a string, which
  alint's loader rejects before it decodes a rule.
- **ayaml defects.** In malformed documents, a block scalar header under its parent's indentation
  after an anchor or after a key with an anchor (asamarts/ayaml#132), or after properties alone on
  their line (asamarts/ayaml#147), is accepted, where the oracle rejects it, or reads it as a second
  document when it is indented less than the root, past any directives and `---`; after a leading
  byte-order mark, an error has no location (asamarts/ayaml#133) and a quoted scalar's continuation
  line under a nested parent is not checked against its indentation (asamarts/ayaml#137). One more
  is in a valid document (asamarts/ayaml#146): a document marker after a top-level block scalar's
  empty lines, one of them indented, is taken for content and rejected, as in `|`, a line of two
  spaces and `...`.
- **Error locations.** ayaml checks a stream's syntax before it builds values, so a later syntax
  error wins over an earlier value error, and reads its tokens before it parses them, so a later
  scanner error wins over an earlier parser error; it reports an error about an empty node where
  what follows starts, and a character YAML does not allow where it stands; the oracle reports where
  its look-ahead stopped, a disallowed character at line 1, column 1, and once the first
  document ends, "more than one document" with no place. Of a rule both refuse, each may name
  another problem: ayaml calls a null rule a unit value, which the oracle finds without `id`,
  and names the variant that a one-entry mapping in an enum's place does not, which the oracle
  refuses as a mapping.

## Parser facade

[ADR-0041](../adr/0041-a-yaml-parser-facade-with-test-only-reference-parsers.md) puts the parser
behind `alint_core::yaml`, so that alint's own suites can run on other parsers while ayaml is
adopted and developed. It supersedes ADR-0039. This section is the design; the log below records
each phase as it lands.

- **Backends.** The two entry points that read text, `alint_core::yaml::from_str` and the JSON
  tree the structured-format rules query, send each parse to a backend
  (`crates/alint-core/src/yaml_backend.rs`):
  - **ayaml**, the default, and the only one without the feature below;
  - **libyaml**, through `yaml_serde` `=0.10.7`, for values, typed decodes and the JSON tree.

  **saphyr**, `=0.3.3`, has no serde support, so it reads value trees in compare mode only.
  `from_value`, `to_value` and `to_string` read no text and stay on ayaml's data model, which
  ADR-0038's owned types hide. The depth guard's pre-parse checks measure what ayaml reads, so
  they parse with ayaml whatever the backend. `Error` holds the error of whichever backend
  failed, behind the same `message` and `location`.
- **The feature.** `alint-core`'s non-default `yaml-reference` feature compiles libyaml, saphyr's
  backend and compare mode, and makes `yaml_serde` an optional dependency. No release build
  enables it. `ci/scripts/yaml-reference-gate.sh` checks that no workspace crate depends on
  `yaml_serde` with default features, on any target, other than as a dev-dependency, and that
  the same query finds it with the feature.
- **Choosing a backend.** With the feature, `ALINT_YAML_PARSER` is read once per process:
  `ayaml` (or unset), `libyaml`, or `compare`. Any other value panics with the accepted ones,
  so a misspelled CI setting cannot fall back to ayaml unnoticed. Without the feature, the
  variable is not read.
- **Compare mode** (`yaml_backend/compare.rs`). ayaml decodes, and its result is what the caller
  gets. The harness also compares the input's value tree, or for the structured-format rules
  its JSON tree, as ayaml and libyaml read it, and libyaml runs the same typed decode. Then:
  - the trees agree, and so does whether the decode succeeds: nothing to explain;
  - the value trees differ, and saphyr reads the tree as ayaml does, or rejects the input as
    ayaml does: a YAML 1.2 reading, explained by the vote;
  - they differ otherwise: the harness's allowlist must explain the case, exactly as in the
    harness;
  - the trees agree but only one typed decode succeeds: a decode into `Config` must be
    explained as the harness's `Config` entry point is; another type's must hold a `!!` tag,
    which the harness's `typed-decode-core-tags` entry explains for `Config`, and is logged
    as unchecked;
  - nothing explains it: the decode panics, with the input, the three results and the class.

  A defect ayaml shares with saphyr passes the vote, so the counts and examples of what the vote
  explains are reviewed on each ayaml bump. With `ALINT_YAML_COMPARE_LOG` set to a file, each
  explained difference appends a JSON line naming the verdict, the entry point, the class and
  the entries, for a report.
- **Where it runs.** A CI job runs `cargo test --workspace --features alint-core/yaml-reference`
  with `ALINT_YAML_PARSER=compare`: the unit tests, the end-to-end tests that run the `alint`
  binary, the policy tests and the dogfood check all read their YAML through the vote. The
  harness's own runs add the vote to their report: per entry, the differences it also explains,
  and the ones only the vote explains, with examples. An entry the vote makes redundant can go
  on review; the first long runs found none, so the allowlist stays the source of the
  CHANGELOG's record of what changed from v0.22 (ADR-0035 decision 5).
- **The dogfood rule.** Beside the rule that fails on a direct ayaml decode outside
  `alint_core::yaml` (ADR-0035 decision 4), `yaml-reference-parsers-stay-behind-the-facade`
  fails on `yaml_serde::` outside the backends and the harness. saphyr's span uses stay until
  they leave in v0.24.
- **Phases.**
  1. The backends, the variable and compare mode with the vote and the allowlist, as a module
     beside `yaml.rs`; the harness's comparison tree and explanation compile with the feature as
     well as in tests.
  2. The harness through the backends, with saphyr's vote in its report, and an allowlist
     entry for each difference compare mode finds that the harness had not.
  3. The CI job, the release gate, the dogfood rule and the documents.

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
  section describes compared ayaml 0.1.2 with `yaml_serde` 0.10.7. The figures are from the
  harness as two review rounds revised it (2026-11-15). The first added the rule decode and
  configuration-shaped input. The second, a review of the first round's fixes, kept one
  difference from masking another (the fair rewrite and the location rules above), made each
  rule a case of its own, and rewrites integers beyond 64 bits in any base, apart from any
  other key. Its long runs found one more difference, which ayaml reads as YAML 1.2 does: a
  plain scalar that starts with `:` after properties in a flow sequence, as `[!!str :a]`,
  which libyaml reads as a mapping.
  - **The corpus.** Of the 905 committed YAML files, 904 decode to the same value and JSON tree,
    and both parsers reject the other at the same place. 254 decode to the same `Config`, and both
    refuse 650 at the same place; the file both reject is not decoded. Of the 3,801 rules in the
    251 files with rules, both decode 3,792 alike and refuse 9 at the same place. There is no
    difference.
  - **Generated and mutated input.** Each run compares 319,005 inputs: 300,000 generated, the
    corpus, and 20 mutations of each file. Seeds 1 to 16 and 101 to 110 with special characters
    (the default) and seeds 1 to 8 and 101 to 108 with `ALINT_YAML_DIFF_SPECIALS=0`, 13.40 million
    inputs in all, left no unexplained difference and no panic. At the value entry point:

    | Runs | Same value | Different values | Same place | Different places | ayaml only | Oracle only |
    |---|---:|---:|---:|---:|---:|---:|
    | Specials, 26 seeds, 8,294,130 inputs | 46.2% | 3.6% | 15.9% | 29.1% | 0.3% | 4.9% |
    | No specials, 16 seeds, 5,104,080 inputs | 50.3% | 1.3% | 17.5% | 28.4% | 0.1% | 2.5% |

    The typed decodes run where the value trees agree. With specials, of 3,830,615 `Config`
    decodes, 4.0% agree, 79.5% are refused at the same place, 15.2% at different places, 0.9% by
    ayaml only and 0.3% by the oracle only; without, of 2,568,190, 3.7%, 81.0%, 14.1%, 0.9% and
    0.3%. Of the rules, each a case of its own, 4,525,658 with specials and 2,815,749 without,
    56.1% and 56.2% agree and the rest are refused at the same place.
  - **The first run, at #521.** Without NEL, LS, PS or a byte-order mark, of 300,904 inputs:
    162,034 decode to the same value; 5,869 to different values, 5,445 of them because of global
    tags, which #482's ad hoc generator rarely wrote; 61,937 are rejected at the same place and
    63,456 at different places; 118 by ayaml only and 7,490 by the oracle only.
  - **Y-C3.** The corpus has no difference, and every difference elsewhere is documented in
    the allowlist (33 deviations and 7 location classes); approving it is phase 9's decision
    (ADR-0035). The harness found asamarts/ayaml#132 and asamarts/ayaml#133. Its first review
    round found asamarts/ayaml#137, a quoted scalar's continuation line under a nested parent
    that goes unchecked after a leading byte-order mark; asamarts/ayaml#138, a tab and then
    spaces on a quoted scalar's continuation line, which both parsers accept; and #132 after a
    key with an anchor, added as a comment there. The second round found no other.
- **2026-11-15: the owned types, corrected.** The 2026-11-14 entry on the public API overstated
  how close alint's YAML types came to those of `yaml_serde` 0.10.7: code that compiled against
  0.22 can fail now. The review of #520 added `Value::get_mut`, indexing by `&String`, index
  assignment on `Mapping` (which adds a missing key), `Mapping::values_mut`, the `MappingKeys`,
  `MappingValues` and `MappingValuesMut` iterators, and `Clone` and `FusedIterator` on the
  iterators; assigning a key on a tagged null now makes it a mapping that keeps its tag. What
  the `yaml_serde` types did that alint's still do not, which the CHANGELOG lists for library
  users as well:
  - `Value` is not an enum, so its variants cannot be matched or built, and there is no
    `Number`, `Tag` or `TaggedValue`, and no `as_null`, `is_i64`, `is_u64`, `is_f64` or
    `apply_merge`. A tagged value comes only from a document, `tag()` gives its tag as a
    string, and the accessors look through it, as `yaml_serde`'s did (its `untag` family is
    private to the crate).
  - `Value::get` and `get_mut`, and `Mapping`'s lookups and indexing, take a string key only,
    which they find untagged only, as `yaml_serde`'s did (`!t k: v` is found by iterating);
    indexing takes `&str`, `&String` or an integer, not a `Value` or an owned `String`.
  - `mapping["missing"]` is null where `yaml_serde` panicked, and `mapping[key] = value` adds a
    missing key where it panicked. `Mapping::remove` keeps the other keys' order where
    `yaml_serde`'s moved the last key into the gap.
  - `Mapping` has no `entry`, `retain`, `clear`, capacity methods, `swap_remove`,
    `shift_remove`, `*_entry` removals, `into_keys` or `into_values`, and no order. The public
    types of `yaml_serde::mapping` that alint's iterators replace are `Iter`, `IterMut`,
    `IntoIter`, `Keys`, `Values` and `ValuesMut`; `Entry`, `OccupiedEntry`, `VacantEntry`,
    `IntoKeys` and `IntoValues` have no counterpart.
  - `Sequence` is not a `Vec<Value>`: it has `len`, `is_empty`, `get`, `get_mut`, `push`, `pop`,
    iteration and indexing by position, and no other `Vec` or slice method (`first`, slicing).
  - `&value == 1` does not compile, values have no order, `Value::from` takes no `Vec` or slice
    of another type and no `Cow<str>`, and an iterator collects into a `Sequence`, not a
    `Value`.
  - Neither `Value` nor `&Value` is a serde `Deserializer`, `Value` is not `IntoDeserializer`,
    its `{:?}` is ayaml's, and an `Error` has no byte offset.

  The public-API gate now also fails on what the documentation cannot show: a `hidden` left in
  any `doc(...)` list, an attribute a macro builds, a `cfg` on `doc` at any depth, a public item
  under a platform, profile or `not(feature)` cfg, an `include!` or `#[path]` outside the
  crate, and a crate without `#![deny(unnameable_types)]`. It reads literals and comments as
  Rust does, and checks itself on every run with a hidden, feature-gated canary that the scan
  must find.
- **2026-11-15: the guards follow ayaml (#483).** The `yaml_depth` lexer reads tokens as ayaml's
  compatibility reader does (see "Security and resource invariants"), and its parity property runs
  against ayaml alone, with the flow half restored and no relaxed assertion. After the fixes
  below, three runs of 2,000,000 cases of each property found nothing.
  - Re-deriving it found five ways the lexer hid a deep flow collection or an alias that ayaml
    reads: NEL, LS or PS taken as a line break; a flow `:` before a quote taken as an indicator; a
    flow `?` before a `#` taken as text, where ayaml reads the `#` as a comment; an anchor or alias
    name holding a quote, `#`, `|`, `>` or `.`; and a byte-order mark before `...`. ayaml bounds
    depth and aliases itself, so none of them let a bomb through, and each now has a fixture.
  - `alint_core::yaml` sets the input, depth and alias limits from the guards' constants, and a
    test pins every limit and ayaml's defaults.
  - The first long run of the property hung rather than fail: in a flow collection a `:` before
    a `?`, as in `[:?` or an alias name that gives back its `:`, started a plain scalar that
    ended before it, again and again. The lexer now reads that `:` as content, as ayaml does
    (`[a:?b]` is one scalar), and asserts that every token consumes input. The lexer v0.22
    shipped read the `:` as an indicator and stepped over it, so no release hangs.
  - Runs of a million cases then found three shapes that ayaml's compatibility reader reads
    inconsistently: an anchor name that ends at a `:` before a flow indicator
    (asamarts/ayaml#139); a second byte-order mark at the start of the text, which it reads as
    content (asamarts/ayaml#140); and a comment right after a flow `?`, which ends at a NEL
    (asamarts/ayaml#141). The lexer reads the second mark and the comment as ayaml does, and
    after such a name counts every bracket and `*` that follows. Each has a fixture.
  - The shims for ayaml 0.1.1 are gone. ayaml 0.1.2 names the field in an error, so
    `requires.alint` no longer appears twice, and the byte-order-mark heuristic, which refused a
    valid document with a BOM, `--- ` and more than 100 `*`, is not needed: the lexer reads each
    BOM reading, and ayaml checks the whole stream's syntax before it replays an alias.
  - Mutation testing found 45 changes to the lexer that no test caught. Seven changed no token:
    end-of-text checks that the next character already decides, and a byte-order-mark case the
    scan never reached; that code is gone. For each of the others, 400,000 sampled inputs showed
    where the change reads other tokens, and ayaml decided which reading is right. The tests now
    hold those inputs, as deep collections the change would hide or valid documents it would
    refuse. One test compares the reading of byte-order marks as ayaml reads them with the values
    ayaml returns, since the union of the readings hides one reading's mistakes. The tests moved
    to `yaml_depth_tests.rs`.
- **2026-11-15: the gate after the review of #523.** The review found code the public-API gate
  passed although rustdoc never documents it, so a parser type in it would reach users on
  another target. The gate now fails on any `cfg` or `cfg_attr` whose predicate is not one on
  features, `test`, `doctest` or `docsrs` (combined with `all` or `any` only), whatever it is on:
  a module, a `const _` block of impls, a variant, a field, a trait item, a statement, or an
  item under `cfg(panic = "abort")`. Test-only code is exempt: an item under such a `cfg`, a file
  only test-only code loads, and the files outside `src/` while the library includes none. The
  cfgs the crates have are listed in the gate's `PLATFORM_ALLOWLIST`, each with why what it hides
  cannot name a parser, and an entry no cfg matches fails, so the list is exactly the code's.
  `cfg_select!` fails, as do a `$` anywhere in an attribute, a module a macro names, a `use` of
  `include` and the word `include` in a macro's input or body (each can reach code the checks
  never read), and an exported macro whose body names a parser crate as a bare word or whose
  name is raw. The scan counts only rustdoc's links to a parser crate's pages, relative or on
  docs.rs, so prose may link to a parser's issues.

  One exception is by design. `ref_cast`'s `RefCastCustom` derive, used on `Value`, `Mapping`
  and `Sequence`, implements its trait `ref_cast::__private<N>::RefCastCustom<ayaml::...>` for
  each, in an anonymous `const _` block; the trait is `doc(hidden)`, so with hidden items shown
  rustdoc still leaves the impls out. A doc(hidden) item under a `__private` path is, by Rust's
  semver conventions, not public API, and nothing outside `ref_cast` can name the trait, so the
  impls are not a way to reach a parser type.
