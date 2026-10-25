# v0.21 R2: row adapters, diagnostics, and coalescing

Status: implementation note for work item R2 of
[`README.md`](README.md). This note does not change the accepted scope; it
records the decisions R1 left open so the adapters, `key_parity` (R3), and the
shadow port (R4) can be built without reworking the row model.

## 1. A row can be a structural key

`key_parity` compares keys, and `StructuredEntryRow` deliberately carries a
key, a key span, and a value span, not a parsed value. Reusing
`RowState::Value(Value::Null)` for such a row would make an object key
indistinguishable from an explicit `null`, which contradicts the IR's own
"no absence coercion" rule.

`RowState` therefore gains one variant:

```rust
pub enum RowState {
    Value(Value),
    /// The row names a structural key; it has no value. Used by key-only
    /// adapters such as `key_parity`'s entry-row adapter.
    Key,
    Missing,
    Error(Box<str>),
}
```

`Row::value()` keeps returning `None` for `Key`, `Missing`, and `Error`;
`Row::is_key()` distinguishes the new case. `Relation::distinct_values()`
ignores key rows. The existing three absence states are unchanged.

## 2. Adapter output carries diagnostics

The `Extract` wrapper must reproduce today's `cross_file` behaviour: string
values become rows, non-string values become one analysis failure that counts
the kinds it saw, and non-literal values become informational notes. Neither
the non-string failure nor the note is a row, and a note is deliberately not an
analysis failure.

`Adapter` therefore returns an output record rather than a bare `Vec<Row>`:

```rust
pub struct AdapterOutput {
    pub rows: Vec<Row>,
    pub non_strings: Vec<NonStringValueKind>,
    pub notes: Vec<AdapterNote>,
}

pub enum AdapterNote {
    /// A value the string-only wrapper skipped because it is computed, e.g.
    /// `${VAR}` or `{{ name }}`. The value stays here so the rule can redact it
    /// before formatting the existing note message.
    NonLiteral { value: String },
}
```

A total parse failure stays `AdapterError::Parse`; a limit hit stays
`AdapterError::Limit`. `AdapterOutput` is per resource. The rule that consumes
it owns message formatting, baseline keys, and redaction, so the IR never
formats a finding and never leaks a value through a diagnostic string.

`Adapter::id()` changes from `&'static str` to `&str` so an adapter instance
can carry a specification identity. The `Extract` wrapper derives a stable id
from its extractor (`extract:json:$.foo`, `extract:regex:<pattern>`, ...); the
entry adapters use their format (`entry-json`, `entry-yaml`, ...). The decode
cache keys on `(adapter.id(), resource.path())`, so two different extract
specifications over the same file never share a decode.

## 3. Two adapters, two decoding contracts

- **`EntryRowsAdapter`** wraps `extract_entry_rows` for JSON, YAML, dotenv, INI,
  and Java properties. It emits one `RowState::Key` row per source-order object
  member, using the entry's `parent + key` pointer and its `key_span`. Duplicate
  keys stay as distinct rows. It decodes **strict UTF-8**: these rows carry byte
  spans into the original file, and lossy replacement would shift every later
  span. Invalid UTF-8 is a parse failure for the resource. It also uses the
  strict YAML form: a YAML alias or a plain `<<` merge key is a parse failure,
  because rows do not expand either and comparing unexpanded keys would report
  false drift. A quoted `"<<"` remains an ordinary key. TOML, generic XML, and
  HCL stay unsupported here; R9 adds the TOML and XML dialect adapters.
- **`ExtractAdapter`** wraps today's `Extract` for every format plus `lines`,
  `regex`, and `whole_file`. It decodes **lossily**, exactly as `cross_file`
  does today, and applies `is_non_literal` to every extractor except
  `whole_file`. It uses the bounded extraction path so a crafted file cannot
  materialise more than `rows_per_extractor` values before failing.

