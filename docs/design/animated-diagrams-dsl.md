# Design doc: animated-diagram DSL, engine foundation, and packaging

Status: Analysis + recommendation. Not implemented. Answers three questions raised
after the Phase -1 aesthetic spike cleared its look bar: (1) is the spike already a
declarative DSL or is everything hand-built, (2) should we build a rendering engine or
adopt one, and (3) should this live in a separate library/package/repo. Backed by three
independent research passes -- a current-state gap analysis of the spike, an ecosystem
build-vs-adopt scan, and a packaging analysis.
Decisions: extends `animated-diagrams.md` (the three-tier proposal) and
`animated-diagrams-prototype.md` (the build plan). Confirms **build, not adopt**; a
**dependency-free native** foundation; and an **in-site module** (no separate
package/repo now) with staged extraction criteria. Folds a **DSL-formalization** pass
into the prototype's Phase 0. No change to alint itself; all code lands in alint.org.

## 1. Context: the three questions

The spike (`spike-artifact.html`) cleared the aesthetic gate: three vertical,
autoplaying, theme-aware diagrams with flowing tokens, arrival glow, in-place value
changes, and provenance traces. That raised the right strategic questions before the
real engine gets built:

1. Is everything manually built and animated, or is there already a declarative model we
   should formalize into a proper DSL?
2. Is there an opportunity to define diagrams declaratively and render off a spec -- and
   does an existing tool already do this well enough to adopt?
3. Would it make sense to build this as a separate library / package / tool / repo?

This doc answers each, then proposes the concrete formalized grammar. The headline: we
already have ~40% of a DSL, no existing tool fits the constraints, and the right home is
inside alint.org with clean boundaries for a *later, conditional* extraction.

## 2. Q1 -- The spike is already a declarative DSL (about 40% formalized)

Nothing in the spike is hand-animated frame by frame. There is a declarative data model
(`DIAGRAMS[id]`) interpreted by a generic engine (`build` / `frameFor` / `applyClasses`
/ `go` / `tick`). A diagram is data:

```
{ title, w, h, sections[], nodes[], edges[], chips[], traces[], steps[] }
```

and a step is a list of "verbs" (`on`, `reveal`, `move`, `flow`, `drip`, `trace`,
`markGood`, `markHot`, `markDrop`, `setText`, `setLevel`, `count`). The engine folds
steps `0..i` into a frame and applies it. So the model is genuinely declarative for
diagram *content*.

What is **not** yet formalized (why it is ~40%, not ~90%):

- **Engine mechanics are ~60% done; formalization is ~25-30% done.** The rAF clock,
  ease/lerp, token-along-path (`getPointAtLength`), edge draw-in, the step fold, and
  side-anchored edge routing all work. But there are no types, no `validateSpec`, no
  tests, and the vocabulary is undocumented.
- **The vocabulary is sprawling and redundant.** Three token paths (`flow` over an edge,
  `drip` over a straight coord-lerp, `trace` over a hand-tuned curve) are one primitive.
  `move` and `reveal` are two front-ends to the same position+opacity animator. `markGood`
  / `markHot` / `markDrop` are one state verb -- with an *inconsistent, undocumented*
  folding rule (good/drop accumulate across steps; hot rebuilds per step). `nodes` vs
  `chips` vs `sections` vs `traces` are four element buckets that could be one. There is
  dead code (`drip`, `setText`, `REPO`, `fileGrid` -- defined, never called) and a
  vestigial node kind (`t`).
- **All geometry is hand-placed literals.** Every `x,y,w,h`, every trace `from/to`, every
  `move` target is eyeballed; char widths are magic constants (`6.6`, `6.3`); trace
  control points are four tuned offsets. There is no auto-layout (edge *routing* from node
  sides is the one auto part).
