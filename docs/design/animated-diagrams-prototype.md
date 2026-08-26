# Design doc: animated-diagram prototype (engine + diagrams + dev-only QA)

Status: Draft (prototype build plan). Not implemented.
Decisions: builds on `animated-diagrams.md` (the interactive "Tier 3" track) and
ADR-0005 (adopt LikeC4). If the prototype validates and graduates, record the
production decision as ADR-0016. No engine/DSL/schema change to alint itself; all
code lands in the alint.org site repo.

## 1. Context

`animated-diagrams.md` proposed adding Cursor-style animated diagrams to the docs and
recommended a three-tier track. This is the build plan for a **prototype/MVP** that
validates the most interactive tier: a reusable rendering engine plus a few real,
educational animated diagrams, embedded on `/docs/about/architecture/` in **dev/local
only** (never live) for hands-on QA. The point is to prove the look/feel and the
authoring model before any production or live-docs commitment.

Two facts from exploring both repos shape everything:

- alint.org is Astro + Starlight with **no framework islands** (no React/Vue/Svelte).
  Its interactivity model is **vanilla custom elements + a static loader**, exactly the
  existing LikeC4 setup: `public/likec4-loader.js` lazily injects the heavy
  `public/likec4-views.js` only on pages that embed `<likec4-view>`. So the engine is a
  **framework-free web component** mirroring that pattern, not a React island.
- Dev-only is clean, three ways over: (1) the loader is injected only under
  `import.meta.env.DEV`, gated in the existing `src/components/Head.astro` Starlight
  override (`public/*.js` cannot read env, so the gate lives in the `.astro` layer);
  (2) the page `src/content/docs/docs/about/architecture.md` is **gitignored and
  wiped+re-synced on every build** (Cloudflare runs sync first), so local embeds can
  never reach production; (3) the tag is absent from the deployed bundle entirely.

The existing LikeC4 dynamic-view steps are **coarse C4 boxes**
(`dev -> alintBin -> dsl -> core -> rules -> output`) -- too abstract for Cursor-grade
detail -- so the engine consumes a **new, richer, hand-authored declarative spec**, not
the raw `.c4` steps. (Reusing the C4 steps and build-time SVG generation are graduation
paths, not MVP; see section 7.)

## 2. Approach

A dependency-free web component `<alint-anim spec="...">` that renders an inline SVG from
a declarative spec (nodes + edges + a captioned step timeline) and animates it
Cursor-style: nodes light up as the pipeline advances, tokens (files, facts, violations)
flow along edges, one caption per step. Transport controls (Prev / Play-Pause / Next /
progress), full keyboard a11y, and first-class `prefers-reduced-motion`. Runtime-rendered
like LikeC4 (gate the *spec* text, render live). Engine + specs are committed to
alint.org (durable); the architecture-page embeds are local/dev-only (throwaway by
construction).

## 3. The engine (the rendering pipeline)

All net-new under `public/anim/` (native ES modules served verbatim, no build step),
plus one guarded line in `Head.astro`. Mirrors the LikeC4 split: a tiny static loader
injects the heavier engine module on demand.

### 3.1 File layout

| Path | Role |
|---|---|
| `public/anim/anim-loader.js` | Static ES5 loader; verbatim mirror of `public/likec4-loader.js`. Presence-gate `if (!document.querySelector('alint-anim')) return;`, mirror `data-theme` + `MutationObserver`, inject `<script type=module src=/anim/anim-engine.js>`. |
| `public/anim/anim-engine.js` | `class AlintAnim extends HTMLElement` + `customElements.define`. `connectedCallback` resolves the spec, attaches an **open shadow root**, renders, wires controls + clock, picks the reduced-motion path. |
| `public/anim/anim-render.js` | Pure SVG builder (edges/nodes/tokens layers) + the `computeFrame(spec, i)` fold + `applyFrame` class application. Returns `index: Map<id,{el,kind,path?}>`. |
| `public/anim/anim-clock.js` | The rAF clock: `seek/next/prev/play/pause`, token spawn, `getPointAtLength` motion, stagger, chained-edge travel, `dt`-clamp, visibility/IntersectionObserver pause. |
| `public/anim/anim-controls.js` | Transport bar (native `<button>`s, `aria-pressed` Play, `role=slider` progress, `role=group`), keyboard map, `aria-live` caption, static caption `<ol>` for reduced motion. |
| `public/anim/anim-spec.d.ts` | The TypeScript types (3.6). Types only; give the `.js` specs autocomplete via `/** @type {...} */`; never shipped. |
| `public/anim/specs/index.js` | Registry: `export const SPECS = { 'check-pipeline': ..., ... }`. |
| `public/anim/specs/*.js` | One hand-authored spec per diagram. |

