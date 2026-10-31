---
status: accepted
date: 2026-10-31
decision-makers: asamarts
---

# 0027. Plugin contract 0.1 stays a draft until plugins load

## Status

Accepted (2026-10-31). Adds to [ADR-0025](0025-v021-pre-release-decisions.md) decision 4, which
stands: v0.21.0 is still not tagged until witforge 0.1.0 is published and pinned and the
contract's check, generation, and regeneration `--check` run in default CI. This ADR decides
what that tag promises about the contract.

## Context

`wit/alint-plugin/v0.1/plugin.wit` is the `alint:plugin@0.1.0` contract (ADR-0023, corrected in
ADR-0024). The plugin design's evolution policy (`plugin-architecture.md` section 8.3) starts
with "a published contract version is frozen": every later change is a new minor version, the
old WIT stays in the tree, and the host keeps one adapter per supported version. witforge
implements the freeze with a `released` manifest beside the version directories: a listed
version's WIT, wire lock, and generated module may no longer change, and its CI scripts fail a
change under a released directory.

Nothing in v0.21 uses the contract from outside alint:

- there is no plugin host, so no plugin can be loaded, and no handshake ever states a contract
  version;
- `alint_core::plugin_api`, the executable form of the contract, is doc-hidden with no semver
  promise, and the `alint-plugin-api` crate that plugins will build against arrives with slice
  P2 (ADR-0023);
- the parts of the contract that real plugins will exercise first are not written: the
  `fixer` world (Q10), the plugin manifest and kind namespacing (Q17), and the handshake that
  carries `plugin-info`.

The review of the witforge consumer pull request (#409) also found that the WIT and
`plugin_api` had already drifted once (`max-findings` against `Limits::max_rows`), with nothing
to catch it.

## Decision

1. **0.1 ships in v0.21 as a draft.** The contract is committed and checked by witforge in
   default CI as ADR-0025 decision 4 requires, but v0.21 does not publish it in section 8.3's
   sense: no `wit/alint-plugin/released` manifest is added, and 0.1 may change in place.
2. **The first release that loads plugins freezes it.** That release (slice P2, about v0.22)
   adds `wit/alint-plugin/released` listing the version it ships, and a CI check that nothing
   under a released version directory changes, beside the regeneration `--check`. From then on
   section 8.3 applies unchanged.
3. **One change, both sides.** Until the generated `alint-plugin-api` crate replaces
   `plugin_api`, `crates/alint-witforge-consumer/tests/plugin_api.rs` converts every
   `plugin_api` type to and from the generated types without `..` patterns and serves each
   trait through the other, so a field, case, or required method on one side without a
   counterpart on the other fails to compile. A change to either side updates the other in the
   same pull request.
4. **Say so.** `wit/README.md` and the release notes call 0.1 a draft with no compatibility
   promise until it is frozen.

## Consequences

- The v0.21 tag waits on witforge 0.1.0, not on the P2 design: changes that P2's fixer world,
  manifest, or handshake need can still be made to 0.1 itself, without a second version
  directory and a host adapter for a version no plugin ever used.
- The name `alint:plugin@0.1.0` may cover a draft in v0.21 and a different frozen contract in
  the P2 release. Because v0.21 cannot load a plugin, no running host ever states the draft's
  version, so the two never meet; plugin authors have nothing to build against before P2.
- The P2 release takes on one more acceptance item: the `released` manifest and its freeze
  check.
- `plugin_api` and the WIT cannot drift silently while both exist.

## Considered Options

- **Freeze 0.1 at the v0.21 tag:** rejected by the owner; it would freeze the contract before
  any plugin, fixer world, or handshake exists, and turn every change P2 needs into a version
  0.2 with a kept copy of 0.1 and a host adapter for it.
- **Leave the WIT unchecked until P2:** rejected by ADR-0025 decision 4.

## More Information

- [ADR-0023](0023-relation-ir-key-parity-and-plugin-contract-0x.md) and
  [ADR-0024](0024-relation-ir-decisions-after-review.md): the contract's shape and the
  doc-hidden `plugin_api`.
- [`plugin-architecture.md`](../design/plugin-architecture.md) section 8.3: the evolution policy
  that applies once a version is frozen.
- [`v0.21/witforge-adoption.md`](../design/v0.21/witforge-adoption.md): the review of #408 and
  #409 and the path to witforge 0.1.0 and the v0.21 tag.
