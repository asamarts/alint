---
title: 'python@v1'
description: Bundled alint ruleset at alint://bundled/python@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/python@v1
```

## Rules

### `python-manifest-exists`

- **kind**: `file_exists`
- **level**: `error`
- **when**: `facts.is_python`
- **policy**: <https://packaging.python.org/en/latest/guides/writing-pyproject-toml/>

> Python project: a `pyproject.toml` (preferred), `setup.py`, or `setup.cfg` at the repo root is required.

### `python-has-lockfile`

- **kind**: `file_exists`
- **level**: `warning`
- **when**: `facts.is_python`

> A lockfile should be committed for reproducible installs (uv.lock / poetry.lock / Pipfile.lock / pdm.lock).

### `python-pyproject-declares-name`

- **kind**: `toml_path_matches`
- **level**: `warning`
- **when**: `facts.is_python`
- **policy**: <https://peps.python.org/pep-0621/>

> `pyproject.toml` has no `project.name` (PEP 621). Declare the distribution name so `pip install .` / `uv build` work.

### `python-pyproject-declares-requires-python`

- **kind**: `toml_path_matches`
- **level**: `info`
- **when**: `facts.is_python`

> `pyproject.toml` has no `project.requires-python`; declare a floor (e.g. `>=3.10`) so installs fail fast on unsupported interpreters.

### `python-module-snake-case`

- **kind**: `filename_case`
- **level**: `info`
- **when**: `facts.is_python`
- **policy**: <https://peps.python.org/pep-0008/#package-and-module-names>

> Python module filenames should be snake_case (PEP 8).

### `python-sources-final-newline`

- **kind**: `final_newline`
- **level**: `info`
- **when**: `facts.is_python`

### `python-sources-no-trailing-whitespace`

- **kind**: `no_trailing_whitespace`
- **level**: `info`
- **when**: `facts.is_python`

### `python-sources-no-bidi`

- **kind**: `no_bidi_controls`
- **level**: `error`
- **when**: `facts.is_python`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Python sources are rejected.

