# alint-witforge-consumer

Compile and round-trip proof for the committed `alint:plugin@0.1.0` WIT
contract (`wit/alint-plugin/v0.1`).

- `src/plain` (contract types and the `AdapterApi` and `AnalyzerApi` traits),
  `src/glue` (conversions to and from protobuf), and `proto/v0.proto` are
  witforge's output, committed byte for byte and checked with `--check`.
- `src/proto` includes the prost types and proto3-JSON impls that `build.rs`
  generates from `proto/v0.proto` with `protox`, `prost-build`, and
  `pbjson-build`; nothing generated from the schema is committed.
- `tests/` round-trips requests and responses through protobuf binary and
  proto3-JSON, converts every type and trait that `alint_core::plugin_api` and
  the generated code share (`plugin_api.rs`), and accounts for every item on
  both sides, recording why each unpaired one has no counterpart
  (`inventory.rs`). These tests compare `plugin_api` with the committed
  output; `ci/scripts/witforge-consumer.sh` compares that output with the WIT.

Do not hand-edit the generated files. Check or regenerate them as
[`wit/README.md`](../../wit/README.md) describes:

```sh
ci/scripts/witforge-consumer.sh            # check
ci/scripts/witforge-consumer.sh --write    # regenerate
```

The crate is a consumer proof, not the production `alint-plugin-api` crate,
and is not published. It is replaced when alint adopts the generated crate
(slice P2).
