---
title: 'key_parity'
description: 'alint key_parity rule (cross-file): One reference file and a glob of peers must declare the same key set.'
sidebar:
  order: 18
categories: ['cross-file']
---

One reference file and a glob of peers must declare the same key set. The reference, and a `peers` value with no glob metacharacters, are literal paths read from disk even when ignored, so a gitignored `.env` can be checked with `reference: .env.example` and `peers: .env`; a `peers` glob selects from the index and respects `.gitignore`. JSON, YAML, TOML, dotenv, INI, and Java properties are supported, plus the Android string-resource and .NET `.resx` XML dialects. Nested keys are compared as leaf paths, arrays are values and are not descended, and duplicate keys are not parity violations.

`direction: both|missing|extra` selects the reported differences. `strip_root: true` (JSON, YAML, and TOML) compares below each file's single top-level key, as Rails locale files do (`en:`, `fr:`); a file with keys that has more than one top-level key, or whose top-level key is a value, is an analysis failure for that file: a failing peer is skipped while the other peers still compare, and a failing reference stops the rule. `plurals: cldr` treats a CLDR plural group — a nested object whose keys are all `zero/one/two/few/many/other`, or i18next's `_<category>` sibling suffixes, with `other` present — as one key. Groups are decided over the reference and each peer together, so a locale with fewer categories (or without `other`) keeps the group, and a base name that is a real key in either file is never merged; exact per-locale category checking is deliberately not in v0.21. `ignore_keys` takes patterns written the way findings show keys: in dotenv, properties, and `.resx` files a pattern matches the key name, where `*` stops at a `.` and `**` crosses it (`app.*`, `app.**`), and a name findings show in bracket form is written that way (`['']`, `['[x']`); in every other format a pattern is a key path, where `*` matches within one segment, a `**` segment matches any number of segments (at the end, at least one), and `['a.b']` names a segment containing a dot (`nav.*` matches `nav.home` and `nav['a.b']`). A malformed path pattern is a configuration error, and so is an index such as `items[0]`, since array elements are values. `format` is inferred from the extension (`.env` / `.env.*` infer dotenv; `.toml` infers TOML; `.resx` infers `resx`) and must be set explicitly for an extensionless reference or an Android `.xml` file (`format: android-strings`). An explicit `format` reads every file in that format, but a file whose extension implies another key model is still refused (see below). Android keys are the pair (element type, name), so `<string name="x">` and `<plurals name="x">` do not collide; `translatable="false"` entries are skipped. `.resx` compares string `<data name>` entries and skips non-string resources and designer metadata; `direction: extra` is its usual setting. Keys are compared by identity, so a flat key named `nav.home` never equals a `home` key nested under `nav`. Findings and baseline keys show a dotenv, properties, or `.resx` key as written (`app.title`, `button1.Text`), and any other key as its path, with bracket notation for a segment that contains a `.`, `[`, `]`, or quote, or is empty (`nav.home`, `nav['a.b']`, `Session['session.save_path']`). Files are compared only within one key model: tree formats together, flat dotenv/properties together, and INI and each XML dialect on its own; a literal peer in another model is a configuration error, and a globbed peer in another model is an analysis failure for that peer.

A missing reference or literal peer, an unparsable reference or peer, and a `peers` glob that matches no file are fail-loud analysis outcomes, never a pass; a peer with no keys reports every reference key as missing. `allow_missing_peers: true` turns an absent peer (a literal `peers` path that does not exist, or a glob that selects nothing) into an informational note, for a file such as `.env` that exists on developer machines but not in CI; a missing reference still fails. Findings quote keys only, never values. The kind has no fix: inserting missing keys waits for the generic edit plan. Under `--changed` the rule is skipped only when neither the reference nor a peer changed, and otherwise evaluates over the full index; a rule with a literal peer runs whenever anything changed, because Git does not report edits to an ignored file such as `.env`.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow_missing_peers` | boolean |  | `false` | Report an absent literal peer, or a `peers` glob that selects no file, as an informational note instead of an analysis failure. For a peer that exists only on developer machines, such as a gitignored `.env`. |
| `direction` | one of `both` \| `missing` \| `extra` |  |  | Which differences to report. |
| `format` | one of `json` \| `yaml` \| `toml` \| `dotenv` \| `properties` \| `ini` \| `android-strings` \| `resx` |  |  | Key format. Inferred from the extension when omitted; `.env` and `.env.*` infer dotenv and `.resx` infers resx, while an Android `.xml` file needs `android-strings`. |
| `ignore_keys` | list of string |  | `[]` | Keys to omit, written as they appear in findings. In dotenv, properties, and `.resx` files a pattern matches the key name, where `*` stops at a `.` and `**` crosses it; a name shown in bracket form (`['']`, `['[x']`) is written that way too. In every other format a pattern is a key path: `*` matches within one segment, a `**` segment matches any number of segments (at the end of a pattern, at least one), and `['a.b']` names a segment that contains a dot. |
| `peers` | string | yes |  | Glob selecting the peer files. The reference is excluded automatically. A value without glob metacharacters is a literal path, read even when gitignored (`peers: .env`). |
| `plurals` | one of `exact` \| `cldr` |  |  | Treat a CLDR plural group (nested object or i18next `_other` siblings) as one key. Exact per-locale category checking is not part of v0.21. |
| `reference` | string | yes |  | The authoritative key file. |
| `strip_root` | boolean |  | `false` | Compare below each file's single top-level key, as in Rails locale files (`en:`, `fr:`). JSON, YAML, and TOML only. A file with keys must have exactly one top-level key, an object, or it is an analysis failure. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths` or `expect_matches`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A locale missing a key from the reference

The rule fires on this repository:

```text
locales/
locales/en.json
locales/fr.json
```

```json title="locales/en.json"
{"nav": {"home": "Home", "about": "About"}}
```

```json title="locales/fr.json"
{"nav": {"home": "Accueil"}}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: locales-match-english
    kind: key_parity
    reference: locales/en.json
    peers: "locales/*.json"
    level: error
```

`alint check` reports:

```ansi
[2m--- locales/fr.json ------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mlocales-match-english[0m
              locales/fr.json: missing key `nav.about` (present in
              locales/en.json)
              [2mrelated:[0m reference key: locales/en.json:1:26-1:33

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every locale declares the reference key set

This repository is compliant:

```text
locales/
locales/en.json
locales/fr.json
```

```json title="locales/en.json"
{"nav": {"home": "Home", "about": "About"}}
```

```json title="locales/fr.json"
{"nav": {"home": "Accueil", "about": "À propos"}}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: locales-match-english
    kind: key_parity
    reference: locales/en.json
    peers: "locales/*.json"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

