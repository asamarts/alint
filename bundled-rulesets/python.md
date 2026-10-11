---
title: 'python@v1'
description: 'python@v1 bundled alint ruleset: Hygiene checks for Python projects (pyproject / setuptools / Poetry / PDM / uv).'
---

Hygiene checks for Python projects (pyproject / setuptools /
Poetry / PDM / uv). Adopt it with:

```yaml
extends:
  - alint://bundled/python@v1
```

Gated with `when: facts.has_python` (true if any standard Python
project marker exists anywhere in the tree) plus a per-rule
`scope_filter: { has_ancestor: [pyproject.toml, setup.py,
requirements.txt] }` on per-file content rules so they only
apply to files inside a Python package — useful in polyglot
monorepos where Python packages sit alongside Rust / Node / Go
subdirectories. Override `has_python` with your own `facts:`
block if you need a different heuristic.

## What it checks

8 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [python-manifest-exists](#python-manifest-exists)<br>`error` | Python project: a `pyproject.toml` (preferred), `setup.py`, or `setup.cfg` at the repo root is required. |
| [python-has-lockfile](#python-has-lockfile)<br>`warning` | A lockfile should be committed for reproducible installs (uv.lock / poetry.lock / Pipfile.lock / pdm.lock). |
| [python-pyproject-declares-name](#python-pyproject-declares-name)<br>`warning` | `pyproject.toml` has no `project.name` (PEP 621). |
| [python-pyproject-declares-requires-python](#python-pyproject-declares-requires-python)<br>`info` | `pyproject.toml` has no `project.requires-python`; declare a floor (e.g. `>=3.10`) so installs fail fast on unsupported interpreters. |
| [python-module-snake-case](#python-module-snake-case)<br>`info` | Python module filenames should be snake\_case (PEP 8). |
| [python-sources-final-newline](#python-sources-final-newline)<br>`info` | File must end with a single \n. |
| [python-sources-no-trailing-whitespace](#python-sources-no-trailing-whitespace)<br>`info` | No line may end with space or tab. |
| [python-sources-no-bidi](#python-sources-no-bidi)<br>`error` | Trojan Source (CVE-2021-42574): bidi override chars in Python sources are rejected. |

All 8 rules run only when `facts.has_python` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `python-manifest-exists`

Modern Python wants `pyproject.toml` (PEP 518 / 621), but `setup.py` + `setup.cfg` are still the reality in older code bases. Any one of the three satisfies the rule.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.has_python`
- **policy**: <https://packaging.python.org/en/latest/guides/writing-pyproject-toml/>

> Python project: a `pyproject.toml` (preferred), `setup.py`, or `setup.cfg` at the repo root is required.

```yaml
- id: python-manifest-exists
  when: facts.has_python
  kind: file_exists
  paths: ["pyproject.toml", "setup.py", "setup.cfg"]
  root_only: true
  level: error
  message: >-
    Python project: a `pyproject.toml` (preferred), `setup.py`,
    or `setup.cfg` at the repo root is required.
  policy_url: "https://packaging.python.org/en/latest/guides/writing-pyproject-toml/"
```

### `python-has-lockfile`

Accept any of the common lockfile names from uv / Poetry / Pipenv / PDM. At least one should be committed for reproducible installs. Skip this rule (`level: off`) if your project genuinely pins via `requirements.txt` with hashes.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.has_python`

> A lockfile should be committed for reproducible installs (uv.lock / poetry.lock / Pipfile.lock / pdm.lock).

```yaml
- id: python-has-lockfile
  when: facts.has_python
  kind: file_exists
  paths:
    - uv.lock
    - poetry.lock
    - Pipfile.lock
    - pdm.lock
  root_only: true
  level: warning
  message: >-
    A lockfile should be committed for reproducible installs
    (uv.lock / poetry.lock / Pipfile.lock / pdm.lock).
```

### `python-pyproject-declares-name`

Both rules are scoped to `pyproject.toml` — if a project still lives on setup.py / setup.cfg without a pyproject.toml at all, neither rule evaluates and the project passes. When pyproject.toml does exist but the enforced field is missing, the rule fires (no match is a violation, which is the point here).

- **kind**: [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- **level**: `warning`
- **when**: `facts.has_python`
- **policy**: <https://peps.python.org/pep-0621/>

> `pyproject.toml` has no `project.name` (PEP 621). Declare the distribution name so `pip install .` / `uv build` work.

```yaml
- id: python-pyproject-declares-name
  when: facts.has_python
  kind: toml_path_matches
  paths: pyproject.toml
  path: "$.project.name"
  matches: '^[A-Za-z][A-Za-z0-9._-]*$'
  level: warning
  message: >-
    `pyproject.toml` has no `project.name` (PEP 621). Declare
    the distribution name so `pip install .` / `uv build` work.
  policy_url: "https://peps.python.org/pep-0621/"
```

### `python-pyproject-declares-requires-python`

Bracket notation is required on keys with dashes — JSONPath dot-notation segments are restricted to \[A-Za-z\_\]\[A-Za-z0-9\_\]\*.

- **kind**: [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- **level**: `info`
- **when**: `facts.has_python`

> `pyproject.toml` has no `project.requires-python`; declare a floor (e.g. `>=3.10`) so installs fail fast on unsupported interpreters.

```yaml
- id: python-pyproject-declares-requires-python
  when: facts.has_python
  kind: toml_path_matches
  paths: pyproject.toml
  path: "$.project['requires-python']"
  matches: '^.+$'
  level: info
  message: >-
    `pyproject.toml` has no `project.requires-python`; declare
    a floor (e.g. `>=3.10`) so installs fail fast on
    unsupported interpreters.
```

### `python-module-snake-case`

PEP 8: module filenames are lowercase with underscores. Scoped to `src/**` and the top level only — tests commonly use `test_*.py` (already snake) and conftest.py which are fine.

- **kind**: [`filename_case`](/docs/rules/naming/filename_case/)
- **level**: `info`
- **when**: `facts.has_python`
- **policy**: <https://peps.python.org/pep-0008/#package-and-module-names>

> Python module filenames should be snake\_case (PEP 8).

```yaml
- id: python-module-snake-case
  when: facts.has_python
  kind: filename_case
  paths: ["*.py", "src/**/*.py"]
  case: snake
  level: info
  message: "Python module filenames should be snake_case (PEP 8)."
  policy_url: "https://peps.python.org/pep-0008/#package-and-module-names"
```

### `python-sources-final-newline`

Changed in v0.9.18: the cosmetic-formatting rules (final-newline + no-trailing-whitespace) default-exclude the canonical test-fixture trees. Linters and test-suite repos (cpython `Lib/test/**`, ruff `crates/*/resources/test/fixtures/**`, pytest-style `tests/fixtures/**`, Go-style `**/testdata/**`) carry deliberately-malformed Python files; the rules can't distinguish "real source" from "intentional bad input" so the exclude defaults narrow the scope. Users with an unconventional fixture layout can override via per-rule `paths:` in their config. The Trojan-Source check (`python-sources-no-bidi`) deliberately keeps the wider scope — bidi-control chars in fixtures are still a security concern.

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.has_python`
- **fix**: `file_append_final_newline` (applied by `alint fix`)

```yaml
- id: python-sources-final-newline
  when: facts.has_python
  kind: final_newline
  paths:
    include: ["**/*.py"]
    exclude:
      - "tests/fixtures/**"
      - "**/tests/fixtures/**"
      - "**/test/fixtures/**"
      - "**/testdata/**"
      - "Lib/test/**"
      - "crates/**/resources/**"
  scope_filter:
    has_ancestor: [pyproject.toml, setup.py, requirements.txt]
  level: info
  fix:
    file_append_final_newline: {}
```

### `python-sources-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **when**: `facts.has_python`
- **fix**: `file_trim_trailing_whitespace` (applied by `alint fix`)

```yaml
- id: python-sources-no-trailing-whitespace
  when: facts.has_python
  kind: no_trailing_whitespace
  paths:
    include: ["**/*.py"]
    exclude:
      - "tests/fixtures/**"
      - "**/tests/fixtures/**"
      - "**/test/fixtures/**"
      - "**/testdata/**"
      - "Lib/test/**"
      - "crates/**/resources/**"
  scope_filter:
    has_ancestor: [pyproject.toml, setup.py, requirements.txt]
  level: info
  fix:
    file_trim_trailing_whitespace: {}
```

### `python-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.has_python`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Python sources are rejected.

```yaml
- id: python-sources-no-bidi
  when: facts.has_python
  kind: no_bidi_controls
  paths: "**/*.py"
  scope_filter:
    has_ancestor: [pyproject.toml, setup.py, requirements.txt]
  level: error
  message: "Trojan Source (CVE-2021-42574): bidi override chars in Python sources are rejected."
  policy_url: "https://trojansource.codes/"
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: has_python
    any_file_exists:
      - pyproject.toml
      - "**/pyproject.toml"
      - setup.py
      - "**/setup.py"
      - setup.cfg
      - "**/setup.cfg"
      - requirements.txt
      - "**/requirements.txt"
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/python@v1
rules:
  - id: python-manifest-exists
    level: off
  - id: python-has-lockfile
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/python@v1
    except: [python-manifest-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/python.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/python.yml) in the alint repo.