- **The committed schema has already drifted from the implementation.** The prototype doc
  committed an `AnimSpec` / `AnimNode` / `AnimStep` schema, but the spike and that schema
  are two shapes of the same idea, neither a superset:
  - The spike *gained* verbs the committed schema dropped: positional `move` (walker's
    files flying into the index), animated `count`, `setLevel` (the `error -> warning`
    strike diff), and the semantic `good` / `hot` / `drop` colors.
  - The committed schema *has* what the spike lacks: types + `validateSpec`, edge
    waypoints/curve, per-spec `defaults`, a string-first (Node-testable) renderer,
    reduced-motion, and real accessibility.
- **Reduced motion is currently absent.** The spike computes `matchMedia` and never uses
  it (the gate was removed so we could evaluate the animation); it always autoplays and
  loops. This must be restored for the real engine.

Conclusion: the work is **formalization + reconciliation, not a rewrite**. The proof of
concept is done; the schema, validation, tests, reduced-motion, and a clean grammar are
not. Section 5 proposes that grammar.

## 3. Q2 -- Build, do not adopt; on a dependency-free native foundation

The ecosystem scan evaluated ~18 tools against the full constraint conjunction: tiny +
framework-free + inline SVG + discrete tokens flowing along edges + in-place value change
with strike-through + CSS-custom-property theming + `prefers-reduced-motion` + pixel-level
hand control (the Starlight no-islands rule and the ~2KB loader budget are hard limits).

**No tool clears the bar; each fails at least one hard constraint:**

- **Motion Canvas** -- the closest single "adopt" candidate; can produce the exact look in
  code, but its ~100KB+/embed and a required Vite build step bust "tiny," and it is a
  whole framework heavier than hand-authoring a few diagrams.
- **Rive** -- ~1.8MB WASM runtime plus a proprietary editor with paid export; no
  git-diffable source.
- **Lottie** -- authored in After Effects, weak at data-driven value changes, runtime near
  maintenance-mode.
- **Mermaid / D2** -- declarative diagram *languages*, but their only animation is looping
  dashed edges (Mermaid) or whole-board cross-fades (D2); both are auto-layout, so no
  pixel control and no discrete token-on-edge flow. Mermaid remains useful as a *static
  SVG substrate* we already ship, not as an engine.
- **GSAP / anime.js / Motion** -- animation *primitives*, not solutions; they would sit
  *under* our engine, not replace it (see below).

Tellingly, the reference aesthetic itself (cursor.com/blog/git-at-any-scale) shows no
third-party animation-library signature in its markup -- the parametric, state-driven
"simulators" are bespoke canvas/JS. The look we want is a custom-built engine at the
source.

**Recommendation: build a thin, dependency-free engine on native web primitives.**

