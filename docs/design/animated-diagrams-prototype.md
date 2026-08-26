# Design doc: animated-diagram prototype (engine + diagrams + dev-only QA)

Status: Draft (prototype build plan). Not implemented. Revised after an adversarial
audit (two independent auditors + codebase verification); the corrections are folded in
below and noted where they changed a prior claim.
Decisions: builds on `animated-diagrams.md` (the interactive "Tier 3" track) and
ADR-0005 (adopt LikeC4). If the prototype validates and graduates, record the
production decision as ADR-0016. No engine/DSL/schema change to alint itself; all code
lands in the alint.org site repo.

## 1. Context

`animated-diagrams.md` proposed adding Cursor-style animated diagrams to the docs and
recommended a three-tier track. This is the build plan for a **prototype/MVP** that
validates the most interactive tier: a reusable rendering engine plus a few real,
educational animated diagrams, embedded on `/docs/about/architecture/` in **dev/local
only** (never live) for hands-on QA. The point is to prove the look/feel and the
authoring model before any production or live-docs commitment.

Two facts from exploring both repos shape everything (all verified against the code):

- alint.org is Astro + Starlight with **no framework islands** (no React/Vue/Svelte).
  Its interactivity model is **vanilla custom elements + a static loader**, exactly the
  existing LikeC4 setup: `public/likec4-loader.js` lazily injects the heavy
  `public/likec4-views.js` only on pages that embed `<likec4-view>`. So the engine is a
  **framework-free web component** mirroring that pattern, not a React island.
- Dev-only rests on two independent mechanisms (see section 6 for the exact test):
  (i) the loader `<script>` is emitted only under `import.meta.env.DEV`, gated in the
  existing `src/components/Head.astro` Starlight override (`public/*.js` cannot read
  env, so the gate lives in the `.astro` layer); (ii) the page
  `src/content/docs/docs/about/architecture.md` is **gitignored and wiped+re-synced by
  `sync-from-alint.mjs` on every real build** (Cloudflare runs the sync-gated `build`),
  so local embeds can never reach production. Note the engine's static files under
  `public/anim/` copy verbatim into `dist/anim/` on any build, but are **inert** unless
  the DEV-gated loader references them -- so "dev-only" means "never referenced/activated
  in production HTML," not "absent from `dist/`."

The existing LikeC4 dynamic-view steps are **coarse C4 boxes**
(`dev -> alintBin -> dsl -> core -> rules -> output`) -- too abstract for the intended
detail -- so the engine consumes a **new, richer, hand-authored declarative spec**, not
the raw `.c4` steps. (Reusing the C4 steps and build-time SVG generation are graduation
paths, not MVP; see section 7.)

## 2. Approach

A dependency-free web component `<alint-anim spec="...">` that renders an inline SVG from
a declarative spec (nodes + edges + a captioned step timeline) and animates it: nodes
light up as the pipeline advances, tokens (files, facts, violations) flow along edges,
one caption per step. Transport controls (Prev / Play-Pause / Next / progress), full
keyboard a11y, and first-class `prefers-reduced-motion`. Runtime-rendered like LikeC4
(the *spec* is the text artifact; render live). Engine + specs are committed to
alint.org (durable); the architecture-page embeds are local/dev-only (throwaway by
construction).

Three decisions the audit pinned down, stated up front because they are load-bearing:

- **Light DOM, not shadow DOM.** For a *documentation* diagram, the caption narrative and
  the step-list are real prose a reader will Ctrl-F, select, copy, and print -- all of
  which are unreliable inside a shadow root (find-in-page across shadow DOM is
  inconsistent; global docs typography does not cross the boundary). Render into light
  DOM with a scoped class prefix (`.alint-anim`), and **namespace every `<defs>` id per
  instance** (the id/marker/gradient collision that a shadow root would otherwise hide;
  see 3.2). CSS-custom-property theming works identically in light DOM.
- **Author-routed edges, not auto-routed.** Real obstacle-avoiding auto-layout is the
  Graphviz/ELK problem; a from-scratch heuristic among ~15 nodes produces spaghetti. The
  author places nodes AND specifies edge anchors (`fromSide`/`toSide`) + optional
  waypoints; the engine draws a smooth/elbowed path through them. See 3.7.
