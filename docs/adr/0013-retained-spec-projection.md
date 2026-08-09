---
status: accepted
date: 2026-08-08
decision-makers: asamarts
---

# 0013. Render explain/list from a retained RuleSpec projection

## Status

Accepted. This resolves the open structural-prevention choice left by ADR-0012
(commitment 3): of the two moves ADR-0012 named, we adopt the retained-spec
projection and reject the `Rule::message()` trait accessor. The decision below is
settled; the implementation lands in the companion `explain-spec-projection`
change and is guarded by gates already on `main`.

## Context

`alint explain <rule>` and `alint list` render a rule's *configured* detail (kind,
scope, message, `when:`, kind-specific options) for local introspection. Since the
output-completeness work began (ADR-0012), the data those commands read has
accreted as **ad-hoc display-only fields on the engine's runtime `RuleEntry`**,
one field per gap as each was closed:

- `when_src` (the `when:` source string, so `explain` shows the authored clause
  rather than the parsed AST) - `crates/alint-core/src/engine.rs:79`
- `kind` - engine.rs:86
- `paths` - engine.rs:90
- `message` - engine.rs:94
- `extra` (the flattened kind-specific options) - engine.rs:98

Five display-only fields now sit on `RuleEntry`, none read on the check hot path,
each with its own `with_*` builder and its own wiring at the load site. Every new
"explain should also show X" gap has meant a new field, a new builder, and a new
load-site call. ADR-0012's commitment 3 named two ways to stop this accretion and
deliberately left the choice to a follow-up:

1. a `Rule::message()` trait accessor emitted from `rule_common_impl!`, or
2. retaining the `RuleSpec` projection so the renderers read the configured shape
   directly.

The trait accessor was investigated during the kind-options change and **does not
hold uniformly.** 65 rule structs carry a `message: Option<String>` field, but a
substantial set do not: the git-commit family plus two cross-file kinds - 8
structs (`git_commit_*`, `pair_changed_together`, `changeset_requires_path`) -
carry a `message_override` field instead (named to disambiguate from an actual
commit message), and the iterator / cross-file kinds delegate to nested rules and
hold no message of their own. A macro emitting `self.message` cannot compile for
those; per-kind hand-wiring is fragile; and the accessor only ever addresses
`message` - `paths` and the heterogeneous kind options still have no home on the
trait. Tellingly, those same kinds already read their message from the spec -
their builders do `message_override: spec.message.clone()`
(`crates/alint-rules/src/git_commit_message.rs`) - so `spec.message` is the one
uniform source that holds every kind's authored message, including the kinds the
accessor cannot reach.

The `RuleSpec` (`crates/alint-core/src/config.rs`), by contrast, already holds
**all** of it uniformly - `kind`, `paths`, `message`, `when` (the authored source),
`fix`, `scope_filter`, and the flattened `extra` options - because
`RuleSpec::deserialize_options` proves `extra` is exactly the recognized
kind-options. And there is prior art in-tree: `export_agents_md::collect_directives`
(`crates/alint/src/export_agents_md/mod.rs`) already renders straight from
`config.rules`, not from per-kind trait methods.

## Decision

We will **retain the rule's `RuleSpec` on its `RuleEntry`** and render
`explain` / `list` from it, retiring the five ad-hoc display fields in its favour.
Concretely:

- `RuleEntry` gains `spec: Option<Arc<RuleSpec>>`, set at the CLI config load site
  (`load_rules`) for the config-built rules `explain`/`list` render. It is `None`
  for `Engine::new`, nested (iterator-child) entries, AND the LSP's check-only load
  path (which builds entries for diagnostics, never renders explain/list, and so
  skips the retained spec) - exactly the cases where the five display fields were
  already empty. The migration MUST preserve the existing kind-empty invariant
  (`engine.rs`): a `None` spec renders as an absent kind, which `list --category`
  treats as "no categories" and silently drops - identical to today's empty-`kind`
  behaviour.
- The runtime fields stay: `rule` (the built `Box<dyn Rule>`), `when` (the parsed
  `WhenExpr` used for gating), and `allow_out_of_root`. The five display fields
  (`when_src`, `kind`, `paths`, `message`, `extra`) and their `with_*` builders are
  removed.
- `explain`, `list`, and `list --category` read `entry.spec` through small
  accessor helpers - `spec.kind`, `spec.paths`, `spec.message`, `spec.when` (the
  authored source, replacing `when_src`), `spec.extra` - so the next "explain
  should also show X" is a one-line render change, not another `RuleEntry` field.
  `explain --format json` keeps deriving both its `when` source and its
  `conditional` flag from that one retained source, so the two cannot disagree;
  `list --format json`'s `conditional` reads the equivalent parsed `when`.
- **The `Rule::message()` trait accessor is rejected** for the reasons in Context.

