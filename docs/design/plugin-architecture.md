# Plugin architecture: one contract, three hosts

Status: Accepted direction (2026-10-14), not yet implemented. The decisions are DEC-10 and
DEC-14 in [`general-linter.md`](general-linter.md) section 13.1, recorded in
[ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md). This is an arc
document: each slice in section 10 graduates to its own `docs/design/vX.Y/` design document
(in the [`TEMPLATE.md`](TEMPLATE.md) shape) before its code is written.
Decisions: [ADR-0019](../adr/0019-general-linter-program-and-plugin-architecture.md).
Demand evidence: the shipped `command` kind (the only extension point today), the roadmap's
WASM plugin backlog, the 2026-10-14 review of the general-linter plans, and the prior-art survey
in section 2.

> This document is the detailed design behind [`general-linter.md`](general-linter.md) section
> 9.4. That section states the decisions; this one records the research, the type mapping, the
> wire-identity and evolution scheme, the protocol, the trust model, the effort estimates, the
> work breakdown, and the open questions, so the plugin track can be executed in separate
> sessions without re-deriving any of it.

## Contents

- [1. Goals and non-goals](#1-goals-and-non-goals)
- [2. Prior art](#2-prior-art)
- [3. The contract](#3-the-contract)
- [4. The three hosts](#4-the-three-hosts)
- [5. The out-of-process protocol](#5-the-out-of-process-protocol)
- [6. The contract compiler](#6-the-contract-compiler)
- [7. Type mapping](#7-type-mapping)
- [8. Wire identity and evolution](#8-wire-identity-and-evolution)
- [9. Trust and distribution](#9-trust-and-distribution)
- [10. Delivery plan and estimates](#10-delivery-plan-and-estimates)
- [11. Changes required in alint](#11-changes-required-in-alint)
- [12. Risks](#12-risks)
- [13. Open questions](#13-open-questions)

## 1. Goals and non-goals

Goals:

- Third parties can add **format adapters**, **checkers** (analyzers), and **fixers** without
  changes to alint itself.
- **Rust is the canonical plugin language**, with full speed and type safety.
- **Any other language** can implement plugins through a typed, performant, cross-platform, and
  lightweight interface.
- **Sandboxed plugins** can be shared through remote rulesets safely.
- **One contract** serves all three, so a plugin's capabilities never depend on how it is run,
  and built-in kinds and formats use the same traits (DEC-10, DEC-14a).
- The guarantees of the declarative core survive: determinism, bounded resources, fail-loudly
  outcomes, trust gates, and host-applied, verified edits.

Non-goals:

- Loading Rust dynamic libraries at runtime (section 4.1).
- A network RPC system: plugins are local, per-run processes or components.
- Plugins that write files, spawn processes, or read the network or clock through the sandboxed
  host.
- A public plugin registry before one adapter and one analyzer plugin have been validated end to
  end.

## 2. Prior art

A survey of linter, formatter, and developer-tool plugin systems on 2026-10-14 informed every
choice below. Sources were each project's own documentation and source where available; items
marked "unverified" were not re-checked.

| System | Mechanism | Lesson for alint |
|---|---|---|
| HashiCorp go-plugin (Terraform providers, tflint, Vault, Packer) | subprocess prints a handshake line, then gRPC over a local socket or loopback TCP; host callbacks through a broker | works, but "any language" in practice means Go; sockets and ports are awkward on Windows; its README still lists plugin semver as roadmap |
| tflint | rulesets are go-plugin gRPC servers that call back into a host "runner"; releases ship signed `checksums.txt` (attestation or PGP) | signature verification is valued; its SDK documents frequent breaking changes |
| dprint | sandboxed WASM plugins plus native "process plugins" that must be pinned by checksum; a binary frame protocol over stdio with message ids, cancellation, and host callbacks | the closest precedent: one logical plugin API, two backends, checksum-pinned native code |
| Nushell | plugins over stdio; the plugin chooses JSON or MessagePack; a hello with protocol version and feature flags; persistent plugins with idle collection | the handshake-with-features model; the cost of minor-version lock-step (plugins break on every minor release) |
| protoc plugins | one protobuf request on stdin, one response on stdout | protobuf over stdio is universally implementable |
| Bazel persistent workers | length-prefixed protobuf (or JSON) over stdio; request ids for multiplexing; opt-in cancellation | the persistent-worker lifecycle and framing |
| golangci-lint module plugins, Caddy `xcaddy` | a manifest of plugin modules, then a custom build of the host binary | the native-plugin model without ABI risk; the cost is needing the toolchain |
| golangci-lint Go `.so` plugins | dynamic loading | requires identical toolchain and dependency versions; Linux and macOS only |
| Bevy dynamic plugins | Rust dylib loading | deprecated as unsound: Rust has no stable ABI across compiler versions or builds |
| Envoy | separate contracts for proxy-wasm, external processing over gRPC, a C-ABI module API, and Lua | the cost of not unifying contracts across hosts |
| Zed extensions | Rust compiled to WASM components; one WIT copy and one host adapter per contract version (`since_v0.0.1` onward, about ten copies) | the real cost of WIT's frozen types, and a working side-by-side versioning model |
| Typst | raw WASM, no WASI, pure functions | pure deterministic WASM plugins are viable |
| sqlc, Buf check plugins | one protobuf contract served to both WASM modules and native processes | one schema across sandboxed and native hosts works |
| Terraform plugin protocol | major versions as separate protobuf packages negotiated at handshake; immutable released minors; dependency lock file with per-platform hashes | side-by-side major versions; lock files must hold hashes for every platform or CI breaks |
| wasmCloud and wRPC | WIT as a transport-independent IDL; wasmCloud plans to move its own protobuf API to WIT and calls WIT-to-protobuf the safe direction | WIT as the source of truth is where the component ecosystem is converging |
| SWC WASM plugins | the host's internal AST layout serialized into the plugin ABI | every internal change broke every plugin; only contract types may cross the boundary |
| oxlint, Deno lint, Biome, Ruff | JS plugins (oxlint, Deno), query-language plugins (Biome), no plugins (Ruff) | in-process language runtimes are a different design point; Ruff's refusal is a reminder that plugins are a long-term maintenance commitment |

## 3. The contract

### 3.1 Roles

| Role | Input (pushed by the host) | Output | Built-in equivalent |
|---|---|---|---|
| Adapter | resource bytes, path, detected format, options | typed rows with spans, plus parse diagnostics | each of the eight config formats, CommonMark, later CSV and front matter |
| Analyzer | resources or rows selected by the rule, options, declared facts and vars | findings with primary and related locations and severity | native rule kinds |
| Fixer | the findings it produced, the resource bytes | typed edit proposals with applicability and optional verifier | the 26 shipped fix operations |

Inputs are pushed: in contract `0.x` a plugin cannot call back into the host. Host callbacks
(for example "parse this embedded region with another adapter", as dprint offers) are a later,
explicitly versioned addition (section 13).

### 3.2 Shape

The contract is a WIT package (working name `alint:plugin`) with:

- shared types: spans and ranges, resource identity and metadata, rows and values, findings,
  related locations, edits, applicability, diagnostics, plugin info, configuration documents;
- one world per role (`adapter`, `analyzer`, `fixer`); one artifact may implement several;
- plugin info carrying the plugin name and version, the supported contract range, the kinds and
  formats it provides, an options schema per kind, declared capabilities (none for WASM), and
  requested limits.

Version 1 uses a restricted, pure subset of WIT, enforced by the contract compiler: no resources
or handles, no `future` or `stream`, no `map`, no recursive types, and `char` and tuples avoided
in favour of strings and named records.

### 3.3 Edits stay host-applied

Fixers return edits; they never write. The host:

1. confines every edit path (the existing confinement rules, including `allow_out_of_root`
   only from the top-level config);
2. applies the tier policy (Safe, Unsafe, Suggestion, Never) and the untrusted-remote demotions;
3. composes edits per file, verifies them (re-evaluation and, for structured edits, the
   structured verifier), and writes atomically;
4. reports per-edit outcomes.

This keeps ADR-0017's guarantees identical for built-in and plugin fixes.

## 4. The three hosts

### 4.1 Native Rust: compiled in

- A plugin crate depends on `alint-plugin-api` (the generated, semver-stable types and traits;
  no wasmtime dependency) and exports its kinds, adapters, and fixers through a registration
  macro.
- `alint build` reads a manifest (working shape below), generates a throwaway crate that depends
  on `alint` and the plugin crates and registers them, and runs `cargo build --release`.
- Embedders can do the same in code: run alint with an extended registry.
- Trust: compiling a crate is the grant; native plugins run with full trust.

```toml
# alint-build.toml (working shape; the format is question Q17)
alint = "0.22"
plugins = [
  { crate = "alint-plugin-k8s", version = "1" },
  { git = "https://example.invalid/acme/alint-checks", rev = "<sha>" },
  { path = "./my-checks" },
]
```

Dynamic loading of Rust libraries is rejected: Rust has no stable ABI, Bevy deprecated its
dynamic plugin loader as unsound, and C-ABI module systems (Envoy) are compatible only across a
narrow host version window and push unsafe code into every plugin.

### 4.2 Out-of-process: any language

Plugins are executables speaking the protocol of section 5. They are full-trust native code,
integrity-pinned and spawn-gated (section 9). SDKs: a Rust SDK that runs the same plugin crate
either compiled in or as a process, then generated Python, Go, and TypeScript SDKs. The raw
protocol is documented well enough that a plugin can be written without an SDK.

### 4.3 WASM components: sandboxed

- wasmtime, behind a cargo feature (off in builds that do not want the size cost; the release
  binary's size budget is measured when the host lands).
- Target the WASI 0.2 component ABI first; guest toolchains lag WASI 0.3 and the contract needs
  no async.
- Pure by construction: the host grants no filesystem, network, clock, randomness, or
  environment imports. Fuel or epoch interruption bounds time; a store limiter bounds memory.
- Results are cacheable by (plugin digest, contract version, input digest, options).
- Precompiled artifacts are cached keyed by the wasmtime version and engine configuration;
  because deserializing a precompiled module is unsafe, the cache is integrity-checked and
  host-owned.
- Guest languages: Rust, C, C++, C#, and Go through `wit-bindgen` and its Go generator; Python and
  JavaScript through `componentize-py` and `componentize-js` (8 to 35 MB components); Java has no
  current WIT guest generator. The out-of-process host is therefore the real "any language" path,
  and WASM is the sandboxed, shareable path.
- Multiple contract versions are supported side by side, one adapter per version (the Zed model).

### 4.4 One internal interface

All three hosts implement the internal `Adapter`, `Analyzer`, and `Fixer` traits, which the
built-in kinds and formats also implement as they move onto the Phase 2 relation layer
([`general-linter.md`](general-linter.md) section 11.5). Plugin results flow through the same
planner, limits, finding model, baseline identity, redaction, and fix engine as built-in ones.

## 5. The out-of-process protocol

### 5.1 Transport and framing

- stdin and stdout carry frames; stderr is captured as plugin logs (bounded, attributed to the
  plugin in diagnostics).
- Each frame is a varint length prefix followed by one message, as in Bazel persistent workers.
- The primary encoding is protobuf. A plugin may select the proto3-JSON encoding of the same
  schema (for debugging and for languages where adding protobuf is a hurdle); 64-bit integers are
  strings in proto3-JSON, and readers accept both forms.
- No sockets, ports, or named pipes: stdio works identically on Linux, macOS, and Windows, and
  the plugin's death is observed as EOF.

gRPC was considered and rejected: it adds an HTTP/2 and async-runtime stack to the host,
requires a socket or port (awkward on Windows and in sandboxes), and its streaming and
cancellation are reproduced more cheaply with request ids over stdio. JSON-RPC (LSP framing) was
rejected as the primary encoding for weaker typing and slower bulk transfer, MessagePack-RPC for
having no schema, and Cap'n Proto for narrower language coverage.

### 5.2 Session

```text
host spawns plugin (argv from config; cwd = repository root; minimal environment)
plugin -> host  encoding selection (protobuf | json)
host   -> plugin Hello { protocol: 1, contract: ">=0.3,<0.4", features: [...] }
plugin -> host  Hello { protocol: 1, contract: "0.3.2", features: [...] }
plugin -> host  PluginInfo { kinds, adapters, fixers, option schemas, limits }
host   -> plugin Configure { rule id, kind, options }         (per configured rule)
host   -> plugin AnalyzeBatch { id: 7, resources: [...] }    (many files per request)
plugin -> host  AnalyzeResult { id: 7, findings: [...] }     (requests multiplexed by id)
host   -> plugin Cancel { id: 9 }                            (best effort)
host   -> plugin Shutdown
```

- **Negotiation.** Unknown features are ignored, and neither side uses a feature the other did
  not confirm. Contract versions are negotiated as a semver range, so a plugin keeps working
  across host minor releases that the contract range covers.
- **Lifecycle.** One persistent process per plugin per run (and per LSP session), started
  lazily. Per-request timeouts; on timeout, EOF, a malformed frame, or an exit, the process is
  killed (process group on Unix, as the `command` kind already does) and the affected requests
  become analysis failures (DEC-2), never silent passes.
- **Batching and concurrency.** Requests carry many resources; the plugin declares whether it
  accepts concurrent requests. Bytes are sent once per resource per session.
- **Bounds.** Frame size, response size, finding count, and stderr volume are capped; exceeding a
  cap is an analysis failure.

## 6. The contract compiler

### 6.1 Why a separate project (`witforge`)

No existing tool generates protobuf or plain Rust types and traits from WIT. The ecosystem's
converters go the other way (a young protobuf-to-WIT generator; a draft mapping note), and
`wasmtime::component::bindgen!` and the wRPC bindings tie generated Rust to their runtimes. The
gap is real and of value beyond alint (wasmCloud's planned move from protobuf to WIT is the same
problem). DEC-14d therefore makes the compiler its own open-source project, with alint as its
first consumer, its scope held to what alint needs until alint's contract v1 ships.

### 6.2 MVP scope

- Input: a WIT package directory, parsed with `wit-parser` (pinned to an exact version and bumped
  deliberately), always with all features enabled so `@unstable` items are not silently dropped.
- A subset checker that refuses everything outside section 3.2's restricted subset, with a
  catch-all for WIT constructs the generator does not know.
- An internal model, then emitters:
  - plain Rust types and traits (for `alint-plugin-api`), with docs carried from `///` comments;
  - a `.proto` file whose field and enum numbers come from the lockfile (section 8);
  - JSON Schema (after the MVP).
- `--check` mode for CI, and stable, LF-only output.
- Golden tests for every mapping row in section 7 and round-trip tests (Rust value to protobuf to
  Rust value; Rust value to proto3-JSON and back).

alint keeps the host glue (wasmtime bindings, the process codec, conversions) in its own
repository; the compiler emits what that glue needs.

### 6.3 Prototype findings

A throwaway prototype (about 290 lines on `wit-parser` 0.262, 2026-10-14) rendered a sample
contract (records with spans, a variant with payload, payload-less, and tuple cases, enums,
flags, a resource, `list<u8>`, nested `option` and `list`, `result`, `@since` and `@unstable`
items) to proto3 with lockfile-assigned numbers, and kept numbers stable across a rename and a
package version bump. The API exposes everything a generator needs (packages with versions,
interfaces, worlds, type definitions, docs, stability). Gotchas found:

1. Record fields, variant cases, enum cases, and flags carry no stability attribute; only types,
   functions, interfaces, and worlds take `@since`. Per-member evolution needs versioned
   interfaces (section 8.3).
2. `@since(version = X)` may not exceed the package version; bump the package first.
3. `@unstable` items are filtered out unless all features are enabled.
4. `use` creates alias definitions, and `type x = list<T>` is a named list definition, not an
   alias; the generator must dedupe and inline both.
5. Lockfile keys must exclude the package version, or every bump renumbers everything.
6. Anonymous types (`result`, tuples, nested `list` and `option`) need synthesized message names;
   these affect only generated code, never the wire or JSON, so they are not locked.
7. Function names arrive mangled (`[method]x.y`); use the unmangled item name.
8. `wit-parser` makes breaking 0.x releases roughly monthly and its enums are exhaustive; pin it
   and keep the subset narrow.

## 7. Type mapping

| WIT | Rust | proto3 | proto3-JSON | Status and convention |
|---|---|---|---|---|
| `bool` | `bool` | `bool` | true / false | clean |
| `u8`, `u16` | `u8`, `u16` | `uint32` | number | range-checked on decode |
| `u32` | `u32` | `uint32` | number | clean |
| `u64` | `u64` | `uint64` | decimal string | readers also accept a number |
| `s8`, `s16` | `i8`, `i16` | `sint32` | number | range-checked on decode |
| `s32`, `s64` | `i32`, `i64` | `sint32`, `sint64` | number, string | clean |
| `f32`, `f64` | `f32`, `f64` | `float`, `double` | number or "NaN" / "Infinity" | clean |
| `char` | `char` | `string` of one scalar value | string | avoided in the contract |
| `string` | `String` | `string` | string | UTF-8 on both sides |
| `list<u8>` | `Vec<u8>` | `bytes` | base64 | clean |
| `list<T>` | `Vec<T>` | `repeated T` | array | clean for scalars and messages |
| `list<list<T>>`, `list<option<T>>` | nested | repeated wrapper message | array of objects | synthesized wrapper |
| `option<T>` (scalar, string, enum) | `Option<T>` | `optional T` | absent or null | clean |
| `option<T>` (record) | `Option<T>` | message field | absent or null | non-optional message fields are presence-checked on decode |
| `option<list<T>>`, `option<option<T>>` | nested | wrapper message | object | synthesized wrapper |
| `result<T, E>` | `Result<T, E>` | message with `oneof { T ok; E err }`; a missing side is `Empty` | `{"ok": ...}` / `{"err": ...}` | convention |
| `tuple<A, B>` | `(A, B)` | message with `_0`, `_1` | object | avoided; use named records |
| `record` | `struct` | message, numbers from the lockfile | object, lowerCamelCase keys | clean once locked |
| `variant` with payload | `enum` with data | message with `oneof`, numbers from the lockfile | `{"case": {...}}` | a list or option payload needs a wrapper; an unknown case is a decode error |
| `variant` case without payload | unit variant | `Empty` in the `oneof` | `{"case": {}}` | convention |
| `enum` | `enum` | `X_UNSPECIFIED = 0`, then locked cases | name string | zero or an unknown value is a decode error (closed enums) |
| `flags` | struct of bools | message of bools | object | preferred over a bitmask (readable JSON, no 64-flag cap) |
| `resource`, `own`, `borrow` | handle | opaque id message | n/a | disallowed in contract v1 |
| `type a = b` | `pub type` | inlined | n/a | clean |
| `map<K, V>` | `BTreeMap` | `map<K, V>` for string or integer keys | object | disallowed in v1; use `list<record>` |
| `future`, `stream`, fixed-length lists | n/a | n/a | n/a | refused by the subset checker |
| function | trait method | request and response messages plus a method id | n/a | convention |
| interface | trait or module | method-id enum | n/a | convention |
| package `ns:name@x.y.z` | Rust module | proto package `ns.name.vX` | n/a | major version only on the protobuf side |

## 8. Wire identity and evolution

### 8.1 Why the strictest system governs

wasmtime checks component types for exact equality (verified in the 49.x source: record field
count, order, and names; variants, enums, and flags compared exactly). So in the component model
adding a record field, a variant case, an enum value, or a flag is a breaking change, and only
patch versions of a `0.x` package may differ, and then only in docs. Protobuf tolerates new
fields by stable number, renames are wire-compatible but break proto3-JSON, and renumbering or
retyping is breaking. Rust tolerates new fields with `#[non_exhaustive]`. The contract follows
the strictest rule.

### 8.2 Stable field numbers: the lockfile

WIT has no field numbers, so the compiler keeps a committed, append-only `wire.lock` mapping
`ns:pkg/interface/type.member` to a number:

- a new member gets the next free number (skipping protobuf's reserved 19000 to 19999 range);
- a removed member is emitted as `reserved` (number and name) and its number is never reused;
- a rename is a removal plus an addition unless the WIT doc comment carries an explicit
  rename marker;
- CI fails if the lockfile loses or changes a line against the merge base, if a member is
  unlocked, or if generated output is stale; `buf breaking` at the WIRE_JSON level runs on the
  emitted schema as a second check;
- the lockfile is designed to be replaced by in-source numbers if the WIT annotations proposal
  (component-model issue 695) lands with a suitable annotation.

### 8.3 Evolution policy

1. A published contract version is frozen.
2. Changes ship as a new minor version (`0.x` while unstable), with the previous WIT kept in the
   tree (`wit/v0.1/`, `wit/v0.2/`, and so on).
3. New members must be defaulted (`option<T>` or `list<T>`), so the compiler can generate the
   old-to-new conversions; enum values and variant cases are append-only; reorders, renames, and
   type changes are forbidden.
4. The host keeps one adapter per supported contract version and converts each into the latest
   plain types; the supported range is published.
5. On the process wire, one protobuf package per major version; field numbers are stable across
   minors through the lockfile, so older process plugins keep working, and the handshake still
   states the exact contract version so the host enforces its semantics.
6. A major version is a new package (`alint:plugin@2`, protobuf `alint.plugin.v2`), negotiated at
   handshake, with an adapter for the previous major for a documented period.
7. Contract 1.0 freezes at alint v1.0 together with the DSL (DEC-9).

## 9. Trust and distribution

| Host | Declared where | Integrity | Capabilities |
|---|---|---|---|
| Native Rust | the `alint build` manifest | Cargo's own (lockfile, checksums) | full trust |
| Process | top-level config only, or a `trusted_extends:` source; never an ordinary `extends:` | SHA-256 per platform in `alint.lock`; optional cosign or attestation verification | full trust native code |
| WASM | top-level config or any `extends:` source, SRI-pinned | SRI digest | none: no filesystem, network, clock, randomness, or environment |

- Process plugins join the spawn gate: the `KindMeta` inventory marks them spawning, so the same
  trust partition that protects `command` applies.
- A plugin inherited through `extends:` receives no environment-derived values (the 2026-10 audit
  rule); host-side `{{env.X}}` interpolation into plugin options applies only to plugins declared
  in the top-level config.
- Content-injecting plugin fixes from untrusted sources are demoted exactly as built-in ones are.
- `alint.lock` records hashes for every supported platform at once (an `alint plugins lock`
  command fetches all of them), avoiding the lock file that only works on its author's operating
  system.
- Signature verification reuses the cosign and attestation tooling of ADR-0015.
- Capability grants extend `trusted_extends:`; ADR-0004 gains the plugin rules in a superseding
  ADR.

## 10. Delivery plan and estimates

Estimates are engineer-weeks for one experienced Rust engineer, produced on 2026-10-14; they
assume a contract of about 30 types, and contract-design rework is the most likely way to exceed
them.

### 10.1 Contract compiler (separate project)

| Component | Weeks |
|---|---|
| Subset checker and internal model | 1 to 1.5 |
| Rust types and traits emitter | 1.5 to 2.5 |
| Protobuf emitter and lockfile | 1.5 to 2 |
| Golden, round-trip tests and CI gates | 1 to 1.5 |
| JSON Schema emitter (after the MVP) | 0.5 to 1 |
| Project scaffolding, docs, releases | 2 to 4 |
| **MVP** | **about 12 to 18** |
| **1.0** (multiple emitters, conformance suite, broader WIT coverage) | **about 26 to 45** |

Ongoing cost: about a day per quarter for `wit-parser` bumps, plus issue triage.

### 10.2 alint-side platform

| Component | Weeks | Notes |
|---|---|---|
| Contract design in WIT | 1 to 2 | designed with the Phase 2 relation layer |
| `alint-plugin-api` crate (generated types, narrow traits, context snapshot) | 2 to 3 | API design dominates; the current `Rule` trait is far wider than a plugin boundary should be |
| Registry injection and namespaced plugin kinds | 1.5 to 2.5 | replaces the hard-coded `builtin_registry()` call sites |
| `alint build` | 2 to 3 | |
| Process host | 4 to 6 | spawn, handshake, framing, multiplexing, batching, timeouts, cancellation, crash handling, Windows |
| Process codec and proto3-JSON | 2 to 3.5 | on the emitted schema |
| WASM host (feature-gated) | 3 to 5, plus about 1 for release packaging | limits, deny-by-default imports, precompile cache |
| WASM host glue | 1 to 2 | possibly less if wasmtime derives can be applied to the plain types (needs a spike) |
| Trust, `alint.lock`, SRI, signatures | 2 to 4 | |
| Rust process SDK | 1 to 1.5 | |
| Python, Go, TypeScript SDKs | 1.5 to 2.5 each | |
| Conformance kit, docs, examples | 2 to 4 | |
| CI matrix and benchmarks | 1 to 2 | wall-clock on the canonical bench host; the instruction-count gate cannot see spawn and I/O cost |

Whole plugin track, including the compiler: **about 33 to 53 engineer-weeks**. The smallest
slice that runs a first real external plugin is **about 12 to 18**.

### 10.3 Order (DEC-14e)

| Slice | Target | Contents | Done when |
|---|---|---|---|
| P0 | now, parallel | contract-compiler repository: subset checker, Rust and protobuf emitters, lockfile, gates | it generates a sample contract's Rust and protobuf bindings with all gates green |
| P1 | v0.21, with Phase 2 | contract `0.x` in WIT; `alint-plugin-api`; registry injection; built-in formats and the `key_parity` pilot on the same traits | a built-in adapter and analyzer run through the plugin traits with no behaviour change |
| P2 | about v0.22 | native Rust plugins and `alint build` | an external crate adds a kind and an adapter to a custom binary, with scenarios |
| P3 | about v0.23 | process host (Linux and macOS, then Windows), Rust process SDK, trust and `alint.lock` | the same plugin crate runs as a process with identical findings |
| P4 | after P3 | WASM host and glue | the same plugin runs as a component with identical findings |
| P5 | after P4 | Python, Go, TypeScript SDKs; conformance kit; docs | one example plugin per language passes the conformance kit |
| P6 | v1.0 | contract 1.0 freeze with the DSL | ADR, published supported-version policy |

## 11. Changes required in alint

Facts from the 2026-10-14 code map, each a work item for P1 to P3:

- Every runtime build site uses `alint_rules::builtin_registry()` directly (the CLI's check, fix,
  and validate-config paths and the LSP); there is no injection hook, though `Engine::new` already
  accepts any `RuleRegistry`.
- `RuleBuilder` is a plain function pointer; plugin kinds need closures or trait objects carrying
  plugin state.
- `Format` is a closed enum with exhaustive matches and parity gates over `Format::ALL`;
  `ExtractSpec` has one hard-coded field per format. Adapters need an open format registry while
  keeping the parity gates for built-ins.
- `RuleSpec.fix` is a closed, untagged enum; plugin fixers need an extension point in the fix
  DSL.
- The JSON Schema's rule dispatch is a fixed `oneOf` over built-in kinds; plugin kinds need
  schemas contributed at runtime (for `validate-config`) and at build time (for `alint build`
  binaries and editors).
- The categories, docs-export, facts, and coverage-audit generators read `docs/rules.md` and the
  built-in registry; plugin kinds need their own documentation path and must not break those
  gates.
- The spawn-gate and fix-spawn-gate audits scan only `alint-rules` source; plugin capabilities
  must come from `KindMeta` (Phase 0) so the trust gates see them.
- `ARCHITECTURE.md`'s "native Rust plugins are out of scope" was replaced on 2026-10-14.

## 12. Risks

1. **Upstream churn.** wasmtime ships a major version monthly and `wit-parser` and `wit-bindgen`
   break often; mitigated by pinning, a narrow WIT subset, and a scheduled bump cadence.
2. **Contract design.** The contract is the hardest part to get right and the costliest to
   change; it is designed with the relation layer and stays `0.x` until v1.0.
3. **Version accumulation.** Frozen WIT types mean one adapter per supported version; the
   supported range must be published and pruned deliberately.
4. **Guest support gaps.** Large Python and JS components and no Java generator; out-of-process
   is the any-language path.
5. **Spawn and IPC cost** on many small files, especially on Windows; mitigated by persistent
   processes, batching, and caching, and measured by wall-clock benchmarks.
6. **Supply chain.** Native and process plugins are unsandboxed; integrity pinning, signatures,
   and spawn gating are the only defences.
7. **Binary size** from wasmtime; feature-gated and measured.
8. **Second-project burden.** The contract compiler is another project to maintain, and the WIT
   annotations proposal may land in a way that changes the field-number mechanism.

## 13. Open questions

These belong to the slices above and are decided in their design documents, one at a time.

| ID | Question | Slice | Recommendation |
|---|---|---|---|
| Q10 | Exact shape of the adapter, analyzer, and fixer worlds in contract `0.x` | P1 | separate worlds; inputs pushed; no host callbacks in `0.x` |
| Q16 | Name and repository of the contract compiler, and its first-release scope | P0 | **Decided 2026-10-14:** `witforge`, at `asamarts/witforge`, private until alint's contract `0.x` consumes it, then public with 0.1; MVP scope as section 6.2. The name was free on crates.io, npm, and PyPI with no GitHub collision on that date |
| Q17 | Plugin manifest, `alint build` manifest, and kind namespacing in `.alint.yml` | P2 | `<plugin>/<kind>` names; YAML `plugins:` in config; TOML build manifest |
| PA-1 | Host callbacks (embedded-region parsing through another adapter, as dprint offers) | after P4 | a versioned feature flag, not in `0.x` |
| PA-2 | Whether a plugin may contribute DSL fix operations, or only `Fixer` objects attached to its own kinds | P2 | only its own kinds' fixers in `0.x` |
| PA-3 | Whether wasmtime derives on the plain Rust types can replace generated conversion glue | P4 | spike before building the glue |
| PA-4 | Long-term-support policy for the wasmtime version alint pins | P4 | track a supported line and bump on a schedule |
| PA-5 | Where plugin kinds' documentation lives and how `alint rules` and `explain` show it | P2 | plugin info carries summaries and doc URLs; rendered alongside built-ins |