- **Staged build (de-risk first).** Build the engine against **check-pipeline only**,
  end-to-end, and iterate on look/feel + authoring ergonomics before authoring the other
  diagrams. Freeze the schema, then author the rest. See section 4.

## 3. The engine (the rendering pipeline)

All net-new under `public/anim/` (native ES modules served verbatim, no build step),
plus one guarded line in `Head.astro`. Mirrors the LikeC4 split: a tiny static loader
injects the heavier engine module on demand.

### 3.1 File layout

| Path | Role |
|---|---|
| `public/anim/anim-loader.js` | Static ES5 loader; mirror of `public/likec4-loader.js`. Presence-gate `if (!document.querySelector('alint-anim')) return;`, mirror `data-theme`, inject `<script type=module src=/anim/anim-engine.js>` once. |
| `public/anim/anim-engine.js` | `class AlintAnim extends HTMLElement` + guarded `customElements.define`. `connectedCallback`: resolve spec -> `validateSpec()` -> render into light DOM -> wire controls + clock -> pick reduced-motion path. |
| `public/anim/anim-render.js` | Pure SVG builder (edges/nodes/tokens layers; per-instance `<defs>` id namespacing) + `computeFrame(spec, i)` + `applyFrame`. Shared with the build-time graduation (7a). |
| `public/anim/anim-clock.js` | The rAF clock: `seek/next/prev/play/pause`, token spawn with cached path lengths, `getPointAtLength` motion, elapsed-time pause, dt-clamp, visibility/IntersectionObserver pause. |
| `public/anim/anim-controls.js` | Transport bar (native `<button>`s + a native `<input type=range>` progress), keyboard map, `aria-live` caption, the step `<ol>`. |
| `public/anim/anim-validate.js` | `validateSpec(spec)`: every edge `from`/`to` and every step-referenced id resolves; returns errors. |
| `public/anim/anim-spec.d.ts` | TypeScript types (3.6). Types only; never shipped. |
| `public/anim/specs/index.js` | Registry `export const SPECS = {...}`. |
| `public/anim/specs/*.js` | One hand-authored spec per diagram. |

Edited (only committed edit) -- `src/components/Head.astro`, after the existing
appended tags, matching how the Cloudflare beacon `<script>` is emitted there (**`is:inline`
is required** -- Astro bundles bare `<script>` tags and a hoisted one can silently
vanish; the beacon and the astro.config `head[]` likec4-loader both use the raw-tag form):

```astro
{import.meta.env.DEV && (
  <script is:inline src="/anim/anim-loader.js" defer></script>
)}
```

When the prototype graduates, this line moves into `astro.config.mjs` `head[]` beside
`likec4-loader.js` (unconditional/production; section 7).

### 3.2 Web component, lifecycle, DOM

`connectedCallback`: resolve the spec (`spec="id"` -> `SPECS[id]`, or an inline
`<script type="application/json">` child); run `validateSpec()`; on failure render a
visible fallback (the step `<ol>` + a bordered error note) and **do not enter the rAF
loop**; else render the SVG into **light DOM** under a `.alint-anim` scoped wrapper, build
controls, init the clock, pick reduced-motion vs interactive. Reserve a `min-height` on
the host so lazy engine injection does not cause layout shift (CLS). Guard double-connect;
`disconnectedCallback` tears down rAF + observers.

Light DOM means CSS could leak both ways; contain it with a single scoped stylesheet
(`.alint-anim ...` selectors) injected once. The only thing a shadow root was buying --
`<defs>` id isolation across multiple diagrams on one page -- is handled explicitly:
**prefix every gradient/marker/filter/clip id with the instance id** (`arrow-<uid>`), so
N diagrams never collide.

### 3.3 Animation model

**One `requestAnimationFrame` scalar clock** drives token-along-path motion via
`path.getPointAtLength()` (per-path `getTotalLength()` is cached once at render, not per
frame). Discrete node/edge appearance is a function of the current step -- a
`computeFrame(spec, i)` pass -- applied as scoped CSS classes. This keeps Prev/Next/seek a
clean "compute the frame, swap the classes" with no half-finished tweens.