- **Foundation:** inline SVG geometry + `path.getPointAtLength()` for pixel-exact token
  placement + a small rAF step-sequencer (the "compute frame as a pure function of the
  current step" model the spike already uses and the prototype doc committed to). This
  covers 100% of the spec with zero dependencies.
- **Why native over a primitive:** CSS-variable theming and `prefers-reduced-motion` are
  *first-class* on the native path; every library still requires wiring reduced-motion by
  hand (GSAP excepted). The only code we actually write is the sequencer plus a little
  path math -- modest, and it is the ~2KB vanilla custom element the site already permits.
- **rAF vs WAAPI vs CSS Motion Path:** keep the **rAF scalar clock** as the runtime spine
  because it makes Prev/Next/seek a clean "compute frame, swap classes" with no
  half-finished tweens (this is why the prototype doc chose `getPointAtLength` over
  `offset-path`). **CSS Motion Path** (`offset-path` / `offset-distance`) is reserved for
  the no-JS, build-time **static-SVG graduation**, where its native reduced-motion and
  `var()` theming are exactly right. WAAPI is a viable alternative conductor but buys
  little over rAF here.
- **Escape hatches, ranked, only if sequencing glue becomes tedious:** Motion `mini`
  (~2.3KB, MIT, WAAPI-based -- but no built-in sequencing/motion-path); anime.js v4 (MIT,
  has motion-path + timeline + stagger, but single-maintainer risk); GSAP 3.15 (capability
  max incl. a built-in reduced-motion helper, but ~30KB, not tree-shakeable, and a
  proprietary -- royalty-free, not MIT -- license). Reserve a canvas lib (two.js) only if
  a diagram ever needs *hundreds* of simultaneous particles. **Start dependency-free.**

## 4. Q3 -- In-site module now; stage a conditional extraction; do not spin out a repo

Three homes were weighed: **A** internal to alint.org (unpublished), **B** a published npm
package, **C** a standalone OSS repo positioned as a general library.

**Recommend Option A now**, which is also what the prototype plan already specifies
(`public/anim/` runtime + typed specs + tests, no publishing). Grounds:

- alint.org is **not** an npm workspace and has **zero** publish infrastructure; the only
  `public/` build step is LikeC4's own CLI. Shipping the engine as untranspiled sibling ES
  modules + a tiny loader matches the existing LikeC4 pattern exactly.
- The team has real publishing muscle (crates, npm, docker, Homebrew, VS Code, JetBrains)
  but **every existing artifact is a distribution channel for the one product** -- it has
  never maintained a standalone reusable *library*. Publishing is also historically the
  most painful surface (OIDC trusted-publisher gaps, token rotations, the secrets-inventory
  gate). A 7th publish target to narrate a docs page fails cost/benefit.
- The animated-diagram market is **saturated** (Motion Canvas, Rive, Mermaid, GSAP...).
  Our engine's strengths -- hand-placed coordinates, Starlight-token theming, docs captions
  -- are alint-specific niceties, not a general USP. An extraction would be supply-driven
  ("we built it, let's publish"), not demand-driven. This directly contradicts alint's
  deliberate positioning: workspace-tier + OSS-polyglot sweet spot, no hyperscale, do not
  split focus.
- **Reject Option C outright.** Most maintenance, most decoupling work, most focus risk,
  to compete in a crowded market for an audience alint does not need -- while diluting what
  alint *is*.

**Stage Option B, do not do it now.** Structure clean boundaries today so a later
extraction is a `git mv`, not a rewrite:

- **Engine core vs environment adapter.** Keep the render/clock/validate core pure
  (`(spec, frame) -> string/state`, no Starlight or alint specifics). Push everything
  environment-bound (the `--sl-color-*` token *names*, `.not-content` containment, the
  `Head.astro` dev-gate, the loader's theme mirroring) into a thin adapter.
- **Spec schema vs spec instances.** The schema/types + `validateSpec` are the reusable
  contract; the concrete `specs/*.js` are alint-only content that would not travel.
  One-way dependency: specs import the schema, never the reverse.
- **Theme as data.** Pass a `theme` token-map *into* the core rather than referencing
  `--sl-color-*` literals inside the renderer. A future package ships a default theme;
  alint.org passes the Starlight one -- zero renderer change at extraction.
- **Tests already portable.** String-first render means tests import the core by relative
  path and travel unchanged.

**Extraction criteria (revisit B when >= 2 fire; C only on a strong, sustained external
signal):** (1) >= 2 distinct in-ecosystem consumers; (2) an unsolicited external demand
signal (someone asking to use it, or vendoring `public/anim/`); (3) the schema has proven
stable across all Phase-1 diagrams with no breaking changes; (4) decoupling is already
cheap (the boundaries above hold); (5) maintenance capacity exists (a second maintainer,
or the linter roadmap has quiesced). If only (1)+(4): extract as a workspace-internal,
*unpublished* package. Add the npm publish target only once (2)+(3)+(5) also hold.

**LikeC4 as a graduation partner, not a foundation.** LikeC4 (already a dependency) renders
via React/XYFlow inside a 2.6MB shadow-DOM island and is locked to C4 notation (no
arbitrary CSS / custom node rendering), so it cannot express token flow, arrival glow, or
in-place value diffs. Keep it for architecture/topology; build the bespoke engine for the
fine-grained pipeline/rule-evaluation explainers. **Both ride the identical build-time /
self-contained custom-element / tiny-loader plumbing** -- so the custom engine is an
extension of the site's established pattern, not a new paradigm. (Far-future optional:
consume LikeC4's exported layout JSON as data for a custom SVG renderer; almost certainly
more effort than hand-authoring a handful of diagrams.)

## 5. The formalized DSL (proposed minimal grammar)

The formalization reconciles the spike's richer verbs with the committed schema's
structure/typing, and collapses the sprawl into one coherent grammar. This supersedes the
`AnimSpec` sketch in `animated-diagrams-prototype.md` section 3.6.

**One element list, keyed by `kind`** (replaces the nodes/chips/sections/traces split):

```ts
type Kind =
  | 'process' | 'artifact' | 'store' | 'decision' | 'terminal'  // structural boxes
  | 'group'                                                      // was: section backdrop
  | 'file' | 'rule' | 'counter';                                 // content templates

interface El {
  id: string; kind: Kind;
  x: number; y: number; w?: number; h?: number;
  label: string | string[]; sub?: string;
  hidden?: boolean;                    // starts invisible, shown by a step
  // kind-specific: file{ cfg?, note? }  rule{ level, scope }  group{}
}

interface Edge {                       // replaces edges + traces
  id: string; from: string; to: string;         // element ids, OR:
  fromPoint?: [number, number]; toPoint?: [number, number];  // raw coords (was: trace)
  fromSide?: Side; toSide?: Side; waypoints?: [number, number][];
  curve?: 'smooth' | 'orthogonal' | 'straight';
}
```

**One step, with a consolidated verb set** (each verb is now a single primitive):

```ts
interface Step {
  caption: string;
  mark?:  { [id: string]: Status };    // was: on/markGood/markHot/markDrop
  place?: { id: string; to?: [number, number]; opacity?: number; rise?: number }[];
                                        // was: move + reveal (one position+opacity animator)
  flow?:  { along: string | [number, number][]; count?: number; spread?: number;
            variant?: 'good' | 'drop'; lightOnArrival?: boolean }[];
                                        // was: flow + drip + trace (one token-along-path)
  set?:   { id: string; text?: string; level?: Level; count?: number }[];
                                        // was: setText + setLevel + count (one value mutation)
  dur?: number; dwell?: number;        // per-step overrides; else spec.defaults
}

type Status = 'active' | 'done' | 'good' | 'drop' | 'hot' | 'dim';
```

**The one folding rule, written down once and applied uniformly** (today it is implicit
and inconsistent):

- **Monotonic** statuses accumulate across steps `0..i`: `done`, `good`, `drop`, and
  element visibility (`place`/`reveal`) and value (`set`). Once set, they persist.
- **Transient** statuses apply only to the current step `i`: `active`, `hot`, `dim`. They
  express "currently being touched," not a verdict.

**What this buys:**

- `flow` unifies the three token paths: `along` is an edge id (resolved from anchors), or
  raw waypoints (the old `trace`), or a straight two-point path (the old `drip`).
  `lightOnArrival` folds in the pipeline's arrival glow.
- `place` unifies `move` + `reveal` over the position+opacity animator the engine already
  has; `rise` keeps the slide-in.
- `mark` unifies the three state verbs behind one documented folding rule.
- `set` unifies `setText` / `setLevel` / `count`; `level` renders the `was -> now` strike
  diff, `count` tweens numerically.
- `group` (a kind) replaces `sections`; edges with `fromPoint`/`waypoints` replace
  `traces`. Four buckets become two lists (`elements`, `edges`).

Plus the non-grammar formalization work: a `.d.ts` for authoring autocomplete; a
`validateSpec` (unknown ids in any verb, dangling edges, chained-edge continuity, unknown
keys, with a visible fallback render on failure); a **string-first** `renderSvg(spec,
frame) -> string` so `computeFrame` and every registered spec are Node-testable; restored
`prefers-reduced-motion` (no autoplay/loop, disable Play, start on a legible final-fold
frame, static tokens); theming re-tokened onto `--sl-color-*` via the injected theme map;
and the a11y scaffolding (real `aria-label` summary, `aria-live` caption, a findable step
`<ol>`).

## 6. Recommended plan (folds into the prototype's Phase 0)

This does not add a phase; it sharpens Phase 0 (the engine) in `animated-diagrams-prototype.md`
with a DSL-first order of work. Priority order (YAGNI throughout):

1. **Freeze the grammar** in section 5: collapse the element buckets and the verb sprawl,
   delete dead code (`drip`, unused `setText`, `REPO`, `fileGrid`, kind `t`). Write the
   `.d.ts`.
2. **Document + unify the folding rule** (monotonic vs transient) and apply it uniformly.
3. **Add `validateSpec`** with a visible bad-spec fallback.
4. **Make the renderer string-first** (`renderSvg(spec, frame) -> string`; move
   `getPointAtLength`/token motion to the runtime layer). Biggest single refactor; unlocks
   Node tests *and* the build-time static-SVG graduation.
5. **Restore reduced-motion** and finish a11y.
6. **Re-token theming** onto `--sl-color-*` when it moves into the site.
7. **Wire minimal tests to CI**: `computeFrame` status/text maps (including after a
   backward seek), `validateSpec` cases, a `renderSvg` smoke over every registered spec.
8. Per-spec `defaults` + optional per-step `dur` to retire the global `1100/650/700`
   magic constants.

Steps 1 -> 3 -> 4 -> 7 are the formalization spine; 5 is a ship blocker for a docs
artifact; 2/6/8 are polish. If it later graduates to live docs, record the production
decision as ADR-0016 (as the prototype doc notes).

## 7. Scope boundaries / non-goals

- **No auto-layout.** Hand-placed coordinates are deliberate (pixel control is the point);
  a layered-DAG engine (dagre/elk) fights the no-deps and byte-stability constraints for a
  handful of ~10-15-node diagrams. A tiny row/grid coordinate helper is the most we would
  add.
- **No new npm dependencies** in the engine (escape hatches in section 3 only if the
  sequencer glue genuinely hurts).
- **No separate package or repo now.** In-site module with clean boundaries; extraction is
  staged and conditional (section 4).
- **No LikeC4-derived specs, no restyling LikeC4** to this aesthetic (wrong renderer, C4
  lock, 2.6MB). Coexistence, not fusion.
- **No live-docs deploy in this scope** -- the engine ships dev-only for QA, per the
  prototype plan; live docs is a later graduation gated on ADR-0016.

## Appendix A: ecosystem verdicts (condensed)

| Tool | Verdict for token-flow docs diagrams |
| --- | --- |
| Native SVG + rAF + `getPointAtLength` | **Build on this.** 0 deps; theming + reduced-motion first-class. |
| CSS Motion Path (`offset-path`) | Primitive; reserve for the no-JS build-time static-SVG graduation. |
| Motion `mini` (2.3KB) / anime.js / GSAP | Primitives; escape hatches only, not adopted up front. |
| Motion Canvas | Closest "adopt"; busts the size budget + adds a Vite build. Not adopted. |
| Rive | ~1.8MB WASM + proprietary/paid editor. Non-fit. |
| Lottie | AE-authored, weak data-driven text, near-dormant runtime. Non-fit. |
| Mermaid / D2 | Auto-layout, only looping-dash / board cross-fade. Static-SVG substrate at best. |
| LikeC4 | React/2.6MB, C4-locked. Coexistence partner, not a foundation. |
| tldraw / Excalidraw / Reveal.js | React editors / no animation / slideshow. Non-fit. |

## Appendix B: primary sources

Ecosystem sizes/licenses/health verified Aug 2026 via official docs, npm, bundlephobia,
jsdelivr, and GitHub. Native web features: MDN + caniuse for `animateMotion` (SMIL,
Baseline 2020, ~96% support), `offset-path` (Baseline 2022, ~96%), WAAPI `element.animate`
(Baseline 2020). Reference aesthetic: cursor.com/blog/git-at-any-scale (bespoke;
no library signature). LikeC4: likec4.dev dynamic-views + styling docs (C4-locked, no
arbitrary CSS). Full option matrix and citations retained in the research record.
