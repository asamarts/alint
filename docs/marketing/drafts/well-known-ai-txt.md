---
destination: alint.org/.well-known/ai.txt (site-repo public root)
status: drafting
blocks_on: confirm the AI-training opt-in decision (logged 2026-05-06; this draft assumes opt-in); coordinate with `robots-txt.md` so the two files express the same posture
last_touched: 2026-05-06
---

# alint.org/.well-known/ai.txt — content brief for the site repo

## Why

[Spawning AI](https://spawning.ai/) is leading an emerging standard for
`/.well-known/ai.txt`: a discovery file at the site root that declares
which AI uses (training, content generation, search indexing, etc.) the
site owner permits. It's the AI-era complement to `/robots.txt`.

The standard is **emerging** — it doesn't have an RFC yet, the syntax
is in flux across drafts, and major LLM training pipelines aren't
universally honouring it. But the cost of shipping it is near-zero,
and the **discoverability signal** matters:

1. **It states our posture publicly.** The AI-training decision logged
   2026-05-06 was: **opt in**. ai.txt makes that posture machine-
   readable in a standard location, separate from the robots.txt
   crawler-control mechanism (which is about *runtime* fetches, not
   *training-time* dataset inclusion).
2. **It complements `/robots.txt` and `/llms.txt`.** Three discovery
   files, three concerns:
   - `robots.txt` — crawler runtime access (which UAs may fetch URLs)
   - `ai.txt` — training/derivative-work permissions (which uses)
   - `llms.txt` — LLM runtime context (which canonical URLs)
3. **Spawning AI's `haveibeentrained.com` and similar directories
   read it.** Sites with explicit ai.txt opt-in show up in
   discoverability tooling for AI-training datasets, which aligns
   with the launch goal of being LLM-discoverable.

This brief implements the **opt-in flip** in ai.txt form, alongside
the corresponding robots.txt change in `robots-txt.md`.

## Proposed `/.well-known/ai.txt` body

```text
# alint AI use policy
# Spawning AI's emerging standard for declarative AI-use permissions.
# https://spawning.ai/ai-txt
#
# This file declares the alint.org site owner's posture on AI uses
# of the site's content. Last updated: 2026-05-06.
#
# Decision: opt IN. alint's positioning includes agent-aware tooling
# (see /docs/output-formats/#agent and the bundled agent-* rulesets);
# being LLM-discoverable advances that mission. Maintainer:
# aliaksandr.samartsau@gmail.com.

User-Agent: *
Allow: /

# Per-use declarations (Spawning's draft v0.2 syntax — superset of
# the User-Agent/Allow shape above; redundant but explicit):

Disallow-AI-Training: false
Disallow-AI-Generation: false
Disallow-AI-Search: false
Disallow-AI-Inference: false

# Notes:
# - "Allow: /" without scope-restrictions is the broadest opt-in.
# - This file does NOT waive copyright; alint.org content remains
#   licensed per the LICENSE files in the asamarts/alint repo
#   (LICENSE-MIT and LICENSE-APACHE — Apache-2.0 OR MIT dual licence).
# - For runtime crawler control (which user agents may fetch which
#   URLs), see /robots.txt.
# - For LLM context discovery (which canonical URLs an assistant
#   should pull when answering alint questions), see /llms.txt.
```

### Syntax notes — the "emerging standard" caveat

The Spawning AI draft is iterating; current shape (as of 2026-05):

- **`User-Agent: *` + `Allow: /`** — The robots.txt-shaped declaration
  is the most widely-honoured. Most agent-stack readers (haveibeentrained,
  Spawning's own crawlers, several LLM training pipelines) parse this
  shape first.
- **`Disallow-AI-{Training,Generation,Search,Inference}: false`** —
  Spawning's draft v0.2 syntax for per-use granularity. The four
  enumerated uses cover the standard categories. Setting all four to
  `false` (i.e., NOT disallowed) is the explicit opt-in.

Older sites that pre-date the per-use syntax sometimes use just the
robots-shape. Newer specs may add fields. This draft uses both layers
for forward+backward compat.

## Implementation notes (for the site repo)

- **File location.** Drop the body above into
  `public/.well-known/ai.txt`. Same `.well-known/` directory as
  `security.txt`.
- **Content-Type.** Cloudflare Pages serves `.txt` as `text/plain;
  charset=utf-8` by default — fine for ai.txt parsers.
- **No alint-repo equivalent.** This is a site-only file; the alint
  repo doesn't ship its own ai.txt (the site IS the public surface).
- **Coordinate with `/robots.txt`.** The ai.txt opt-in posture must
  match the robots.txt opt-in posture (see `robots-txt.md`). If a
  crawler reads robots.txt and sees a Disallow but reads ai.txt and
  sees Allow, the conflict resolution is undefined (different vendors
  pick differently). Ship the two files together.

## Open questions

1. **Spec stability.** Spawning's draft is at v0.2-ish; the
   per-use field names (`Disallow-AI-Training` etc.) may rename in
   v0.3. Risk: shipping with field names that go stale within 6
   months. **Mitigation:** include both the robots-shape and the
   per-use shape so at least one of them is always parseable.
   Refresh when the spec stabilises.
2. **Spawning AI's directory pickup.** Sites with an explicit ai.txt
   opt-in get listed in haveibeentrained.com's allow-list directory.
   Worth confirming alint.org actually shows up there after publish
   (manual smoke test 1-2 weeks post-deploy).
3. **License-vs-permission distinction.** ai.txt declares the
   *site owner's permission* for AI use; the actual content licence
   (Apache-2.0 OR MIT for the underlying alint code, CC-BY-likely
   for the docs) is separate. The "Notes:" comment block documents
   this; consider whether to make it more prominent (e.g., a top-
   level `License:` field — not currently in the Spawning spec but
   could be added as a comment).
4. **Per-route exclusions.** Some sites disallow AI training on
   specific paths (e.g., `/private/`) while opting in for the
   public corpus. alint.org has no private paths, so the broad
   `Allow: /` is correct. If we ever add a `/contributors-only/`
   route, revisit.
5. **`User-Agent` granularity.** Spawning's shape allows per-UA
   declarations (`User-Agent: GPTBot` then `Allow: /`). We use
   `User-Agent: *` for the broad opt-in; per-UA blocks are
   redundant when the wildcard already says "yes." Consider
   adding explicit per-UA blocks if the spec evolves to require
   them.

## Pre-publish checklist

- [ ] `public/.well-known/ai.txt` exists at the site root and
      serves as `text/plain` from `https://alint.org/.well-known/ai.txt`.
- [ ] Body matches the proposed content above.
- [ ] `robots-txt.md` is in `ready` state with the matching opt-in
      posture for coordinated publish.
- [ ] Spec-link URL (`https://spawning.ai/ai-txt`) resolves at
      publish time (Spawning sometimes reorganises URLs; if
      broken, link to a stable archive snapshot or Wayback link).
- [ ] Manual parse test: paste body into Spawning's online
      validator if available; otherwise verify the
      User-Agent/Allow lines are syntactically valid robots-shape.
- [ ] Calendar reminder for spec-version review at +6 months —
      check whether v0.3 of the Spawning draft renames any
      fields.
- [ ] STATE.md row for `.well-known/ai.txt` flipped from `missing`
      to `live` with date + commit SHA.

## Estimated diff size on the site repo

- 1 new file at `public/.well-known/ai.txt`: ~30 lines (mostly
  comments documenting the posture; the actual rules are 6 lines).

Total: ~30 lines (one file).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `robots-txt.md` | The two files MUST express the same opt-in posture. Conflict resolution between robots.txt and ai.txt is undefined per spec. Ship coordinated. |
| `llms-txt.md`, `llms-full-txt.md` | Conceptually paired (the LLM-discovery trio). Different files, different concerns; no hard dependency, but "ship the AI-discoverability bundle together" is the right framing. |
| `well-known-security-txt.md` | Same `/.well-known/` directory, both static text files. Can ship in either order; bundling them in one site-repo PR makes sense. |