Chosen over the Web Animations API (unified scrub across N concurrent animations is more
bookkeeping) and pure CSS transitions (backward seek + per-step scrub fight the
declarative model; CSS cannot interpolate a token along a path). Manual rAF wins on the
four axes that matter: framework-free, smooth token motion, clean pause/seek/step, and a
trivial reduced-motion path.

`computeFrame` -- **corrected from the audit**: `reveal` and the visited/`done` trail are
**monotonic** (folded over `0..i`), but `active` and `dim` are **state at step i**
(computed from the current step, NOT folded), so a later step can freely restore a node
to `done`/`resting` instead of being forced to over-emphasize it via `activate`:

```js
// per-element status: hidden | resting | done | active | dim
function computeFrame(spec, i) {
  const status = new Map();
  // monotonic layer: reveal (hidden->present) + the visited trail (done)
  for (const n of spec.nodes) status.set(n.id, n.hidden ? 'hidden' : 'resting');
  for (const e of spec.edges) status.set(e.id, endpointHidden(spec, e) ? 'hidden' : 'resting');
  for (let s = 0; s <= i; s++) {
    (spec.steps[s].reveal ?? []).forEach(id => { if (status.get(id) === 'hidden') status.set(id, 'resting'); });
    (spec.steps[s].activate ?? []).forEach(id => status.set(id, 'done')); // trail lit
  }
  // current-step layer (not folded): this step's actives are strongest; its dims muted
  (spec.steps[i].dim ?? []).forEach(id => status.set(id, 'dim'));
  (spec.steps[i].activate ?? []).forEach(id => status.set(id, 'active'));
  // per-step dynamic text (accumulating counts etc.) applied from the fold
  return status;
}
```

Visual hierarchy (specify it, the "lights up as you go" look depends on it):
`resting` = full muted graph; `done` = brighter than resting (traversed); `active` =
brightest + accent glow; `dim` = below resting (pushed back this step); `hidden` = absent.