Array members are outside `key_parity`'s key model. `extract_entry_rows` still
emits the objects it finds inside arrays, and it emits an object-valued key
such as `nav` alongside `nav.home`. The `key_parity` consumer (R3) therefore
drops any row whose pointer contains an `Index` segment and keeps only leaf
pointers; the adapter does not decide the key model.

## 4. Limits

Both adapters check `LimitKind::RowsPerExtractor` before each row is appended;
an over-limit result is `AdapterError::Limit`, never a truncated success.
`ExtractAdapter` also counts non-string values against the same limit, matching
`extract_values_bounded`'s existing accounting. The defaults in R1 remain
provisional until R2 records the corpus and scenario measurements. The shadow
port passes `cross_file`'s existing 10,000-value cap through `Limits`, so the
wrapper cannot become more permissive than the implementation it shadows.

## 5. Coalescing

The read and decode caches are separate:

- `ReadCache` maps a repository path to a `ResourceRead` and performs at most
  one filesystem read per path per run.
- `DecodeCache` maps `(adapter id, repository path)` to an `AdapterOutput` and
  performs at most one decode per pair per run.

Both use a per-key `OnceLock` obtained under a short lock, so concurrent
consumers share one initializer rather than decoding twice and racing on an
insert. Counters are exposed for the R2/R3 tests; production does not read
them.

A literal resource must be symlink-confined before it reaches the read cache:
`normalize_confined` is lexical, while `cross_file` additionally verifies the
resolved path with `resolved_within_root`. R2 moved that filesystem check into
`alint-core::pathsafe` (where the lexical check already lived) and has
`alint-rules` re-use it, so the read cache and `cross_file` cannot diverge.
A literal resource also rejects a FIFO, socket, or device before opening it:
`read_for_analysis` opens a path, and opening a FIFO read-only blocks until a
writer appears. Index entries take the cheaper path: the walker already prunes
escaping symlinks and indexes regular files only, so the cache does not repeat
the containment or file-type checks per file. The cache records `Present`,
`Missing`, `TooLarge`, and `Unreadable`; the consuming rule maps those to the
existing read-failure findings. Its `Debug` implementation prints counts only,
never cached bytes or error text, because `FileIndex` derives `Debug`.

The engine integration keeps the existing file-major read for per-file rules
and routes relation consumers through the same run-scoped caches. Both caches
live on `FileIndex` and are enabled together, so two relation rules cannot
decode one `(adapter, resource)` pair twice. Only resources a relation consumer
marks (`mark_interest`, or automatically for a literal `Resource`) are
retained; every other per-file read still happens once and is dropped, so
enabling a relation rule cannot pin the whole tree's bytes in memory at 1M
files. The test-only relation consumer in the engine's tests reads through that
cache while a normal per-file rule reads the same file; the counters must show
one read, one hit, and one decode for the pair.

## 6. Sensitive parse errors

An adapter keeps the parser's diagnostic so a non-sensitive failure stays
useful, and declares `redacts_by_default()` when the source format is
secret-bearing (dotenv, Java properties, and a structured extractor over
either). The IR never formats a finding: the consuming rule applies the
existing redaction before rendering a parse failure. The first consumer's
regression test (R3/R4) plants a secret in an invalid document and asserts it
never reaches any rendered finding.

## 7. Completion evidence for R2

- unit tests for every adapter: key rows, duplicates, spans, array members,
  strict/lossy decoding, non-string kinds, non-literal notes, parse failures,
  sensitive-error marking, and every limit;
- cache tests: one read and one decode per `(adapter, resource)`, two adapters
  decode two ways, a concurrent initializer runs once;
- the test-only relation consumer alongside a per-file rule, plus the
  relation-free hot path keeping the direct read and a `--changed` filtered
  index sharing the full index's cache;
- S1-S3 syscall counts within the reference gate's allowance and no instruction
  regression beyond +3% against v0.18.2.
