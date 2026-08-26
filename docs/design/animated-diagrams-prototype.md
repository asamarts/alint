# Design doc: animated-diagram engine + diagrams (spike-gated, dev-only QA)

Status: Draft (build plan). Not implemented. Revised across two adversarial audit
rounds (independent auditors + codebase verification); corrections are folded in and
noted where they changed a prior claim.
Decisions: builds on `animated-diagrams.md` (the interactive "Tier 3" track) and
ADR-0005. If it graduates, record the production decision as ADR-0016. No engine/DSL/
schema change to alint itself; all code lands in the alint.org site repo.

## 1. Context and honest framing

`animated-diagrams.md` proposed Cursor-style animated docs diagrams and recommended a
three-tier track. This plans the interactive tier as a **reusable animated-diagram
engine + a few diagrams**, embedded on `/docs/about/architecture/` in **dev/local only**
(never live) for QA.

Two framing corrections from the audit, stated up front because they reshape the plan:

- **This is a small reusable product, not a throwaway prototype.** The engine (a
  declarative schema, a validated render/animation core, transport controls, first-class
  accessibility, and two forward graduation paths) is production-shaped by design. Full
  WCAG work on a never-live artifact only makes sense as *graduation-readiness*, so it is
  deferred to **after** the go/no-go gate below, not built up front.
- **The expensive-to-be-wrong question is the aesthetic, and it must be answered first.**
  "Do Cursor-style animated diagrams actually look good and earn their maintenance in
  these docs?" is answered by *one polished diagram*, not by the engine's architecture.
  The plan therefore gates the whole engine behind a **1-day throwaway spike** (Phase -1)
  whose only deliverable is that go/no-go. Building the engine first would sink ~80% of
  the work before anyone can judge the look, creating sunk-cost pressure to ship a
  mediocre result.

Verified integration facts (evidence in the audits): alint.org is Astro + Starlight with
**no framework islands**; interactivity is **vanilla custom elements + a static loader**
(the LikeC4 pattern -- but note LikeC4 ships one *pre-bundled* file, whereas this engine
is multiple sibling ES modules that fetch each other at runtime, which is net-new here).
Dev-only rests on two independent mechanisms: the loader `<script>` is emitted only under
`import.meta.env.DEV` in `src/components/Head.astro`, and `architecture.md` is gitignored
and wiped+re-synced by `sync-from-alint.mjs` on every real `npm run build`.

## 2. Phase -1: throwaway aesthetic spike (the go/no-go gate)

Before any engine: **one hand-built inline `<svg>`** on the architecture page, at the real
target size (~1000x520), using the real Starlight `--sl-color-*` tokens, with ~30 lines of
*hardcoded* `requestAnimationFrame` token motion along *one* path and bare Prev/Next.

- No schema, no web component, no `validateSpec`, no registry, no accessibility, no
  reduced-motion, no responsive work -- none of the generality that makes the engine
  expensive. It exists only to show the *look* and the polish gap vs Cursor.
- Build it as a **single self-contained `.html` file** opened directly (not through
  `astro dev`), because `public/**` gets no HMR in Astro (section 6) -- a standalone file
  you refresh directly is the fastest iteration loop for pure look/feel.
- **Hard rule: throwaway.** Do not evolve the spike into the engine; that reintroduces the
  sunk-cost bias. Time-box ~1 day.
- **Exit gate:** an explicit decision. If the look does not clear the bar (honestly judged
  against Cursor, not a pre-lowered bar), stop here -- the engine is not authorized. If it
  clears, proceed to Phase 0 with a felt sense of the required polish.

## 3. The engine (authorized only after Phase -1 says go)

Net-new under `public/anim/` (native ES modules served verbatim), plus two committed edits
(`Head.astro` + `custom.css`; the second is required by the CLS fix). Tests live OUTSIDE
`public/` (section 6). The render core is **string-first** (below) so it is Node-testable
and shareable with the build-time graduation.

### 3.1 File layout