Per-step **dynamic text**: nodes may carry an accumulating value (e.g. "FileIndex: 1,234
files", "violations: 7"). A `setText` step op (3.6) overrides a node's rendered value at
that step; the fold applies the last-set value. Static `label`/`sublabel` remain for
fixed text.

The clock spawns `<circle>` tokens per the step's `flow`, moves them along the edge path
over `durationMs`, staggers multiple tokens, and settles (removes them). Tokens are
ephemeral, so a seeked/settled frame never has partial token state; **clear in-flight
tokens on any manual seek, forward Next included** (not only backward). Timing uses
`performance.now()` deltas with `dt` clamped (~64ms) and accumulates **elapsed** time so
pause freezes exactly (never `now - startTime`, which jumps on resume).

Chained `edges: [...]` travel: **require consecutive edges to share an identical anchor
point** on the shared node (validated by `validateSpec`), and cache per-token the
`[path, cumulativeOffset]` list at spawn. Document whether the token passes *through* the
shared node or dwells; default = passes through. Without the shared-anchor requirement a
token teleports across the node at the seam.

### 3.4 Controls, accessibility, reduced motion

**No autoplay on connect** -- default is user-driven stepping, which satisfies WCAG 2.2.2
(Pause/Stop/Hide, Level A) because that criterion is triggered only by content that
starts *automatically* (verified). Play is an opt-in button; under reduced motion it
advances discrete snapped steps (or is disabled), never a running rAF.

- **Transport:** native `<button>`s (`aria-pressed` Play toggle) and a **native
  `<input type="range">`** for progress (free keyboard arrows/Home/End, `aria-valuenow`,
  and touch drag; avoids a hand-rolled `role=slider`). Wrapper `role="group"
  aria-label="Diagram playback"`. Keyboard `<-/->`, Space, Home/End. Ensure the
  component's global Left/Right handler does not *also* fire while the range has focus
  (double-step). Touch targets meet WCAG 2.5.8 (>=24x24).
- **The SVG has an accessible exposure:** `role="img"` on the diagram with an
  `aria-label` + `aria-describedby` one-line summary; decorative token/edge layers marked
  `aria-hidden`.
- **The numbered step `<ol>` exists in BOTH modes** (in light DOM, so it is findable,
  copyable, printable, and inherits docs typography). It is the structural, browsable map;
  the `aria-live="polite"` caption announces the *current* step. Complementary, not
  redundant (carousel status + list).
- **Reduced motion starts at step 0** (not the final fold -- starting at the end is
  disorienting), transitions disabled (states snap), token motion skipped; transport
  steps instantly. The static `<ol>` gives the full narrative without motion. Captions
  carry the flow's invariants in prose so the still frames + list are genuinely
  meaningful.

### 3.5 Theme

SVG strokes/fills use `var(--sl-color-*)` and `currentColor`; Starlight sets
`data-theme="light|dark"` on `<html>` before first paint, so recolor on toggle is
automatic with no per-frame JS (custom properties are inherited and pierce into light or
shadow DOM -- verified). **Use the real Starlight docs tokens (never hardcode a hex; the
`~#93a4fd` in an earlier draft was the marketing accent, not these docs tokens):**
- accent stroke/highlight: `var(--sl-color-text-accent)` (adaptive -- light periwinkle on
  dark, indigo on light);
- solid accent fill (with `--sl-color-white` text): `var(--sl-color-accent)`;
- background `var(--sl-color-bg)`; foreground `var(--sl-color-text)` / `--sl-color-white`;
  borders `var(--sl-color-gray-5)` / `--sl-color-hairline`.

### 3.6 Spec schema

Hand-placed coordinates + author-specified edge anchors (not auto-layout): dependency-free,
art-directed, deterministic. `fromSide`/`toSide` are **restored** (the audit flagged their
loss); `label` accepts `string | string[]` for manual line breaks; `setText` supports
per-step dynamic values.

```ts
type NodeKind = 'process'|'artifact'|'store'|'decision'|'terminal'|'group';
type Side = 'top'|'right'|'bottom'|'left';
interface AnimNode { id; x; y; w; h; label: string|string[]; kind?; sublabel?; group?; hidden?; }
interface AnimEdge { id; from; to; label?; fromSide?: Side; toSide?: Side; waypoints?: {x;y}[]; kind?:'flow'|'dep'|'back'; curve?:'smooth'|'orthogonal'|'straight'; }
interface AnimFlow { edge?; edges?; count?; spread?; reverse?; variant?; }
interface AnimStep { caption; activate?; dim?; reveal?; flow?: AnimFlow[]; setText?: {id;text}[]; durationMs?; dwellMs?; note?; }
interface AnimSpec { id; title; width; height; nodes: AnimNode[]; edges: AnimEdge[]; steps: AnimStep[]; defaults?; }
```

`validateSpec` (run on connect, and unit-tested per section 6) rejects: unknown ids in any
edge/step reference, chained edges that do not share an anchor point, and empty
nodes/steps. On failure the component renders the fallback rather than a broken SVG.

### 3.7 Edge routing (author-routed) + node text

Not auto-routing. Each edge draws from `from`'s `fromSide` anchor, through any
`waypoints`, to `to`'s `toSide` anchor, as a `curve` (`smooth` = quadratic/cubic through
the points; `orthogonal` = rounded elbows; `straight`). Default side (when a hint is
absent) is the geometric nearest, but the author is expected to set sides on any edge that
would otherwise cross a node. When several edges share a node side, distribute their
anchor points along that side by index (port offset) so they do not stack. Token motion
then follows exactly the path the author sees (3.3).

Node text does not wrap in SVG `<text>`; author labels as `string[]` for explicit
`<tspan>` line breaks, and keep `w`/`h` sized for the longest line (a QA pass checks for
overflow against the real terminology, e.g. ".alint.yml + extends sources").

### 3.8 Worked example -- the `alint check` pipeline (Phase 0)

