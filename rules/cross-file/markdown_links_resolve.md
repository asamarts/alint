---
title: 'markdown_links_resolve'
description: 'alint markdown_links_resolve rule (cross-file): Validate live Markdown link, image, and reference-definition destinations against the repository tree.'
sidebar:
  order: 3
categories: ['cross-file', 'content']
---

Validate live Markdown link, image, and reference-definition destinations against the repository tree. Relative destinations resolve from the source file's directory after URL percent-decoding; query strings and fragments do not affect the filesystem lookup. Both files and directories are valid targets. External URLs and other URI schemes, protocol-relative URLs, and same-page `#fragment` links are intentionally offline/out of scope. Explicit reference uses such as `[guide][install]` and `[guide][]` also report undefined labels.

The CommonMark-aware scanner skips YAML (`---`) and TOML (`+++`) front matter, fenced and indented code blocks (including nested blocks), inline code spans, and HTML comments, so documentation examples and prose are not mistaken for live links. It recognizes footnote definitions instead of misclassifying their prose as link destinations, while still checking real links inside footnotes. It handles escaped, nested, and multiline link syntax; a real link whose visible text contains inline code, such as ``[`Rule`](../rule.md)``, is still checked. Each bad link is a separate violation at the destination's line and Unicode-scalar column.

`relative: forbid` is the site mode: relative links fail even if their source-tree path exists, preventing trailing-slash rendered pages from resolving them against the wrong URL. Add a `root` map to validate root-absolute rendered routes against their source tree (`dir: .` maps to the repository root); extensionless routes try the exact path, `<route>.md`, and `<route>/index.md`.

This is a whole-index cross-file rule: in `--changed` mode a non-empty change checks the complete Markdown link graph, so deleting a target is caught even when the linking page did not change. Check-only; guessing a moved destination is not a safe mechanical edit.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `relative` | one of `resolve` \| `forbid` |  | `resolve` | How to handle relative Markdown destinations: `resolve` (default) or `forbid`. |
| `root` | object { `dir`, `url_prefix` } |  |  | Optional mapping from root-absolute rendered URLs to repository source files. This validates only URLs beneath the declared prefix. |

Plus the common `paths`, `level`, `id`, `when`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repository link into a moved directory is reported

The rule fires on this repository:

```text
CHANGELOG.md
docs/
docs/benchmarks/
docs/benchmarks/macro/
docs/benchmarks/macro/results/
docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/
docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.4/
docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.4/README.md
```

```markdown title="CHANGELOG.md"
# Changelog

Benchmarks: [v0.9.4](docs/benchmarks/macro/results/linux-x86_64/v0.9.4/).
```

```markdown title="docs/benchmarks/macro/results/linux-x86_64-ryzen-3900x/v0.9.4/README.md"
# results
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: repo-links
    kind: markdown_links_resolve
    paths: CHANGELOG.md
    level: error
```

`alint check` reports:

```ansi
[2m--- CHANGELOG.md ---------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mrepo-links[0m
              [2m3:22[0m  Markdown link `docs/benchmarks/macro/results/linux-x86_64/v0.9.4/`
              resolves to `docs/benchmarks/macro/results/linux-x86_64/v0.9.4`
              but no file or directory exists

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Live repository links resolve while examples and external URLs are ignored

This repository is compliant:

```text
README.md
docs/
docs/guide.md
docs/logo.svg
```

````markdown title="README.md"
# Project

See [the guide](docs/guide.md "Guide") and ![logo](docs/logo.svg).

[reference]: docs/guide.md?view=full#start

Continue with [the reference][reference].
External links such as [alint](https://alint.org) stay offline.

`[example](missing.md)`
<!-- [commented example](missing.md) -->
```markdown
[fenced example](missing.md)
```
````

```markdown title="docs/guide.md"
# Guide
```

```text title="docs/logo.svg"
<svg/>
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: repo-links
    kind: markdown_links_resolve
    paths: README.md
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