Edited (only committed edit): `src/components/Head.astro` -- after the existing appended
tags, a dev gate mirroring how the Cloudflare beacon `<script>` is already emitted:

```astro
{import.meta.env.DEV && (
  <script src="/anim/anim-loader.js" defer></script>
)}
```

When the prototype graduates, this line moves into `astro.config.mjs` `head[]` beside
`likec4-loader.js` (unconditional/production; section 7).

### 3.2 Web component + lifecycle

`connectedCallback`: resolve the spec (`spec="id"` -> `SPECS[id]`, or an inline
`<script type="application/json">` child) -> attach open shadow root -> render base SVG
-> build controls -> init the clock -> pick reduced-motion vs interactive path. Guard
double-connect; `disconnectedCallback` tears down rAF + observers.

**Open shadow DOM** encapsulates the controls CSS while CSS custom properties still
inherit through the boundary -- so `var(--sl-color-*)` inside the shadow tree tracks the
theme automatically. One inlined `<style>`.

### 3.3 Animation model (the key decision)

**One `requestAnimationFrame` scalar clock** drives token-along-path motion via
`path.getPointAtLength()`. Discrete node/edge appearance is a **pure function of the
current step** -- a `computeFrame(spec, i)` fold. This keeps Prev/Next/seek a clean
"compute the frame, swap the classes" with no half-finished tweens to unwind.

Chosen over the Web Animations API (unified scrub across N concurrent animations is more
bookkeeping, and token-along-path forces `offset-path`/pre-sampled keyframes) and over
pure CSS transitions (backward seek and per-step scrubbing fight the declarative model;
CSS cannot interpolate a token along a path). Manual rAF wins on the four axes that
matter: framework-free, smooth token motion, clean pause/seek/step, and a trivial
reduced-motion path.

The fold -- every element's status is derived by folding step deltas `0..i`:

```js
// status in {hidden, resting, done, active, dim}
function computeFrame(spec, i) {
  const status = new Map();
  for (const n of spec.nodes) status.set(n.id, n.hidden ? 'hidden' : 'resting');
  for (const e of spec.edges) status.set(e.id, endpointHidden(spec, e) ? 'hidden' : 'resting');
  for (let s = 0; s <= i; s++) {
    const step = spec.steps[s];
    (step.reveal ?? []).forEach(id => { if (status.get(id) === 'hidden') status.set(id, 'resting'); });
    (step.activate ?? []).forEach(id => status.set(id, s === i ? 'active' : 'done'));
    (step.dim ?? []).forEach(id => status.set(id, 'dim'));
  }
  return status; // applyFrame() toggles .is-active/.is-done/.is-dim/[hidden]
}
```

`reveal` is monotonic (hidden -> present, stays); `activate` lights the current step
strongest and leaves the traversed path lit as `done` (the "pipeline lights up as you go"
look); `dim` explicitly de-emphasizes a branch we are past. The clock spawns `<circle>`
tokens into a `tokens` layer per the step's `flow`, moves them along the edge path over
`durationMs` (`performance.now()` deltas, `dt` clamped ~64ms so a tab-away does not
teleport them), staggers multiple tokens, chains `edges: [...]` as one continuous travel,
then removes them on settle. Tokens are ephemeral, so a seeked/settled frame never has
partial token state -- backward is as cheap as forward.

### 3.4 Controls, a11y, reduced motion

**No autoplay on connect** -- default is user-driven stepping, which satisfies WCAG 2.2.2
(Pause/Stop/Hide, Level A) trivially. Play is an opt-in button; it never auto-starts under
reduced motion. Controls are native `<button>`s (`aria-pressed` Play toggle), a
`role=slider`/range progress with `aria-valuenow/min/max/text`, inside a
`role=group aria-label`; keyboard `<-/->`, Space, Home/End, visible focus rings. The
caption is an `aria-live="polite"` region.