`specs/check-pipeline.js`: ~15 nodes hand-placed (dev, alint binary, `.alint.yml`, extends
sources, facts, rule filter, parallel walk, FileIndex, dispatch, per-file rules,
cross-file rules, aggregate, emit, fixers, report), ~16 author-routed edges (with
`fromSide`/`toSide`), and ~11 captioned steps mirroring `docs/design/ARCHITECTURE.md`'s
Execution model. Captions carry the four invariants visually: **walk once** (one walk
step), **read each file once** (single file token per file), **facts + rules parallel**
(concurrent tokens), **fixers serial** (one slow token). This is the **Phase 0** diagram:
build the whole engine against it, iterate on legibility, edge routing, timing, and
authoring ergonomics, and only then freeze the schema and author the rest. Coordinates
will need visual iteration -- they cannot be validated as readable without rendering.

### 3.9 Responsive + overflow

The specs use a fixed viewBox (~1000x520); SVG scales to container width, so on a phone a
15-node diagram shrinks ~3x and labels turn illegible. Strategy: below a breakpoint wrap
the SVG in an `overflow-x: auto` scroller that preserves a minimum legible text size (with
a subtle "scroll to see more" affordance), rather than shrinking to fit. The transport bar
wraps/stacks on narrow widths; the caption wraps. This is part of Phase 0 QA, on a real
phone viewport.

### 3.10 Gotchas (established, must-honor)

- **Node scaling:** position the node **group** via the `transform="translate(x y)"`
  attribute and scale the **inner** box via CSS with `transform-box: fill-box;
  transform-origin: center` (a CSS transform replaces the presentation attribute, so use
  different elements).
- **Never animate path `d`** (SMIL banned; CSS `d` support uneven). Draw an edge in via
  `stroke-dashoffset`; move a token along the static path via `getPointAtLength`.
- **Token motion:** cache `getTotalLength()` once at render; `getPointAtLength()` per frame
  sets the token `translate`. `getPointAtLength` is universally supported;
  `offset-path` is avoided for coordinate-origin friction (its old browser gaps are now
  moot, but the friction reason stands).
- **rAF hygiene:** elapsed-time accumulation (pause freezes), clamp `dt`, pause on
  `visibilitychange` hidden + off-screen via `IntersectionObserver`, `cancelAnimationFrame`
  on disconnect. Wrap the rAF body so one bad frame cannot wedge the clock. For several
  diagrams, a single shared rAF ticker iterating active instances is preferable to N
  loops (IO-pausing offscreen ones makes either acceptable at MVP scale).

## 4. The diagrams (specs), staged

Staged to de-risk the engine before multiplying work across specs:

- **Phase 0:** engine + **`check-pipeline`** only, end-to-end (real author-routed edges,
  tokens, chained travel, controls, reduced-motion, responsive). Iterate until the look is
  right and authoring is ergonomic; this surfaces edge-routing, fold, and token issues
  once, not four times. Host section `## Execution model`.
- **Phase 1 (after freezing the schema):**
  - `walker-gitignore` -- parallel walk -> honor `.gitignore`/`.alintignore`/`ignore:`
    globs (files dropped) -> merge + deterministic sort -> lazy indices. Host `## Execution
    model` (net-new embed).
  - `dispatch-partition` -- partition rules (rule-major vs per-file); file-major loop reads
    each file once and fans out; cross-file scans the index. Host `### Dispatch flip`.
  - *(stretch)* `monorepo-scoping` -- nested `.alint.yml` + closest-ancestor `scope_filter`
    walk. Host `### Closest-ancestor scoping`.

Aesthetic target, stated honestly: **clean, legible, on-brand animated line diagrams**
(monochrome + `--sl-color-text-accent`, Cursor-style token variants file/fact/violation).
Matching Cursor's *designer-crafted* polish (custom easing, glow, token trails, bespoke
timing) is a visual-iteration cost; budget explicit iteration cycles in Phase 0 rather
than asserting "Cursor-grade" up front.

## 5. Dev-only embedding + QA

- Add the `Head.astro` dev gate (3.1).
- Embed `<alint-anim spec="..."></alint-anim>` in the **local**
  `src/content/docs/docs/about/architecture.md` in the relevant sections (verified present
  locally, with the target section anchors). Gitignored + wiped by the sync-gated build, so
  embeds are dev-only by construction; the local `astro dev` server never runs sync, so the
  hand-added embeds simply persist between runs (the local `npm run sync` is Node-broken,
  so nothing wipes them locally -- a dedicated re-apply helper is therefore unnecessary).
