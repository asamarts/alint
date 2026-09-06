---
title: The walker and git
description: "How alint discovers files by walking the tree through git's own ignore rules, why the walked tree can diverge from git's index, and how git_tracked_only switches a rule to the index."
sidebar:
  order: 5
---

Every run begins by walking your repository once into a sorted in-memory index, and every rule reads that index, never the raw filesystem and never `git` directly. What lands in the index is the working tree minus everything `.gitignore` excludes, so a rule's idea of "what is here" is the un-ignored working tree. That is almost always what you want; the two places it diverges from git's own index are the source of nearly every "why didn't my rule fire" question.

<svg class="alint-walk" viewBox="0 0 460 424" role="img" aria-labelledby="walk-t walk-d" xmlns="http://www.w3.org/2000/svg">
<title id="walk-t">The walker builds an index from the un-ignored tree; git_tracked_only reads git's index instead</title>
<desc id="walk-d">Files on disk split two ways. The default walked tree includes README.md and an un-ignored untracked file but drops a gitignored-but-committed file. The git index, which git_tracked_only consults, includes README.md and the committed file but not the untracked one. The two divergent files are the ones a mis-set gitignore hides.</desc>
<style>
  .alint-walk { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-walk { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-walk { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-walk .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-walk .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-walk .tx { fill:var(--tx); } .alint-walk .mut { fill:var(--mut); } .alint-walk .ac { fill:var(--ac); }
  .alint-walk .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-walk .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-walk .off  { opacity:.42; }
  .alint-walk .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:walkflow 1s linear infinite; }
  .alint-walk .pulse { animation:walkpulse 2.4s ease-in-out infinite; }
  @keyframes walkflow { to { stroke-dashoffset:-12; } }
  @keyframes walkpulse { 0%,100% { opacity:1; } 50% { opacity:.55; } }
  @media (prefers-reduced-motion:reduce){ .alint-walk .flow{animation:none;stroke-dasharray:none} .alint-walk .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">one walk, one index</text>
<text class="ui mut" x="442" y="16" text-anchor="end">git_tracked_only reads git instead</text>
<rect class="card" x="20" y="28" width="420" height="118" rx="10"/>
<text class="ui mut" x="34" y="46">on disk</text>
<rect class="chip" x="34" y="54" width="400" height="24" rx="6"/><rect x="34" y="54" width="5" height="24" rx="2" fill="#22c55e"/><text class="tag tx" x="48" y="70">README.md</text><text class="tag mut" x="428" y="70" text-anchor="end">committed</text>
<rect class="chip" x="34" y="82" width="400" height="24" rx="6"/><rect x="34" y="82" width="5" height="24" rx="2" fill="#7c3aed"/><text class="tag tx" x="48" y="98">config.local.yml</text><text class="tag mut" x="428" y="98" text-anchor="end">gitignored, committed</text>
<rect class="chip" x="34" y="110" width="400" height="24" rx="6"/><rect x="34" y="110" width="5" height="24" rx="2" fill="#f59e0b"/><text class="tag tx" x="48" y="126">scratch.tmp</text><text class="tag mut" x="428" y="126" text-anchor="end">untracked</text>
<path class="flow" d="M 180 146 C 180 176, 120 176, 120 200"/>
<path class="flow" d="M 280 146 C 280 176, 340 176, 340 200"/>
<text class="tag mut" x="34" y="166">walk + .gitignore</text>
<text class="tag mut" x="426" y="166" text-anchor="end">git ls-files</text>
<rect class="card" x="20" y="204" width="200" height="182" rx="10"/>
<text class="ui ac" x="34" y="226">walked tree</text>
<text class="tag mut" x="34" y="242">default rule view</text>
<rect class="chip" x="34" y="252" width="172" height="24" rx="6"/><rect x="34" y="252" width="5" height="24" rx="2" fill="#22c55e"/><text class="tag tx" x="48" y="268">README.md</text>
<rect class="chip pulse" x="34" y="282" width="172" height="24" rx="6"/><rect x="34" y="282" width="5" height="24" rx="2" fill="#f59e0b"/><text class="tag tx" x="48" y="298">scratch.tmp</text>
<rect class="chip off" x="34" y="312" width="172" height="24" rx="6" stroke-dasharray="4 3"/><text class="tag mut" x="48" y="328">config.local.yml</text>
<text class="tag mut" x="34" y="356">the un-ignored tree,</text>
<text class="tag mut" x="34" y="372">committed file filtered out</text>
<rect class="card" x="240" y="204" width="200" height="182" rx="10"/>
<text class="ui ac" x="254" y="226">git index</text>
<text class="tag mut" x="254" y="242">git_tracked_only: true</text>
<rect class="chip" x="254" y="252" width="172" height="24" rx="6"/><rect x="254" y="252" width="5" height="24" rx="2" fill="#22c55e"/><text class="tag tx" x="268" y="268">README.md</text>
<rect class="chip pulse" x="254" y="282" width="172" height="24" rx="6"/><rect x="254" y="282" width="5" height="24" rx="2" fill="#7c3aed"/><text class="tag tx" x="268" y="298">config.local.yml</text>
<rect class="chip off" x="254" y="312" width="172" height="24" rx="6" stroke-dasharray="4 3"/><text class="tag mut" x="268" y="328">scratch.tmp</text>
<text class="tag mut" x="254" y="356">what git tracks,</text>
<text class="tag mut" x="254" y="372">catches the drift</text>
<text class="tag mut" x="230" y="410" text-anchor="middle">the two files that differ are the ones a mis-set .gitignore hides</text>
</svg>

## What the walker sees

Starting at the path you pass to `alint check` (or the current directory), the walker yields every regular file under that root, **except** paths matched by any of: the repo's `.gitignore` files (root and per-directory), `.git/info/exclude`, your global gitignore (`core.excludesFile`), `.ignore` files (the same syntax, honored by the [`ignore`](https://docs.rs/ignore/) crate that powers `ripgrep` and the walker), the `.git/` directory itself, and anything in the config's `ignore:` list.

Hidden files **are** included: alint walks `.github/`, `.editorconfig`, and `.cargo/` by default. In-tree symlinks are followed, but a symlink whose target escapes the repo root, or that dangles, is pruned from the walk. No git repo is required; on a plain directory the walk just has nothing to filter, so every file is visible.

Two config fields shape the filtering. `respect_gitignore` (default `true`) toggles every gitignore source at once; the CLI's `--no-gitignore` forces it off for one run. `ignore:` adds gitignore-style patterns on top, and applies regardless of `respect_gitignore`, for exclusions that are an alint concern rather than a git one:

```yaml
version: 1
ignore:
  - "vendor/**"
  - "**/*.snapshot.json"
```

Setting `respect_gitignore: false` is rarely useful during development: absence-style rules (`dir_absent`, `file_absent`) begin firing on every locally-built `target/`, `node_modules/`, and `__pycache__/`. It fits one-off audits of a build tree, or linting a directory that is not a git repo.

## Walked tree versus git's index

Because the walker filters by `.gitignore`, "the walked tree" is a close but imperfect stand-in for "what git would commit." alint never reads `.git/index` or shells out to `git ls-files` for the walk, so the approximation drifts in two directions:

- **A gitignored-but-tracked file** (added to git first and gitignored later, or forced in with `git add -f`) stays in git's index on every commit, yet the walker filters it out. Absence rules never see it and content rules never inspect it.
- **An un-ignored untracked file** (a scratch file no `.gitignore` pattern covers) is in the walked tree but not git's index, so a rule fires on a file git is not tracking.

In a healthy repo neither case is common, and `git ls-files <path>` is the authoritative answer when you suspect one. When a rule must key off git's index rather than the walked tree, set `git_tracked_only: true` on it. The rule then fires only for paths git actually tracks, independent of `.gitignore` state:

```yaml
- id: target-not-tracked
  kind: dir_absent
  paths: "**/target"
  git_tracked_only: true
  level: error
```

| `target/` state | default | `git_tracked_only` |
|---|---|---|
| Gitignored, never built | silent | silent |
| Gitignored, built locally | silent | silent |
| Not gitignored, on disk | **fires** | silent (not in index) |
| Committed (not gitignored) | **fires** | **fires** |
| Gitignored but force-added | silent (walker prunes it) | **fires** |
| Not a git repo | **fires** | silent (no index) |

`git_tracked_only` applies to the existence kinds `file_exists`, `file_absent`, `dir_exists`, and `dir_absent`; any other kind rejects it at load rather than ignoring it. Outside a git repo (or with `git` off `PATH`) the tracked set is empty, so absence rules with the flag become silent no-ops (there is nothing to commit) and existence rules with it fail conservatively (no file qualifies). The git-hygiene family also ships `git_commit_message`, `git_no_denied_paths`, and other git-aware kinds; see the [rule reference](/docs/rules/) for the full set.

## In practice

The canonical "do not let `target/` be committed" rule keys off the index, not the walked tree, so a locally-built `target/` stays quiet while a force-added one is caught:

```yaml
version: 1
rules:
  - id: target-not-tracked
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
    message: "target/ must not be committed"
```

On a repo where someone ran `git add -f target/debug/build.log`:

```
error  target-not-tracked  target/ must not be committed
```

The same rule is silent for every developer who merely built locally, because that `target/` is gitignored and untracked.

## Going deeper

- [Configuration](/docs/configuration/) is the field reference for `ignore:`, `respect_gitignore`, and every rule field.
- [Scoping](/docs/concepts/scoping/) is how a rule narrows within the index, with `paths:`, `when:`, and `scope_filter:`.
- [Changed mode](/docs/concepts/changed-mode/) restricts a run to the files in a diff, layered on top of the walk.
- The interactive <a href="/docs/about/architecture-diagrams/">architecture diagrams</a> include the walker view (`walkerFlow`) as an explorable model.
