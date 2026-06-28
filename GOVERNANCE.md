# Governance

This is a one-page setup-of-expectations doc; pre-1.0 alint is small
enough to govern from a single file. As the project grows, this doc
will too.

## Project status

- **Version:** v0.13.0. Pre-1.0; the DSL, plugin ABI, and `alint-core`
  public API are not yet committed to semver-major stability. Versioning
  is plain semver `x.y.z`: pre-1.0, a minor bump (`0.y` → `0.(y+1)`) may
  carry breaking changes and a patch bump (`0.y.z` → `0.y.(z+1)`)
  preserves compatibility. v1.0 ships when the surface stops moving; see
  [`docs/design/ROADMAP.md`](docs/design/ROADMAP.md).
- **Maintainership:** single-maintainer
  ([asamarts](https://github.com/asamarts)). The maintainer reviews
  every PR and cuts every release. Multi-maintainer governance arrives
  if/when a second person picks up sustained authorship; this section
  will be revised at that point.

## Decision-making

- **Default mode: lazy consensus.** Issues, discussions, and PRs that
  go ~3 working days without objection from the maintainer can be
  considered approved. Most contributions land this way.
- **Maintainer veto:** the maintainer reserves the right to decline
  any change at any time. Reasons will be stated in the PR or issue
  thread. Common ones: scope creep beyond the project's deliberate
  non-goals (AST-aware linting, IaC scanning, secret scanning; see
  [`README.md`](README.md) "When alint is NOT the right tool"); a
  rule kind without 3 demand sources from the case-study corpus
  (the saturation bar is documented in
  [`docs/development/launch-evidence.md`](docs/development/launch-evidence.md));
  performance regressions outside the published bench tolerances.
- **Roadmap changes:** discussed in [Discussions](https://github.com/asamarts/alint/discussions)
  under "Ideas." Roadmap edits land via PR against
  `docs/design/ROADMAP.md` once the discussion has converged.

## Contributing

- **Bugs and small fixes:** open a PR directly. Run
  `bash ci/scripts/preflight.sh` before pushing (or install the
  pre-push hook with `git config core.hooksPath ci/githooks` so it
  runs automatically). See [`CONTRIBUTING.md`](CONTRIBUTING.md).
- **New rule kinds:** open an issue first using the feature-request
  template. The bar is documented at
  [`CONTRIBUTING.md` § Proposing a new rule kind](CONTRIBUTING.md).
  Two real-repo demand sources is the floor; three is preferred.
- **New bundled rulesets:** same bar of three or more real repos
  converging on the same set of conventions.
- **DSL or schema changes:** issue first; these affect every
  adopter's `.alint.yml` and need a deprecation path. The maintainer
  will coordinate the schema bump and the migration guide.

## Security

Vulnerability reports go to <security@alint.org> per
[`SECURITY.md`](SECURITY.md). Do **not** open a public issue for
security findings; the email path lets us coordinate a fix + advisory
before disclosure.

## Code of conduct

[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) (Contributor Covenant).
Reports of conduct violations also go to <security@alint.org> until
the project has a separate moderation contact.

## License + contribution sign-off

alint is dual-licensed under [Apache-2.0](LICENSE-APACHE) OR
[MIT](LICENSE-MIT). By submitting a PR you agree your contribution is
licensed under the same terms (no separate CLA required).

## Funding

GitHub Sponsors is enabled (see the "Sponsor" button at the top of
the repo). Sponsorship is greatly appreciated but does not buy
prioritisation, roadmap influence, or private support. Feature
ordering is driven by case-study demand signal, full stop.
