# Design documents

Plans, designs, and decision records for alint. Start with
[`ARCHITECTURE.md`](ARCHITECTURE.md) for how the engine fits together and
[`ROADMAP.md`](ROADMAP.md) for what ships when. Each release with a design of
its own has a `vX.Y/` directory (for example [`v0.21/`](v0.21/README.md)), and
new designs start from [`TEMPLATE.md`](TEMPLATE.md).

## Architecture decision records

Accepted ADRs are never rewritten; a correction is a new ADR that supersedes
the old one. New ADRs start from [`0000-template.md`](../adr/0000-template.md).
The dogfood rule `adr-index-lists-every-adr` in `.alint.yml` fails when an ADR
in `docs/adr/` is missing from this table.

| ADR | Decision | Status |
|---|---|---|
| [0001](../adr/0001-adopt-spec-driven-development.md) | Adopt spec-driven development | accepted |
| [0002](../adr/0002-config-language-is-yaml.md) | The configuration language is YAML with extends-based composition | accepted |
| [0003](../adr/0003-rule-engine-dispatch-and-determinism.md) | Rule engine dispatch model and determinism guarantee | accepted |
| [0004](../adr/0004-extends-trust-boundary-and-path-confinement.md) | Extends trust boundary and path confinement | accepted |
| [0005](../adr/0005-adopt-likec4-for-architecture-diagrams.md) | Adopt LikeC4 for architecture diagrams | accepted |
| [0006](../adr/0006-baseline-suppression.md) | Baseline suppression via a committed fingerprint file | accepted |
| [0007](../adr/0007-release-aware-documentation.md) | Release-aware, single-source documentation contracts | accepted |
| [0008](../adr/0008-git-tracked-only-is-a-kind-option.md) | `git_tracked_only` and `respect_gitignore` are kind-specific options | accepted |
| [0009](../adr/0009-rule-discovery-cli-config-vs-catalog.md) | Rule discovery in the CLI: `alint rules` (catalog) vs `alint list` (config) | accepted |
| [0010](../adr/0010-manifest-derived-rule-scope.md) | Manifest-derived rule scope stays on the scope_filter seam | accepted |
| [0011](../adr/0011-per-kind-rule-explanations.md) | Per-kind rule explanations via a generated summary bridge | accepted |
| [0012](../adr/0012-output-completeness-testing.md) | Output-completeness as a tested contract | accepted |
| [0013](../adr/0013-retained-spec-projection.md) | Render explain/list from a retained RuleSpec projection | accepted |
| [0014](../adr/0014-rule-page-examples-are-executed-fixtures.md) | Rule-page examples are executed scenario fixtures | accepted |
| [0015](../adr/0015-distribution-strategy.md) | Distribution channel tiering and supply-chain hardening | accepted |
| [0016](../adr/0016-pypi-distribution-channel.md) | PyPI distribution channel (wheels for uvx / pipx / pre-commit) | accepted |
| [0017](../adr/0017-auto-fix-edit-model-and-applicability.md) | Auto-fix edit model and applicability tiers | accepted |
| [0018](../adr/0018-eslint-integration-boundary.md) | Host-linter integration boundary (no ESLint plugin tier) | proposed |
| [0019](../adr/0019-general-linter-program-and-plugin-architecture.md) | General-linter program and plugin architecture | accepted |
| [0020](../adr/0020-explicit-analysis-time.md) | Explicit analysis time | accepted |
| [0021](../adr/0021-policy-source-files.md) | Policy-source files may decide conformance verdicts | accepted |
| [0022](../adr/0022-policy-authoring-loop-contracts.md) | Policy authoring loop contracts | accepted |
| [0023](../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md) | Relation IR, `key_parity`, and plugin contract 0.x | superseded by ADR-0024 |
| [0024](../adr/0024-relation-ir-decisions-after-review.md) | Relation IR decisions after review | accepted |
| [0025](../adr/0025-v021-pre-release-decisions.md) | v0.21 pre-release decisions | accepted |
| [0026](../adr/0026-measuring-the-v022-simplicity-gate.md) | Measuring the v0.22 simplicity gate | accepted |
| [0027](../adr/0027-plugin-contract-0-1-stays-a-draft.md) | Plugin contract 0.1 stays a draft until plugins load | accepted |