| Path | Role |
|---|---|
| `public/anim/anim-loader.js` | Static ES5 loader; mirror of `likec4-loader.js`. Presence-gate, mirror `data-theme`, inject `anim-engine.js` **once** (guard against N elements). |
| `public/anim/anim-engine.js` | `class AlintAnim extends HTMLElement` + guarded `customElements.define`. Lifecycle in 3.2. Types via JSDoc `@typedef` only (never a runtime `import type` -- that is a SyntaxError in an untranspiled `public/` module). |
| `public/anim/anim-render.js` | **String-first** SVG builder: `renderSvg(spec, frame) -> string` (nodes/edges/tokens layers, per-instance `<defs>` id namespacing); `computeFrame(spec, i) -> {status, text}`; the shared anchor-resolution function (3.7). No DOM APIs -> runs in Node. Runtime sets `el.innerHTML` and queries nodes for motion. |
| `public/anim/anim-clock.js` | rAF clock: `seek/next/prev/play/pause`, token spawn (cached path lengths), `getPointAtLength` motion. Step-transition logic is separated from the rAF pump so it is synchronously unit-testable (only token motion needs rAF). |
| `public/anim/anim-controls.js` | Transport bar (native `<button>`s + native `<input type=range>` scrubber, 3.4), keyboard, `aria-live` caption, the step `<ol>`. |
| `public/anim/anim-validate.js` | `validateSpec(spec)`: unknown ids; chained edges whose **resolved anchor points** (not just sides) differ (3.7). |
| `public/anim/anim-spec.d.ts` | Types (3.6). Advisory-only; note it *is* copied to `dist/anim/` (served, not executed) -- not "never shipped". |
| `public/anim/specs/index.js`, `specs/*.js` | Registry + one spec per diagram. |

Committed edits:
- `src/components/Head.astro` -- the dev gate (`is:inline` is required; Astro bundles a
  bare `<script>` and a hoisted one can silently vanish):
  ```astro
  {import.meta.env.DEV && <script is:inline src="/anim/anim-loader.js" defer></script>}
  ```
- `src/styles/custom.css` (already `customCss`-registered, present at first paint) --
  `alint-anim { display: block; min-height: <px>; }`. The CLS reservation **must** be here,
  not in the JS-injected stylesheet: an unstyled custom element is `display:inline` (zero
  box) until the deferred engine runs, so a min-height applied by the engine arrives after
  the shift it is meant to prevent.

### 3.2 Web component, lifecycle, DOM containment

`connectedCallback`: resolve the spec (`spec="id"` -> `SPECS[id]`, or inline
`<script type="application/json">`); `validateSpec()`; on failure render a visible fallback
(the step `<ol>` + a bordered error) and **never enter the rAF loop**; else set
`innerHTML` from `renderSvg(...)` into **light DOM**, build controls, init the clock.

Light DOM is chosen (over shadow DOM) so the caption + step list are real prose a reader
can Ctrl-F, select, copy, and print. But light DOM sits inside Starlight's
`.sl-markdown-content`, whose **descendant** rules (lobotomized-owl block margins; a direct
`svg { display:block; max-width:100%; height:auto }`; `a`/`li`/list styling) reach into the
component regardless of a class prefix. **Fix: put `class="not-content"` on the host** --
Starlight's official opt-out (every markdown rule excludes `:where(.not-content *)`), which
the existing shadow-DOM LikeC4 embeds never needed. The engine then supplies its own styles
for the caption/`<ol>` using `--sl-*` font/color tokens so they still read as docs prose;
because Starlight's content styles live in `@layer starlight.content`, the engine's
**unlayered** styles win automatically (there is no specificity battle). A `.alint-anim`
prefix is then only for preventing leak-*out*.

### 3.3 Animation model

One rAF scalar clock drives token-along-path motion via `getPointAtLength()` (per-path
`getTotalLength()` cached once). Discrete node/edge appearance is a function of the current
step via `computeFrame`. Per-instance clock for the MVP (a single shared ticker across
instances is a graduation optimization; do not over-build it now). Clean pause/seek/step
because appearance is recomputed, not tweened.

`computeFrame(spec, i)` returns **`{status, text}`** (the round-1 rewrite returned only
status and left `setText` as a dead comment -- fixed):

```js
function computeFrame(spec, i) {
  const status = new Map(), text = new Map();
  for (const n of spec.nodes) status.set(n.id, n.hidden ? 'hidden' : 'resting');
  for (const e of spec.edges) status.set(e.id, endpointHidden(spec, e) ? 'hidden' : 'resting');
  for (let s = 0; s <= i; s++) {                       // monotonic layer (folded)
    (spec.steps[s].reveal ?? []).forEach(id => { if (status.get(id) === 'hidden') status.set(id, 'resting'); });
    (spec.steps[s].activate ?? []).forEach(id => status.set(id, 'done'));   // visited trail
    (spec.steps[s].setText ?? []).forEach(t => text.set(t.id, t.text));     // last-set wins
  }
  (spec.steps[i].dim ?? []).forEach(id => status.set(id, 'dim'));           // current-step layer
  (spec.steps[i].activate ?? []).forEach(id => status.set(id, 'active'));   // active > dim if both
  return { status, text };
}
```