Under `prefers-reduced-motion: reduce`: the initial frame is the **final fold** (everything
revealed, the whole path shown traversed, no tokens) plus a permanently-visible **numbered
`<ol>` of every step caption** -- the full narrative without motion. Transitions are
disabled (states snap); transport still steps instantly. This is the mandated
static-meaningful fallback.

### 3.5 Theme

SVG strokes/fills use `var(--sl-color-*)` and `currentColor`; Starlight sets
`data-theme="light|dark"` on `<html>` before first paint, so recolor on toggle is
automatic with no per-frame JS. The loader's `data-theme` mirror is belt-and-suspenders.

### 3.6 Spec schema

Hand-placed coordinates (not auto-layout): dependency-free, art-directed, deterministic
across builds; edges auto-route from node anchors, so authoring is "place ~15 nodes on a
20px grid, add the occasional waypoint." Auto-layout can graduate in later if specs
proliferate.

```ts
type NodeKind = 'process' | 'artifact' | 'store' | 'decision' | 'terminal' | 'group';
interface AnimNode { id; x; y; w; h; label; kind?; sublabel?; group?; hidden?; }
interface AnimEdge { id; from; to; label?; waypoints?; kind?: 'flow'|'dep'|'back'; curve?; }
interface AnimFlow { edge?; edges?; count?; spread?; reverse?; variant?; }
interface AnimStep { caption; activate?; dim?; reveal?; flow?: AnimFlow[]; durationMs?; dwellMs?; note?; }
interface AnimSpec { id; title; width; height; nodes: AnimNode[]; edges: AnimEdge[]; steps: AnimStep[]; defaults?; }
```

Node shapes by `kind`: process (rounded rect), terminal (pill), store (cylinder),
artifact (folded-corner note), decision (diamond), group (low-opacity backdrop).

### 3.7 Worked example -- the `alint check` pipeline

`specs/check-pipeline.js`: ~15 nodes hand-placed on a 20px grid (dev, alint binary,
`.alint.yml`, extends sources, facts, rule filter, parallel walk, FileIndex, dispatch,
per-file rules, cross-file rules, aggregate, emit, fixers, report), ~16 auto-routed edges,
and 11 captioned steps mirroring `docs/design/ARCHITECTURE.md`'s Execution model: run ->
load + resolve extends (cache/cycle/SRI) -> facts (parallel) -> filter by `when` -> walk
once -> FileIndex (deterministic sort) -> partition dispatch -> evaluate (each file read
once; cross-file scans the index) -> aggregate Violations -> fix (serial) -> emit + exit.
The captions carry the four invariants visually: **walk once** (one walk step), **read
each file once** (single file token per file), **facts + rules parallel** (concurrent
tokens), **fixers serial** (one slow token). This spec exercises every schema field
(`activate`, `dim`, `reveal` of hidden nodes/edges, single + staggered multi-token
`flow`, chained `edges[]`, per-step `durationMs`, `variant` token styles, `waypoints`,
every `NodeKind`) and is the reference for authoring the rest.

### 3.8 Gotchas (established, must-honor)

- **Node scaling:** position the node **group** via the `transform="translate(x y)"`
  *attribute* and scale the **inner** box via CSS with `transform-box: fill-box;
  transform-origin: center` (a CSS transform *replaces* the presentation attribute, so use
  different elements).
- **Never animate path `d`** (SMIL banned; CSS `d` support uneven). Draw an edge in via
  `stroke-dashoffset`; move a token *along* the static path via `getPointAtLength`.
- **Token motion:** `getTotalLength()` + `getPointAtLength()` set the token's `translate`
  each frame. Avoid `offset-path`/`offset-distance` for the prototype (historical Safari/
  Firefox gaps + coordinate-origin friction).
- **rAF hygiene:** `performance.now()` deltas, clamp `dt`, pause on `visibilitychange`
  hidden + off-screen via `IntersectionObserver`, `cancelAnimationFrame` on disconnect.

## 4. The diagrams (specs)

Hand-authored specs grounded in `docs/design/ARCHITECTURE.md` prose. Aesthetic:
monochrome line-art + accent `--sl-color-accent` (~`#93a4fd`), Cursor-style token
variants (file/fact/violation). MVP set (3 core + 1 stretch), each teaching a distinct
hot-path concept and mapping to an architecture-page section:

1. **`check-pipeline`** (hero; worked spec in 3.7) -- host section `## Execution model`.
2. **`walker-gitignore`** -- parallel walk (WalkBuilder) -> honor `.gitignore`/
   `.alintignore`/`ignore:` globs (files dropped) -> merge thread-local vecs +
   deterministic sort -> build lazy indices. Host `## Execution model` (walk step,
   currently prose-only -- net-new embed).
3. **`dispatch-partition`** -- partition rules (`requires_full_index()` -> rule-major;
   else per-file); file-major loop reads each file once and fans out to every applicable
   `PerFileRule`; cross-file rules scan the index; merge + sort. Host `## Rule model` /
   `### Dispatch flip`.
4. *(stretch)* **`monorepo-scoping`** -- nested `.alint.yml` discovery + closest-ancestor
   `scope_filter: has_ancestor` walk gating a rule per file. Host `### Closest-ancestor
   scoping`.

## 5. Dev-only embedding + QA

- Add the `Head.astro` dev gate (3.1).
- Embed `<alint-anim spec="..."></alint-anim>` in the **local**
  `src/content/docs/docs/about/architecture.md` in the relevant sections (alongside or in
  place of a static `<likec4-view>` where an animation teaches more). Gitignored +
  sync-overwritten, so embeds are dev-only by construction.
- Because that file is wiped on sync, an optional idempotent
  `scripts/inject-anim-demos.mjs` (git-tracked dev helper) re-applies the demo embeds
  after a sync, so QA is reproducible without hand-re-editing.
- QA with `astro dev` (Node via nvm: `. ~/.nvm/nvm.sh && nvm use default`).

## 6. Verification (end-to-end)

At `astro dev`, load `/docs/about/architecture/`, per diagram confirm: renders inline SVG;
Prev/Next/Play/Pause and progress work; captions update in the `aria-live` region; tokens
flow; concurrent tokens read as parallel and the serial-fixer step as one slow token;
light/dark recolors with no reflow/flash; emulated `prefers-reduced-motion` shows the
static final-fold + numbered caption list with no autoplay; keyboard controls work; no
console errors; the loader is inert on pages with no `<alint-anim>`.

Then confirm **production exclusion**: `npm run build:no-sync` (astro build, DEV=false) and
grep `dist/` to prove `/anim/anim-loader.js` is NOT referenced and no `<alint-anim>`
survives. This is the "not live" guarantee.

## 7. Scope boundaries and graduation paths (NOT in the MVP)

- **Build-time deterministic-SVG variant** (graduation (a)): a `scripts/` Node script
  (mirroring `scripts/build-likec4.mjs`, already chained in `npm run build`) imports the
  same `specs/*.js` and a shared headless render function and emits a static, fully-
  labelled SVG (final fold + caption list) -- a no-JS/print/GitHub rendering and a
  deterministic artifact for perf-gating. Runtime engine and build script share one render
  core so they cannot diverge.
- **Live docs** (graduation (b)): move the dev gate into `astro.config.mjs` `head[]`
  (unconditional) and author embeds in the alint repo's `docs/design/ARCHITECTURE.md` so
  they flow through the docs-bundle pipeline. Caveat (already documented for LikeC4):
  GitHub strips the custom element, so the build-time static SVG from (a) is what renders
  on GitHub, while alint.org shows the animated version.
- No auto-layout (hand-placed coordinates), no LikeC4-derived specs (the C4 steps are too
  coarse), **no new npm dependencies** (dependency-free). Engine + specs are committed to
  alint.org only; nothing lands in the alint repo or the live site in this prototype.

## 8. Files

- Create in alint.org: `public/anim/{anim-loader,anim-engine,anim-render,anim-clock,
  anim-controls}.js`, `public/anim/anim-spec.d.ts`, `public/anim/specs/index.js`,
  `public/anim/specs/{check-pipeline,walker-gitignore,dispatch-partition,
  monorepo-scoping}.js`, optional `scripts/inject-anim-demos.mjs`.
- Edit (committed): `src/components/Head.astro` (the dev-gate line).
- Edit (local/dev-only, not committed): `src/content/docs/docs/about/architecture.md`
  (demo embeds).
