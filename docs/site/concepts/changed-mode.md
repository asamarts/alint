---
title: Changed mode
description: "How alint check --changed restricts a run to the files in a diff for per-file rules, while cross-file and existence rules keep evaluating the whole tree so they stay correct."
sidebar:
  order: 7
---

`alint check --changed` layers a diff filter on top of the walk so a per-file rule sees only the files you touched, which is what a pre-commit hook or a PR check wants. Cross-file and existence rules deliberately opt out and keep evaluating the whole tree, because an invariant they enforce can be broken by a file you changed even when its partner did not.

<svg class="alint-chg" viewBox="0 0 460 424" role="img" aria-labelledby="chg-t chg-d" xmlns="http://www.w3.org/2000/svg">
<title id="chg-t">--changed filters per-file rules to the diff while cross-file and existence rules stay whole-tree</title>
<desc id="chg-d">A repository of four files has two changed. Per-file rules receive only the two changed files. Cross-file and existence rules receive all four files, so a whole-tree invariant is still checked.</desc>
<style>
  .alint-chg { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-chg { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-chg { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-chg .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-chg .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-chg .tx { fill:var(--tx); } .alint-chg .mut { fill:var(--mut); } .alint-chg .ac { fill:var(--ac); }
  .alint-chg .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-chg .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-chg .off { opacity:.45; }
  .alint-chg .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:chgflow 1s linear infinite; }
  .alint-chg .pulse { animation:chgpulse 2.4s ease-in-out infinite; }
  @keyframes chgflow { to { stroke-dashoffset:-12; } }
  @keyframes chgpulse { 0%,100%{opacity:1} 50%{opacity:.55} }
  @media (prefers-reduced-motion:reduce){ .alint-chg .flow{animation:none;stroke-dasharray:none} .alint-chg .pulse{animation:none} }
</style>
<rect class="card" x="30" y="28" width="400" height="128" rx="10"/>
<text class="ui mut" x="44" y="48">repository</text>
<text class="tag mut" x="416" y="48" text-anchor="end">2 of 4 files changed</text>
<rect class="chip pulse" x="44" y="56" width="372" height="22" rx="6"/><rect x="44" y="56" width="5" height="22" rx="2" fill="#4f46e5"/><text class="tag tx" x="58" y="71">src/parser.rs</text><text class="tag ac" x="404" y="71" text-anchor="end">changed</text>
<rect class="chip pulse" x="44" y="82" width="372" height="22" rx="6"/><rect x="44" y="82" width="5" height="22" rx="2" fill="#4f46e5"/><text class="tag tx" x="58" y="97">docs/guide.md</text><text class="tag ac" x="404" y="97" text-anchor="end">changed</text>
<rect class="chip off" x="44" y="108" width="372" height="22" rx="6"/><text class="tag mut" x="58" y="123">src/lib.rs</text><text class="tag mut" x="404" y="123" text-anchor="end">unchanged</text>
<rect class="chip off" x="44" y="134" width="372" height="22" rx="6"/><text class="tag mut" x="58" y="149">api.h</text><text class="tag mut" x="404" y="149" text-anchor="end">unchanged</text>
<path class="flow" d="M 180 156 C 180 186, 120 186, 120 210"/>
<path class="flow" d="M 280 156 C 280 186, 340 186, 340 210"/>
<rect class="card" x="20" y="214" width="200" height="176" rx="10"/>
<text class="ui ac" x="34" y="236">per-file rules</text>
<text class="tag mut" x="34" y="252">see the changed set</text>
<rect class="chip" x="34" y="262" width="172" height="22" rx="6"/><rect x="34" y="262" width="5" height="22" rx="2" fill="#4f46e5"/><text class="tag tx" x="48" y="277">src/parser.rs</text>
<rect class="chip" x="34" y="290" width="172" height="22" rx="6"/><rect x="34" y="290" width="5" height="22" rx="2" fill="#4f46e5"/><text class="tag tx" x="48" y="305">docs/guide.md</text>
<text class="tag mut" x="34" y="338">2 of 4 evaluated</text>
<text class="tag mut" x="34" y="358">no_trailing_whitespace,</text>
<text class="tag mut" x="34" y="374">file_header, filename_case</text>
<rect class="card" x="240" y="214" width="200" height="176" rx="10"/>
<text class="ui ac" x="254" y="236">cross-file + existence</text>
<text class="tag mut" x="254" y="252">see the whole tree</text>
<rect class="chip" x="254" y="262" width="172" height="20" rx="5"/><rect x="254" y="262" width="5" height="20" rx="2" fill="#4f46e5"/><text class="tag tx" x="268" y="276">src/parser.rs</text>
<rect class="chip" x="254" y="286" width="172" height="20" rx="5"/><rect x="254" y="286" width="5" height="20" rx="2" fill="#4f46e5"/><text class="tag tx" x="268" y="300">docs/guide.md</text>
<rect class="chip off" x="254" y="310" width="172" height="20" rx="5"/><text class="tag mut" x="268" y="324">src/lib.rs</text>
<rect class="chip off" x="254" y="334" width="172" height="20" rx="5"/><text class="tag mut" x="268" y="348">api.h</text>
<text class="tag mut" x="254" y="374">pair, file_exists</text>
<text class="tag mut" x="230" y="410" text-anchor="middle">cross-file and existence rules stay whole-tree for correctness</text>
</svg>

## The two diff modes

`--changed` picks its diff from git in one of two shapes:

| Invocation | Diff source | Fits |
|---|---|---|
| `alint check --changed` | working tree: modified plus untracked, `--exclude-standard` | pre-commit, local dev |
| `alint check --changed --base=main` | `main...HEAD` (three-dot, merge-base) | PR checks |

The three-dot `<base>...HEAD` form diffs against the merge-base of `<base>` and `HEAD`, which is exactly what a GitHub PR calls "your changes." `--base=<ref>` implies `--changed`, so the ref is the verb's argument; you never pass both `--changed` and a bare `--base` awkwardly. The same flags work on `alint fix --changed`.

## Which rules stay whole-tree

The filter narrows the file set for **per-file rules** only. Two families opt out, on purpose:

- **Cross-file rules** (`pair`, `for_each_dir`, `every_matching_has`, `unique_by`, `dir_contains`, `dir_only_contains`) always evaluate against the whole tree. A `pair` rule that requires every `api.h` to have an `api.c` must still fire when you delete `api.c`, even though the surviving `api.h` is not itself in your diff.
- **Existence rules** (`file_exists`, `file_absent`, `dir_exists`, `dir_absent`) also evaluate whole-tree, but the engine **skips a rule entirely when its `paths:` scope does not intersect the diff**. So a missing `LICENSE` does not fail every PR; it fails only the PRs that touch a `LICENSE`-shaped path. This keeps whole-tree correctness without re-reporting the same unchanged-tree finding on every unrelated PR.

## Edge cases

- **Empty diff** (nothing modified, nothing untracked): the run short-circuits to an empty report in milliseconds, so a no-op pre-commit is nearly free.
- **Outside a git repo** (or `git` missing from `PATH`): `--changed` hard-errors rather than silently falling back to a full check, because a silent full run would betray the intent the flag expressed.
- **Deleted files** appear in the diff. A `LICENSE` you deleted is in the changed set, the walker no longer sees it on disk, and an existence rule for `LICENSE` evaluates the whole tree (which now lacks it) and fires.

`--changed` pairs naturally with [`git_tracked_only:`](/docs/concepts/walker-and-gitignore/): the changed set is a working-tree concept and the tracked set is an index concept, so a rule with both fires only on tracked entries that are part of this diff.

## In practice

A PR check that lints only the files the branch touched, against the merge-base:

```bash
alint check --changed --base=origin/main
```

If the branch added a trailing-whitespace line to `src/parser.rs` and deleted `api.c` (leaving `api.h` orphaned), one report carries both a per-file finding and a whole-tree one:

```
warning  no-trailing-whitespace  src/parser.rs:42 trailing whitespace
error    header-source-pair      api.h has no matching api.c
```

The `no-trailing-whitespace` finding came through the diff filter; the `pair` finding came from the whole-tree pass, even though `api.h` was never in the diff.

## Going deeper

- [The walker and git](/docs/concepts/walker-and-gitignore/) is the whole-tree index this filter sits on top of.
- [Scoping](/docs/concepts/scoping/) covers `changed_since:`, the per-rule scope_filter counterpart to the run-wide `--changed`.
- [Configuration](/docs/configuration/) documents the rule kinds and their cross-file versus per-file classification.