Semantics to state in the doc so authors are not surprised: `reveal` and the visited/`done`
trail are **monotonic**; `active` and `dim` are **single-step** -- a node dimmed at step 4
returns to `done`/`resting` at step 5 unless step 5 re-lists it, so **`dim` must be repeated
to sustain a mute** (persistent de-emphasis is out of scope; revisit with a folded
`mute`/`unmute` pair only if authoring proves it necessary). `activate` implicitly reveals a
still-hidden node. Visual hierarchy: `resting` < `done` < `active`(+accent glow); `dim`
below `resting`; `hidden` absent. `setText` overrides a node's rendered value; static
`label`/`sublabel` are the default.

Tokens are ephemeral: cleared on **any** manual seek (forward `Next` included, not only
backward). Timing accumulates **elapsed** time (pause freezes exactly; never
`now - startTime`) with `dt` clamped ~64ms. Chained `edges:[...]` travel: consecutive edges
must share the **resolved anchor point** on the shared node -- validated against the same
anchor-resolution function the renderer uses (3.7), not merely "same side", or port-offset
distribution places the seams apart and the token teleports.

### 3.4 Controls, accessibility, reduced motion (graduation-readiness; built after go)

**No autoplay on connect** (satisfies WCAG 2.2.2, which triggers only on auto-starting
motion). Under `prefers-reduced-motion` Play is **disabled** (snapped auto-advance still
updates content automatically and can re-trigger 2.2.2); the user steps manually.

- Transport: native `<button>`s + a native `<input type=range>`. The scrubber is
  **step-granular** (`min=0 max=N-1 step=1`; arrows = prev/next step) -- not continuous time,
  which would fight the clear-tokens-on-seek rule -- and exposes **`aria-valuetext`**
  ("Step 5 of 11: <caption>"), since a bare `aria-valuenow` announces only "5". Wrapper
  `role=group aria-label`; keys arrows/Space/Home/End; ensure the global arrow handler does
  not double-fire while the range has focus; touch targets >=24x24 (WCAG 2.5.8).
- The SVG carries `role="img"` + a static `aria-label` summary; decorative token/edge layers
  `aria-hidden`. Because `role="img"` gives a **static** accessible name, all per-step
  semantics live in the `aria-live="polite"` caption and the step `<ol>`.
- The numbered step `<ol>` exists in **both** modes (light DOM: findable, copyable,
  printable). Reduced motion **starts at step 0** (not the final fold), transitions
  disabled, token motion skipped.

### 3.5 Theme

SVG uses `var(--sl-color-*)` / `currentColor`; `data-theme` on `<html>` recolors with no
per-frame JS (custom properties inherit into light DOM -- verified). Use the real Starlight
docs tokens (never hardcode; the earlier `~#93a4fd` was the marketing accent):
`var(--sl-color-text-accent)` (adaptive accent), `var(--sl-color-accent)` (solid fill with
`--sl-color-white` text), `var(--sl-color-bg)`, `var(--sl-color-text)`,
`var(--sl-color-gray-5)` / `--sl-color-hairline` (borders).

### 3.6 Spec schema

Hand-placed coordinates + author-specified edge anchors (not auto-layout).

```ts
type NodeKind = 'process'|'artifact'|'store'|'decision'|'terminal'|'group';
type Side = 'top'|'right'|'bottom'|'left';
interface AnimNode { id; x; y; w; h; label: string|string[]; kind?; sublabel?; group?; hidden?; }
interface AnimEdge { id; from; to; label?; fromSide?: Side; toSide?: Side; port?: string; waypoints?: {x;y}[]; kind?:'flow'|'dep'|'back'; curve?:'smooth'|'orthogonal'|'straight'; }
interface AnimFlow { edge?; edges?; count?; spread?; reverse?; variant?; }
interface AnimStep { caption; activate?; dim?; reveal?; flow?: AnimFlow[]; setText?: {id;text}[]; durationMs?; dwellMs?; note?; }
interface AnimSpec { id; title; width; height; defaults?: {durationMs?; dwellMs?; curve?: 'smooth'|'orthogonal'|'straight'; kind?: NodeKind}; nodes: AnimNode[]; edges: AnimEdge[]; steps: AnimStep[]; }
```

