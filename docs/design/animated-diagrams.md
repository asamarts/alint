# Design doc: animated docs diagrams (Cursor-style, without Cursor's cost)

Status: Draft (proposal). Not implemented.
Decisions: builds on ADR-0005 (adopt LikeC4 for architecture diagrams) and
`architecture-diagrams.md`. If the primary track below is adopted, record it as
ADR-0016. No engine/DSL/schema change; this is a docs-rendering + docs-export
tooling proposal.

## 1. Problem

We want several *animated* technical/architecture diagrams across the docs, in the
spirit of [cursor.com/blog/git-at-any-scale](https://cursor.com/blog/git-at-any-scale):
polished, motion-driven explanations of a pipeline or data flow that a reader can
watch or step through. The obvious candidates are flows we already document in prose
or as static pictures: the `alint check` execution pipeline, the file walker +
gitignore filtering, the fix/remediation flow, config resolution + `extends`, and
monorepo nested-config layering.

Two facts frame the work:

1. We already ship LikeC4 (ADR-0005): one text-DSL model, ~40 views, embedded as
   `<likec4-view>` web components, gated for drift. Its **dynamic views already
   animate step by step** — each is an ordered `source -> target 'label'` sequence
   (with `parallel {}` blocks), which is literally an animation timeline. So we own
   the *spec*; what we lack is engaging *rendering*. The stock LikeC4 renderer is a
   generic ~2.5 MB interactive web component whose step-through is off by default
   inline, not an autoplay narrative.
2. We just removed four low-value static diagram embeds (concepts index, fix-ops,
   content-from, installation) because a diagram that neither animates nor teaches is
   clutter. The bar for *adding* motion is therefore high: it must teach, be cheap to
   maintain at the scale of "several," and not fight alint's ethos.

That ethos is the real constraint. alint's docs are **code-as-source-of-truth**:
generated from a spec and gated byte-stably (`gen-*-check`, `docs-export --check`,
`likec4 validate`). Whatever we adopt has to keep that property, survive the
docs-bundle markdown sync, ship accessibly, and be editable by an engineer editing
text — not a designer in a GUI.

## 2. What Cursor actually does (investigated)

We pulled the live page and its shipped JS/CSS chunks and confirmed the stack from
the bundle (not from commentary):

- **Next.js 16.2 (RSC/Turbopack/pnpm) + React + Tailwind.**
- **Framer Motion** (the `motion` package — `MotionValue`, `AnimatePresence`,
  `whileHover/whileTap/layoutId`, `anticipate`/`backInOut` easings) for declarative
  transitions and micro-interactions.
- A **hand-rolled `requestAnimationFrame` loop** for the actual simulation clock
  (elapsed ms, speed multiplier, in-flight packet motion, latency).
- **d3 primitives** (`scaleLinear`/`scaleBand`/`line`/`area`/`ticks`/`format`) used
  as a *math library* to compute **bespoke inline SVG** charts — there is no charting
  library (no recharts/visx/nivo).
- **Hand-coded, data-driven React + SVG** per diagram. The simulator SVG is built
  procedurally in JSX (redrawn when you change replica count/latency), so it is *not*
  a Figma → SVGR export (no SVGR signature anywhere). State is a hand-rolled reducer +
  `phase` index + playback transport (`Back`/`Pause`/`Next`/`Play`), not xstate.

Definitively **not** used: Rive, Lottie/bodymovin, `<canvas>`/WebGL, SMIL, GSAP,
react-spring. The one `.mp4` on the page is a product demo, not a diagram.

**The lesson.** Cursor's diagrams are gorgeous because each is a small bespoke app:
a parameterized state machine plus ~150+ hand-placed, data-driven SVG primitives
wired to an animation clock and adjustable inputs — dozens to a few hundred lines of
custom React/SVG/d3 each. There is a shared chart component and a shared transport
control, but the simulation logic is per-diagram. That is a deliberate
design-engineering investment that pays off for a flagship post. It does **not**
scale cheaply to "several diagrams on a small team," and its authoring model
(hand-coded React components, no regenerable text artifact) is the opposite of
alint's generate-and-gate docs. The *technique* — inline SVG + a small state machine
+ a motion library — is reproducible and worth borrowing for at most one or two hero
diagrams; the *approach as a default* is not the right fit.

## 3. The constraints that decide this for alint

Four constraints, in priority order, filter the option space hard:

1. **Deterministic, gate-able artifact.** We gate generated docs byte-stably. This
   favors approaches whose *source is text* and whose *output is either that text or
   a byte-stable regeneration*: the LikeC4 DSL, CSS/SVG generated from a spec, D2 (if
   pinned), Mermaid (only with `deterministicIds: true`). It **hard-rejects** opaque
   binary GUI exports: Rive `.riv`, dotLottie ZIP, Motion Canvas mp4.
2. **Survives the docs-bundle `.md` sync.** The `/docs/*` pages are synced markdown +
   assets built from a release tag (see `architecture-diagrams.md`, docs-bundle.yml).
   What passes through cleanly:
   - **Raw inline `<svg>` + `<style>`** in `.md` — renders as-is.
   - A **custom element** (`<likec4-view>`) — works because its defining script is
     registered in the site layout (`likec4-loader.js`), not in the markdown.
   - What does **not**: a **framework island** (`client:*`) requires `.mdx`, not plain
     `.md`; and a raw `<script>` in `.md` is *not* run through Astro's bundler. So
     anything needing an island must be a **site-side Astro component**, not something
     the synced markdown carries.
3. **Code-as-SoT, small team.** Favor "an engineer edits text and regenerates" over
   "a designer opens a GUI." This is the axis on which Rive/Lottie/Motion-Canvas lose
   hardest.
4. **Accessibility throughline.** WCAG **2.2.2 Pause/Stop/Hide is Level A**: motion
   that starts automatically, lasts > 5 s, and sits beside content must be
   pausable/stoppable. Only **CSS** animation is gated *declaratively* by
   `prefers-reduced-motion`; SMIL, canvas/wasm, Lottie, Rive, and D2's animated SVG
   all need a manual `matchMedia` gate + static fallback. Net: **user-driven
   step-through (LikeC4 walkthrough, Cursor-style step buttons) is inherently the
   safest** — nothing autoplays, so 2.2.2 does not apply.

## 4. Options

Dimensions: authoring · Astro render · runtime cost · deterministic & gate-able ·
reduced-motion · fits synced `.md`.

| Approach | Authoring | Render | Runtime | Gate-able? | Reduced-motion | Fits synced `.md`? |
|---|---|---|---|---|---|---|
| **LikeC4 dynamic views** (MIT) | Text DSL we already use | `<likec4-view>` tag (+ optional React island for inline walkthrough) | Heavy interactive bundle — but **already shipped** | **Yes** (gate DSL + model, as today) | **Best** — user-driven stepping, no autoplay | **Yes** via tag; inline walkthrough needs a site-side island |
| **Generated CSS-animated inline SVG** | **Generate from a spec** (best alint fit) | Raw inline `<svg>`+`<style>` in `.md` | **Zero JS** | **Yes** — the animation *is* the text artifact | **Native** (`@media (prefers-reduced-motion)`) | **Yes**, cleanest — pure text through Markdown |
| **D2 `--animate-interval`** (MPL-2.0) | Text DSL | Inline `<svg>` | Zero JS | Yes, if version+fonts pinned; sketch mode not byte-stable | **None native** → manual gate; autoplay > 5 s trips WCAG 2.2.2 A | Yes, but a *second* toolchain + non-C4 idiom |
| **Mermaid animated edges** | Text DSL | Pre-render SVG at build | Zero JS if pre-rendered | Only with `deterministicIds: true` | Add media query yourself | Yes; edge-only "marching ants," not stepped pipelines |
| **`motion` / anime.js island** (MIT) | Code (JS) | Vanilla `<script>` (risky in `.md`) or island | 2.3 KB vanilla `animate()` … ~34 KB full | Engine, not an artifact — gate your SVG + code | `useReducedMotion()` / manual | Island → needs `.mdx`/site component |
| **GSAP + ScrollTrigger** | Code (JS) | Island/script | ~40 KB gz | Engine, not an artifact | `gsap.matchMedia` | Island caveat; **license is proprietary "no-charge," not OSI** |
| **SVG SMIL** | Hand/generated SVG | Inline `<svg>` | Zero JS | Yes (declarative text) | **No native** reduced-motion → needs JS | Yes, but the a11y gap makes CSS preferable |
| **Rive** | **GUI editor only** (`.riv` binary) | `<canvas>`+JS / island | **~750 KB wasm** | **No** (opaque binary) | Manual + fallback | **No** |
| **Lottie** | AE/Rive → JSON (`.lottie` = ZIP) | web component / island | 46–75 KB JS, or ~510 KB wasm | Weak (AE export not byte-stable; ZIP blob) | **No built-in** | Weak for architecture diagrams |
| **Motion Canvas** | TS generator + editor | mp4 `<video>` or JS-bundle player | Video or full bundle | **No** (binary / opaque bundle) | Manual | Poor |

Notes that move the decision:

- **LikeC4 is animatable in-track, with one embedding wrinkle.** Dynamic views are
  first-class; v1.59 added `parallel`/`opt`/`loop`/`alt`/`try` flow-control blocks and
  `diagram`/`sequence` variants. The interactive diagram animates transitions
  (it is built on `@xyflow/react` + `motion`). The **inline animated walkthrough** is a
  real prop, `enableDynamicViewWalkthrough`, but it **defaults to `false`**, and
  LikeC4's own Astro docs enable it via a **React island** (`client:only="react"`),
  with the walkthrough on in the popup "browser" but off inline. So the bare
  `<likec4-view>` tag (what survives synced `.md`) gives the stepped dynamic diagram
  plus a browser-popup walkthrough; the *inline* autoplay walkthrough needs a
  site-side island. This is the one thing to spike before committing (below).
- **Generated CSS-animated SVG is the most alint-native new capability.** It mirrors
  our existing `gen-X --check` artifacts exactly: emit inline `<svg>` + `<style>` from
  a deterministic generator (stable IDs, LF, fixed number formatting), commit/gate the
  text, drop it straight into synced markdown. Zero runtime JS, native
  `prefers-reduced-motion`, byte-stable. Gotchas: use `transform-box: fill-box` for
  element transforms; CSS can't animate a path's `d` (use SMIL only where morphing is
  essential, behind a JS reduced-motion gate).
- **License flag.** GSAP is free since 3.13 but its license is a proprietary
  "no-charge" grant (not OSI, bans "competitive products," terminable). For an OSS
  code-as-SoT project, prefer MIT engines (`motion` v13, anime.js v4) if we ever build
  an island. And prefer Solid/Svelte over React for a *new* island (few-KB runtime vs
  ~40 KB) — though note we already ship a React-class bundle for LikeC4.

## 5. Recommendation: a three-tier track

Adopt a tiered strategy, weighted to the cheap high-value tiers. This gets "several"
animated diagrams for near-zero new tooling and reserves bespoke effort for the one
or two places interactivity actually teaches.

**Tier 1 (primary — do this for the several pipeline diagrams): LikeC4 dynamic
views.** Model the check pipeline, walker + gitignore, fix flow, config resolution +
extends, and monorepo nested-config layering as `dynamic view`s in the model we
already gate. We get stepped, animated, user-navigable diagrams from one SSOT, in the
same C4 idiom as our 40 existing views, MIT, with the best accessibility posture
(user-driven stepping). Several of these already exist as dynamic views (checkFlow,
fixFlow, walkerFlow, monorepoNesting) — the work is authoring the missing ones and
resolving the inline-walkthrough embedding (spike 1). Beyond authoring, the only new
work is that embedding decision.

**Tier 2 (complement — pure-markdown flourishes): generated-and-gated CSS-animated
inline SVG.** For small, always-on motions embedded *directly* in synced markdown — a
token flowing along a pipeline edge, a file traversing the walker, a pack "arriving" —
emit inline `<svg>` + `<style>` from a deterministic generator and gate the text like
any `gen-X` artifact. Ideally the generator consumes the **same LikeC4 dynamic-view
step data** we already maintain (`A -> B 'label'` order becomes the keyframe
timeline), so there is one source of truth and no divergent hand-drawn SVG. Zero JS,
declarative reduced-motion, byte-stable, drops into `.md`. Keep loops subtle and short
to stay clear of WCAG 2.2.2.

**Tier 3 (reserved — one or two hero "explorable" diagrams): inline SVG + a small
non-React island.** Where a *parameter* genuinely teaches — e.g. monorepo config
layering with a "depth" slider, or the walker with a toggle for gitignore vs
git-tracked — build one Cursor-style component: inline SVG + a small state machine
(step index + slider params) + an MIT motion engine (`motion` vanilla, or
`solid-motionone`). Because islands can't live in synced `.md`, build it as a
first-class **site-side Astro component** (or render that one page as `.mdx`), fed
SVG/data from the docs bundle. Highest effort and weakest determinism (we gate the
committed SVG + component code, not a regenerated artifact), so do **not** scale it to
all five — use it only where interactivity is the point.

**Explicitly avoid for the gated-docs core:** Rive, Motion Canvas, Lottie for
architecture diagrams, Snap.svg (abandoned), GSAP (license), and autoplaying GIF/APNG.
They produce opaque binaries that can't be gated, impose GUI/designer authoring
against code-as-SoT, ship heavy wasm, or can't be paused for accessibility. **D2
`--animate-interval`** is the one near-miss: a single self-contained autoplaying
animated SVG from a text DSL, deterministic if pinned — acceptable *only* if we ever
specifically want an auto-looping "watch it flow" SVG, and only with a manual
reduced-motion gate + WCAG-2.2.2 pause affordance. Not worth a second toolchain for
the general case.

## 6. Implementation sketch

Phased, smallest-useful-first:

- **Phase 0 — spike the LikeC4 inline walkthrough (spike 1).** Decide whether
  `<likec4-view>`'s stepped diagram + browser-popup walkthrough is "animated enough,"
  or whether we add a site-side `DynamicLikeC4View`-style island (LikeC4's own
  reference embed) for the inline autoplay walkthrough. This decides whether Tier 1
  stays 100% pure synced markdown. Small, site-side, reversible.
- **Phase 1 — Tier 1 authoring.** Add/refine the dynamic views for the target flows in
  `docs/design/architecture/model/*.c4`; they flow through `docs-export` and the
  existing `likec4 validate` + `gen-mermaid --check` gates unchanged. Re-embed on the
  relevant pages (the ones we just de-cluttered) only where the animation teaches.
- **Phase 2 — Tier 2 generator.** Add a small `xtask` step (or a `gen-anim-svg`
  target) that renders a chosen dynamic view's step sequence to a byte-stable
  CSS-animated inline SVG (stable IDs, LF, fixed number format), committed and gated
  with a `--check` variant like the other `gen-*` artifacts. Wire it into the
  docs-bundle so it survives the sync. Ship a static first-frame under
  `@media (prefers-reduced-motion: reduce)`.
- **Phase 3 (optional) — one Tier 3 hero.** Only if a parameter-driven explorable is
  wanted; build it site-side with an MIT engine and gate the committed SVG + code.

Determinism is the acceptance test throughout: every generated artifact must survive a
`--check` regen (matching the `gen-X --check` pattern the repo already enforces), or
it does not ship.

## 7. Accessibility

Non-negotiable, folded into every tier:

- Every animation ships with a **static fallback** and honors
  `prefers-reduced-motion`. CSS (Tier 2) gets this declaratively; anything
  JS/SMIL/wasm (Tier 3, or a D2 SVG) needs an explicit
  `window.matchMedia('(prefers-reduced-motion: reduce)')` gate that renders a static
  frame.
- **Prefer user-triggered playback** (LikeC4 walkthrough, Tier 3 step buttons) over
  autoplay — it sidesteps WCAG 2.2.2 Level A entirely.
- If anything autoplays and can run > 5 s beside text, it **must** offer
  pause/stop/hide (WCAG 2.2.2, Level A). The documented swap pattern is
  `<picture>`/`<source media="(prefers-reduced-motion: reduce)">` to serve a still.

## 8. Open questions / spikes

1. **LikeC4 inline walkthrough (blocks Tier 1 embedding choice).** Is
   `<likec4-view browser>`'s popup walkthrough sufficient, or do we want the inline
   React-island walkthrough (site-side `.astro`/`.mdx`)? Resolve in a spike; it decides
   whether the animated tier stays pure synced markdown.
2. **Tier 2 generator determinism.** Confirm the emitted CSS-animated SVG is byte-stable
   across the CI toolchain (IDs, number formatting, LF) so it can be `--check`-gated
   like `gen-schema`/`gen-arch`.
3. **Tier 3 island weight (only if we build one).** Solid vs Svelte vs vanilla
   `motion`: measure the added island weight against the already-shipped LikeC4 React
   bundle, and confirm the site-side component path (vs `.mdx`) for a page whose body
   is otherwise synced markdown.

Resolve each inline with an editorial note when the work lands.

## 9. Sources

Investigated: [Cursor: Git at any scale](https://cursor.com/blog/git-at-any-scale)
(stack confirmed from its shipped bundle: Next.js 16.2 + React + Framer Motion + a
hand-rolled rAF loop + d3-fed bespoke SVG; no Rive/Lottie/canvas/SVGR). Technique
precedents: [Red Blob Games](https://www.redblobgames.com/) (vanilla JS + SVG, with
"how I make these" writeups), [Josh Comeau](https://www.joshwcomeau.com/) (React-in-MDX).
LikeC4: [dynamic views](https://likec4.dev/dsl/views/dynamic/),
[web component](https://likec4.dev/tooling/code-generation/webcomponent/),
[React embed](https://likec4.dev/tooling/code-generation/react/) (`enableDynamicViewWalkthrough`
defaults off; docs embed as a `client:only` island). Alternatives:
[D2 `--animate-interval`](https://d2lang.com/tour/exports),
[Mermaid `deterministicIds`](https://mermaid.js.org/config/schema-docs/config.html),
[motion.dev](https://motion.dev/docs/quick-start) (MIT),
[anime.js v4](https://github.com/juliangarnier/anime) (MIT),
[GSAP license](https://gsap.com/standard-license) (proprietary),
[Rive web](https://rive.app/docs/runtimes/web/web-js) (~750 KB wasm),
[Motion Canvas](https://github.com/motion-canvas/motion-canvas). Platform:
[Astro islands](https://docs.astro.build/en/concepts/islands/),
[Markdown](https://docs.astro.build/en/guides/markdown-content/) /
[MDX](https://docs.astro.build/en/guides/integrations-guide/mdx/) /
[client scripts](https://docs.astro.build/en/guides/client-side-scripts/) (islands need
`.mdx`; raw `<script>` in `.md` is not bundled). Accessibility:
[WCAG 2.2.2 Pause/Stop/Hide (A)](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html),
[MDN prefers-reduced-motion](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion),
[web.dev prefers-reduced-motion](https://web.dev/articles/prefers-reduced-motion).
