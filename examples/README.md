# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/launch-prep.md`](../docs/launch-prep.md)). Each subdirectory is one
case study — a popular OSS repo's existing structural-validation tooling
inventoried, rebuilt as an alint config, and compared.

## Layout

```
examples/
├── README.md                          # this file
├── <owner>-<repo>/
│   ├── README.md                      # case study writeup
│   ├── .alint.yml                     # the alint config that matches their existing tooling
│   ├── existing-tooling.md            # inventory of what they enforce today
│   └── comparison.md                  # alint output vs existing tool output + perf delta
```

## Case studies

P2a (single-language + diverse-ecosystem):

- *(empty — populated as the validation pass progresses)*

P2b (polyglot monorepos):

- *(empty — populated as the validation pass progresses)*

## Using these as starting points

Each `<owner>-<repo>/.alint.yml` is a working config. To use one as a starting
point for your own repo:

```sh
curl -fsSL https://raw.githubusercontent.com/asamarts/alint/main/examples/<owner>-<repo>/.alint.yml \
  > .alint.yml
alint check
```

Trim what doesn't apply to your repo, add what's specific. The configs are
deliberately written to be readable + adaptable, not minimal.

## Contributing a case study

If you've adopted alint for a public repo, consider contributing the case
study back — it helps other users with similar repo shapes.

The per-repo workflow ([`docs/launch-prep.md`](../docs/launch-prep.md#per-repo-workflow-2-4-hr-per-repo)) describes the steps.