`group` nodes render at **lowest z-order** (a labelled backdrop behind their members); a
member references its container via `group`. `defaults` supplies per-spec fallbacks for
`durationMs`/`curve`/`kind`. Chained edges may pin a shared `port` on the seam node so
port-offset distribution keeps their anchor identical. A **small, complete fixture spec**
(a handful of nodes/edges/steps exercising every field: `fromSide`/`toSide`/`waypoints`,
`setText`, `dim`+`activate`+`reveal`, a chained `edges:[...]` flow) lives beside the tests
as the schema-coherence proof and test fixture -- deliberately NOT the full ~15-node
`check-pipeline` coordinate dump, which cannot be validated as readable without rendering
and would be instantly stale.

### 3.7 Edge routing (author-routed) + shared anchor resolution

Each edge draws from `from`'s `fromSide` anchor, through `waypoints`, to `to`'s `toSide`,
as `curve`. When several edges share a node side they are distributed by index (port
offset); an explicit `port` pins a specific anchor. A **single `resolveAnchor(edge, node,
side)` function** is used by both `validateSpec` and `anim-render` so validation checks the
same point the renderer draws (this is what makes the chained-edge continuity check real).
Default side (absent a hint) is the geometric nearest, but authors set sides on any edge
that would otherwise cross a node. Node labels are `string[]` for explicit `<tspan>` line
breaks (SVG `<text>` does not wrap); size `w`/`h` for the longest line and QA for overflow
against real terminology.

### 3.8 The diagrams, staged (after go)

- **Phase 0:** engine + **`check-pipeline`** only, end-to-end, iterating on legibility,
  routing, timing, ergonomics -- surfacing schema/behavior issues once, not four times.
  Host `## Execution model` (heading `:338`).
- **Phase 1 (after freezing the schema):** `walker-gitignore` (walk -> gitignore/alintignore
  filtering -> deterministic sort -> lazy indices; `## Execution model`), `dispatch-partition`
  (rule-major vs per-file, read-once fan-out; heading `### Dispatch flip + PerFileRule
  (v0.9.3)`, `:81`), and *(stretch)* `monorepo-scoping` (`### Closest-ancestor scoping
  (scope_filter:, v0.9.6+)`, `:304`). Use the exact current heading slugs; they are
  bundle-synced and can drift.

Aesthetic bar, honestly: **clean, legible, on-brand animated line diagrams** (mono +
`--sl-color-text-accent`). Matching Cursor's designer-crafted polish is a real
visual-iteration cost that the Phase -1 spike measures before this is authorized.

### 3.9 Responsive + overflow

Fixed viewBox scales down on phones until labels are illegible. Below a breakpoint wrap the
SVG in an `overflow-x: auto` scroller preserving a minimum legible text size (with a
"scroll to see more" affordance) rather than shrinking to fit; the transport bar and caption
wrap. Part of Phase 0 QA on a real phone viewport.

### 3.10 Gotchas

Node scaling: position the group via the `transform` attribute, scale the inner box via CSS
`transform-box: fill-box; transform-origin: center` (CSS transform replaces the attribute).
Never animate path `d` (draw edges via `stroke-dashoffset`; move tokens along a static
path). Cache `getTotalLength()` at render. rAF: elapsed-time accumulation, `dt`-clamp,
pause on `visibilitychange` hidden + off-screen via `IntersectionObserver`,
`cancelAnimationFrame` on disconnect, wrap the rAF body so one bad frame cannot wedge the
clock.

## 4. Browser + authoring notes

- **Evergreen-only.** `public/anim` ships **untranspiled** (no `vite.build.target`, no
  browserslist for `public/`); target ES2020+. Revisit at graduation (b) only if analytics
  show meaningful old-browser docs traffic.
- **Spec captions escape the site's prose gates.** `alint check`'s em-dash/curly-quote rules
  target `.astro`/`.md`/`.mdx` + `llms.txt`, not `public/anim/specs/*.js`, so an em-dash in a
  caption sails through. Author captions to the same no-em-dash discipline manually.
- **Typing posture:** JSDoc `@typedef` (the `tsconfig` sweeps `public/` under strict, but no
  `astro check` CI gate exists, so this is for editor sanity only).

## 5. Dev-only embedding + QA

