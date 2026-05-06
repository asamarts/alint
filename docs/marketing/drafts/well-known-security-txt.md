---
destination: alint.org/.well-known/security.txt (site-repo public root)
status: drafting
blocks_on: PGP key URL decision (default: omit Encryption field; we currently say "key on request" in SECURITY.md); confirm the 1-year Expires cadence (recommend yearly refresh tied to a release)
last_touched: 2026-05-06
---

# alint.org/.well-known/security.txt — content brief for the site repo

## Why

[RFC 9116](https://www.rfc-editor.org/rfc/rfc9116) defines
`/.well-known/security.txt` as the standard discovery file for
security researchers reporting vulnerabilities. A well-formed
security.txt cuts time-to-disclosure dramatically — researchers
don't have to dig through GitHub for a `SECURITY.md` or guess at an
email address.

For a build-time / CI-time tool with supply-chain implications,
having this file at the site root is **table stakes**:

1. **Disclosure path is already documented.** `SECURITY.md` (in the
   alint repo root) is well-formed: 72-hour acknowledgement,
   90-day disclosure, severity-tiered fix windows, scope clearly
   bounded. security.txt just makes that path discoverable from a
   well-known URL.
2. **Supply-chain seriousness signal.** Security teams looking at
   alint as a build dependency will check for security.txt as a
   smoke test. Its absence is read as "not seriously maintained."
3. **Bug-bounty platforms expect it.** Even though we don't run a
   bounty (yet), the file is the established convention.

This brief produces a **complete `/.well-known/security.txt` body
ready to drop on the site**, plus the implementation notes for the
applier.

## Proposed `/.well-known/security.txt` body

```text
# alint security disclosure
# RFC 9116 — https://www.rfc-editor.org/rfc/rfc9116

Contact: mailto:aliaksandr.samartsau@gmail.com
Contact: https://github.com/asamarts/alint/security/advisories/new

Expires: 2027-05-06T00:00:00.000Z

Acknowledgments: https://github.com/asamarts/alint/security/advisories
Preferred-Languages: en
Canonical: https://alint.org/.well-known/security.txt
Policy: https://github.com/asamarts/alint/blob/main/SECURITY.md

# Scope, response timelines, and threat model are documented in
# the Policy URL above. PGP-encrypted reports are accepted; key
# fingerprint is published on request to the Contact email.
```

### Field-by-field rationale

| Field | Value | Why |
|---|---|---|
| `Contact` (email) | `aliaksandr.samartsau@gmail.com` | Matches the SECURITY.md primary contact. RFC 9116 allows multiple `Contact:` lines; email is the lowest-friction default. |
| `Contact` (URL) | `https://github.com/asamarts/alint/security/advisories/new` | The GitHub Private Vulnerability Reporting endpoint — researchers who prefer that channel can use it directly. SECURITY.md already documents both. |
| `Expires` | `2027-05-06T00:00:00.000Z` | RFC 9116 requires this field; 1 year is the typical cadence. Set it to 1 year from `last_touched` and add a calendar reminder for a refresh PR ~3 weeks before expiry. |
| `Acknowledgments` | `https://github.com/asamarts/alint/security/advisories` | Where past advisories live. Currently empty (no advisories shipped as of v0.9.16); the URL still resolves (GitHub Security tab) and will populate as advisories accumulate. |
| `Preferred-Languages` | `en` | Single-language project; English-only triage. RFC 9116 allows BCP 47 language tags. |
| `Canonical` | `https://alint.org/.well-known/security.txt` | Self-reference; protects against spoofed copies hosted elsewhere. |
| `Policy` | `https://github.com/asamarts/alint/blob/main/SECURITY.md` | The full disclosure policy (response timelines, threat model, scope) lives in SECURITY.md. The security.txt is just the discovery layer. |

### Fields deliberately omitted

| Field | Why omitted |
|---|---|
| `Encryption` | We currently document "PGP key fingerprint published on request to the same email address" in SECURITY.md rather than hosting a public key. If we publish a key at `https://alint.org/pgp-key.asc`, add an `Encryption:` line then. Default for v0.10 ship: omit. |
| `Hiring` | Not relevant; alint isn't a company hiring security researchers. |
| `CSAF` | Not yet emitting CSAF-format advisories. Add when/if. |

## Implementation notes (for the site repo)

- **File location.** Drop the body above into
  `public/.well-known/security.txt` (Astro/Starlight serves
  `public/` verbatim, so `.well-known/` works as a literal
  directory).
- **Content-Type.** Cloudflare Pages serves `.txt` as `text/plain;
  charset=utf-8` by default. RFC 9116 prefers `text/plain` — no
  custom header config needed.
- **PGP-signing the file (optional).** RFC 9116 §3.3 strongly
  recommends signing the security.txt itself with PGP. Until we
  publish a PGP key, ship unsigned. When we add a key, sign with:

  ```bash
  gpg --clearsign --output security.txt.asc security.txt
  mv security.txt.asc public/.well-known/security.txt
  ```

  (clearsign embeds the original text + signature in one file.)
- **Coordination with SECURITY.md.** The `Policy:` URL points at
  the canonical SECURITY.md in the alint repo. If SECURITY.md ever
  moves (e.g., to a `.github/SECURITY.md` location), update the
  Policy URL — but the GitHub redirect will hold for the
  foreseeable future.
- **Don't publish to the alint repo's `docs-bundle` branch.** This
  file is site-only; the alint repo's SECURITY.md is the canonical
  policy doc, and the security.txt is just a discovery shim.

## Open questions

1. **PGP key publication.** Currently SECURITY.md says "fingerprint
   on request." If we want to lower the friction for high-
   sensitivity reports, publish a key at
   `https://alint.org/pgp-key.asc` and add `Encryption:` to the
   security.txt. **Recommend defer to v0.11** — no demand
   surfaced; defaults are fine.
2. **Expires cadence.** Default is 1 year. Some projects use
   3-6 months for tighter rotation. Recommend 1 year + calendar
   reminder for refresh.
3. **Per-project security.txt for the alint.org site repo
   itself.** The site repo (Astro/Starlight) has its own attack
   surface (XSS in markdown rendering, etc.). For now the site
   inherits the alint policy; if the site repo grows complex
   enough to warrant separate triage, fork the security.txt at
   that point.
4. **Multi-line `Contact:` ordering.** RFC 9116 doesn't specify
   precedence when multiple `Contact:` are listed; researchers
   will use whichever they prefer. Email is listed first because
   it's the lowest-friction path.

## Pre-publish checklist

- [ ] `public/.well-known/security.txt` exists at the site root and
      serves as `text/plain` from `https://alint.org/.well-known/security.txt`.
- [ ] All listed URLs resolve:
      - `https://github.com/asamarts/alint/security/advisories/new`
      - `https://github.com/asamarts/alint/security/advisories`
      - `https://github.com/asamarts/alint/blob/main/SECURITY.md`
- [ ] Contact email is reachable (smoke test: send a "test" email,
      verify it lands).
- [ ] RFC 9116 validator passes — paste the body into
      https://securitytxt.org/ to verify all required fields are
      present + well-formed.
- [ ] `Expires` date is at least 30 days in the future.
- [ ] Calendar reminder set for `Expires - 21 days` to
      open the refresh PR.
- [ ] STATE.md row for `.well-known/security.txt` flipped from
      `missing` to `live` with date + commit SHA.

## Estimated diff size on the site repo

- 1 new file at `public/.well-known/security.txt`: ~13 lines
  (10 fields + 3 comment lines).

Total: ~13 lines (one file).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| (none in P3.3) | Standalone file. Doesn't gate or get gated by other P3.3 work. Can ship in any order. |
| Future: `branding/` (P4) | If we add a logo + favicon to alint.org, no interaction with security.txt. |
| Future: `pgp-key.asc` publication | When we publish a PGP key, add `Encryption:` line to security.txt + bump the file's `Expires`. |