This changes no rule's runtime behaviour, no rendered output content, and none of
the existing completeness gates. It is a data-flow refactor behind byte-identical
output.

## Consequences

Easier:

- One retained value replaces five accreting fields; the next display gap is a
  render change, not an engine-struct change. The "one ad-hoc field per gap" smell
  is gone.
- A single source of truth for what a rule is configured as (the spec), shared by
  `explain`, `list`, and `export-agents-md`.

Harder, and accepted:

- **`RuleEntry`'s public shape changes** - five `pub` fields out, one in. It is an
  `alint-core` public type, but pre-1.0 and pre-frozen-API: RELEASING.md scopes the
  semver contract to the observable product surface (`.alint.yml`, CLI, machine
  output) and states the `alint-core` API "joins at v1.0", so this ships as a PATCH
  under that document's tie-breaker (an unchanged config yields byte-identical
  findings and output). Its only in-tree consumers are the CLI renderers, which
  move in the same change.
- A per-rule spec clone at load time: `spec` is borrowed while entries build, so
  retaining it is `Arc::new(spec.clone())` - a full `RuleSpec` clone, not a
  refcount bump. The dominant field (`extra`) was already cloned into `RuleEntry`
  at this site; the clone newly copies a few small fields too (`id`, `level`,
  `policy_url`, `fix`, `scope_filter`), so the marginal cost is negligible rather
  than literally nil, and it is off the hot path - the check loop never touches
  `spec`.

Mostly guarded, with one gap this change must close. Four completeness gates
already on `main` - `explain_surfaces_configured_rule_detail`,
`explain_covers_every_registered_kind`, `list_human_surfaces_kind_and_markers`,
and `explain_surfaces_kind_options` (`crates/alint/tests/cli_format_contract.rs`) -
assert the rendered output for `kind`, `paths`, `message`, and the kind options,
so a lossy migration of those four fails a test. The fifth retired field,
`when_src`, is NOT covered: the only when-touching gate asserts the `list`
`[when]` marker, which reads the retained parsed `when` (`entry.when.is_some()`),
not the authored source, and no `explain` fixture sets a `when:`. So `explain`'s
`when:` source line and `explain --format json`'s `when` / `conditional` fields
would migrate unguarded. This change therefore ADDS a positive `when`-source
completeness gate - assert `explain` (human and json) surfaces a configured
`when:` and reports `conditional: true` - as part of the same work, closing the
exact gap-class ADR-0012 named for the one field it had not yet reached.

## Considered Options

- **Retain the `RuleSpec` projection on `RuleEntry` (chosen).** Uniform across
  message, paths, options, and `when:` in one value; matches the
  `export_agents_md` prior art; ends the field accretion. Costs one `RuleSpec`
  clone per rule at load (wrapped in an `Arc`), negligible over today since the
  dominant `extra` field is already cloned here.
- **A `Rule::message()` trait accessor.** Rejected: 8+ kinds do not hold the
  message in a `message` field (`message_override` / delegated to nested rules), so
  a macro-emitted `self.message` cannot be uniform, and it addresses only
  `message` - not `paths` or the heterogeneous kind options.
- **Keep adding ad-hoc `RuleEntry` display fields (status quo).** Rejected: every
  future display gap keeps meaning a new engine field, builder, and load wiring -
  the accretion this ADR exists to stop.
- **A bespoke display-projection struct instead of `Arc<RuleSpec>`.** Slightly less
  memory, but a second type to define and keep in sync with `RuleSpec`; the spec is
  already exactly the configured shape, so retaining it is simpler.
- **`Box<RuleSpec>` instead of `Arc`.** The entry is the sole owner today
  (`RuleEntry` is not `Clone`), so `Box` would avoid the atomic refcount. `Arc` is
  kept for the near-zero-cost option of sharing the spec later (e.g. with rendered
  iterator children) without a field-type change; the atomic ops are off the hot
  path.
- **Keep the specs in `LoadedConfig` (a parallel id-keyed map) rather than on
  `RuleEntry`.** ADR-0012 sketched this variant. Viable, but the renderers already
  hold the `RuleEntry`; hanging the spec off it avoids a per-render id/index
  lookup.

## More Information

- Resolves ADR-0012 (output-completeness as a tested contract), commitment 3, and
  keeps the completeness gates that ADR introduced as the migration's guardrail.
- Prior art for spec-based rendering: `export_agents_md::collect_directives`
  (`crates/alint/src/export_agents_md/mod.rs`).
- The fields being retired were added across the explain enrichment (kind / paths /
  message / when_src) and the kind-options change (extra); anchors at
  `crates/alint-core/src/engine.rs:79-98` and the load site in
  `crates/alint/src/main.rs`.
- Independent of ADR-0011 (per-kind explanation prose), which adds a *new* data
  source rather than reorganising the config-instance projection this ADR covers.