Add the two committed edits (3.1). Embed `<alint-anim spec="...">` in the **local**
`architecture.md` (present, gitignored, wiped by the sync-gated build). The local `astro
dev` server never runs sync and the local `npm run sync` is Node-broken, so hand-added
embeds simply persist. **`public/**` has no HMR in `astro dev`** -- after editing
`anim-engine.js`/specs, hard-refresh (or dev-inject the engine URL with a `?v=` cache-bust);
this asymmetry (content hot-reloads, `public/` does not) is a reason the Phase -1 spike is a
standalone HTML file. QA under nvm (`. ~/.nvm/nvm.sh && nvm use default`).

## 6. Verification

**Tests (out of `public/`, wired to CI).** Put them beside the existing
`src/lib/nav.test.mjs` (or `tests/anim/`) -- NOT under `public/`, where they would copy to
`dist/` and be served at `https://alint.org/anim/*.test.mjs`. Add a `test:anim` script and a
CI step (fold into an existing workflow; the only wired test today is `test:nav`). Run under
nvm (Node >=18 for `node --test`). Coverage: `computeFrame(spec, i)` `{status, text}` maps
(including text after a backward seek); `validateSpec` (unknown ids; chained-edge resolved-
anchor mismatch); clock transitions (`seek` empties the token layer; `next`/`prev` clamp;
`play`->`pause` preserves elapsed with a mocked `performance.now`; reduced motion never
enters rAF); and -- enabled by the string-first render -- a `renderSvg()` smoke that every
registered spec builds a non-empty SVG string without throwing (this needs no DOM/jsdom).

**Manual QA** at `astro dev`, `/docs/about/architecture/`: renders; transport + step
scrubber + `aria-valuetext`; captions in the live region and a findable step `<ol>`; tokens
flow, parallel reads as concurrent, serial-fixer as one slow token; light/dark recolor;
reduced motion starts at step 0 with Play disabled; keyboard, no double-step; bad spec ->
fallback not a dead clock; responsive on a phone; no CLS on load.

**Production-exclusion.** Primary = structural: embeds live only in the gitignored/sync-
wiped `architecture.md`, and the loader `<script>` is DEV-gated. Confirmation = full
`npm run build` (the sync-gated production build) under nvm, then grep `dist/**/*.html`
only for (i) no `<alint-anim` element and (ii) no `/anim/anim-loader.js` reference. **Do
not use `build:no-sync`** for this proof -- it skips sync, so its `dist` HTML still contains
the (inert) embed tags; a `build:no-sync` output must never be deployed. Note the engine's
static `dist/anim/*.js` (+ the `.d.ts`) are present-but-unreferenced on any build and are
crawlable in prod (robots `Allow: /`, AI bots allowed); keep the shipped footprint minimal,
keep tests out of `public/`, and optionally add a build assertion that fails if `dist/anim`
is present without a graduation flag.

## 7. Scope boundaries and graduation paths (NOT in the MVP)

- **Build-time deterministic-SVG variant** (graduation (a)): a `scripts/` Node script imports
  the same specs and the **string-first** `renderSvg` and emits a static, fully-labelled SVG.
  Because `getPointAtLength`/rAF are browser-only, the shared core is precisely the
  string-emitting structure/frame builder; token motion + the clock are runtime-only. The
  "cannot diverge" guarantee therefore covers the **static** half only -- do not over-claim
  it.
- **Live docs** (graduation (b)): move the dev gate into `astro.config.mjs` `head[]` and
  author embeds in the alint repo's `docs/design/ARCHITECTURE.md`. GitHub strips the custom
  element, so the graduation-(a) static SVG is what renders there; alint.org shows the
  animation. Revisit the browser baseline here.
- No auto-layout, no LikeC4-derived specs, no new npm dependencies. Engine + specs committed
  to alint.org only.

## 8. Files

- Create in alint.org: `public/anim/{anim-loader,anim-engine,anim-render,anim-clock,
  anim-controls,anim-validate}.js`, `public/anim/anim-spec.d.ts`, `public/anim/specs/
  index.js`, `public/anim/specs/*.js`; tests under `src/lib/anim/` or `tests/anim/`
  (`*.test.mjs`, NOT under `public/`); the Phase -1 spike as a standalone `.html` (throwaway).
- Edit (committed): `src/components/Head.astro` (the `is:inline` dev gate) and
  `src/styles/custom.css` (the eager `alint-anim { display:block; min-height }` CLS
  reservation).
- Edit (local/dev-only, not committed): `src/content/docs/docs/about/architecture.md`.