- QA with `astro dev` under nvm (`. ~/.nvm/nvm.sh && nvm use default`; the shell's default
  Node is too old).

## 6. Verification (end-to-end)

At `astro dev`, load `/docs/about/architecture/`, per diagram confirm: renders inline SVG;
Prev/Next/Play/Pause and the range progress work; captions update in the `aria-live`
region and the step `<ol>` is present + findable via Ctrl-F; tokens flow; concurrent
tokens read as parallel and the serial-fixer step as one slow token; light/dark recolors
with no reflow; emulated `prefers-reduced-motion` starts at step 0, no autoplay, static
`<ol>` narrative; keyboard controls + no double-step; a bad spec id renders the fallback,
not a broken SVG or a console-spamming dead clock; responsive on a phone viewport; no CLS
on load; the loader is inert on pages with no `<alint-anim>`.

**Automated tests (add now, not at graduation):** unit-test `computeFrame(spec, i)`
status maps; a Node test that asserts **every registered spec passes `validateSpec`**; a
headless smoke that each spec builds its SVG without throwing.

**Production-exclusion test (corrected).** The prior `build:no-sync` + grep check was
wrong: `build:no-sync` is bare `astro build` (skips sync), so it does NOT wipe the local
embeds, and `public/anim/*.js` copy into `dist/anim/` on any build -- so the tags survive
and the static files are always present. The correct checks:
- **Structural (primary, no build needed):** the embeds live only in the gitignored,
  sync-overwritten `architecture.md`, and the loader `<script>` is emitted only under
  `import.meta.env.DEV`. Both are inspectable facts; neither reaches a production build.
- **Full-build confirmation:** run **`npm run build`** (the sync-gated production build)
  under nvm, then grep `dist/**/*.html` only, asserting (i) no `<alint-anim` element and
  (ii) no `<script>` referencing `/anim/anim-loader.js`. (Sync wipes the embeds; the
  DEV gate omits the loader.) Caveat: the full build clones the docs bundle and runs a
  network freshness cross-check that can fail if the site's version pin is ahead of the
  bundle -- if that blocks locally, the structural check above is authoritative. Do not use
  `build:no-sync` for this proof; grepping all of `dist/` for the static `/anim/` files
  will always match and is not evidence of a leak.

## 7. Scope boundaries and graduation paths (NOT in the MVP)

- **Build-time deterministic-SVG variant** (graduation (a)): a `scripts/` Node script
  (mirroring `scripts/build-likec4.mjs`, already chained in `npm run build`) imports the
  same `specs/*.js` and the shared `anim-render.js` core and emits a static, fully-labelled
  SVG (final fold + caption list) -- a no-JS/print/GitHub rendering and a deterministic
  artifact for perf-gating. Runtime engine and build script share one render core so they
  cannot diverge.
- **Live docs** (graduation (b)): move the dev gate into `astro.config.mjs` `head[]`
  (unconditional) and author embeds in the alint repo's `docs/design/ARCHITECTURE.md` so
  they flow through the docs-bundle pipeline. Caveat (already documented for LikeC4):
  GitHub strips the custom element, so the build-time static SVG from (a) is what renders
  on GitHub, while alint.org shows the animated version.
- No auto-layout (author-routed), no LikeC4-derived specs (the C4 steps are too coarse),
  **no new npm dependencies** (dependency-free). Engine + specs are committed to alint.org
  only; nothing lands in the alint repo or the live site in this prototype.

## 8. Files

- Create in alint.org: `public/anim/{anim-loader,anim-engine,anim-render,anim-clock,
  anim-controls,anim-validate}.js`, `public/anim/anim-spec.d.ts`, `public/anim/specs/
  index.js`, `public/anim/specs/{check-pipeline,walker-gitignore,dispatch-partition,
  monorepo-scoping}.js`, plus unit tests (`*.test.mjs`).
- Edit (committed): `src/components/Head.astro` (the `is:inline` dev-gate line).
- Edit (local/dev-only, not committed): `src/content/docs/docs/about/architecture.md`
  (demo embeds).
