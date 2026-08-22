# Release credentials & publishing automation

The goal: **every release is a single `git tag` + `git push`**, and no
human touches a publishing portal afterward. This document is the
convention for *where* publishing credentials live, *how* to obtain
each, and *which* channels we can run keyless (OIDC) so there is nothing
to rotate.

## Where secrets live (convention)

- **GitHub Actions *repository* secrets** (`asamarts/alint` → Settings →
  Secrets and variables → Actions). Not environment secrets: an
  Environment with required reviewers would re-introduce a manual
  approval on every release, which is the opposite of the goal. (A
  protection gate is available later if ever wanted; see "Optional".)
- **Naming:** `UPPER_SNAKE_CASE`, already the house style.
- **Set them with the `gh` CLI so the value is typed locally and never
  pasted into a chat, a file, or a commit:**

  ```sh
  # single-line token (prompts, or pipe from a password manager):
  gh secret set NPM_TOKEN --repo asamarts/alint

  # multi-line value (SSH key, cert chain) from a file:
  gh secret set HOMEBREW_TAP_DEPLOY_KEY --repo asamarts/alint < deploy_key
  ```

  Never commit a secret value, never echo it, never put it in this repo.

## Inventory

| Secret | Channel | Obtain from | Expires? | Keyless (OIDC) possible? |
|---|---|---|---|---|
| `GITHUB_TOKEN` | ghcr.io Docker | built-in | per-run | already keyless |
| `CARGO_REGISTRY_TOKEN` | crates.io | crates.io account | manual | **yes → migrate to Trusted Publishing** |
| `NPM_TOKEN` | npm (`@asamarts/alint`) | npmjs.com | retired | **migrated → Trusted Publishing (tokenless)** |
| `HOMEBREW_TAP_DEPLOY_KEY` | `asamarts/homebrew-alint` | ssh-keygen + repo deploy key | no (SSH key) | n/a (use a GitHub App for org scale) |
| `VSCE_PAT` | VS Code Marketplace | Azure DevOps PAT | **yes (max 1 yr)** | no (Azure DevOps has no GH OIDC) |
| `OVSX_PAT` | Open VSX | open-vsx.org token | no | no |
| `JETBRAINS_MARKETPLACE_TOKEN` | JetBrains Marketplace | plugins.jetbrains.com | no (permanent token) | no |
| `JETBRAINS_CERTIFICATE_CHAIN` / `JETBRAINS_PRIVATE_KEY` / `JETBRAINS_PRIVATE_KEY_PASSWORD` | JetBrains plugin signing | self-generated (openssl) | cert validity (set long) | n/a |
| `CODECOV_TOKEN` | Codecov (CI coverage) | codecov.io | no | tokenless for public repos |
| `ALINT_ORG_DEPLOY_HOOK` | alint.org Cloudflare rebuild | Cloudflare Pages deploy hook | no | n/a |

## Automation strategy: keyless where the registry supports it

Both crates.io and npm support **GitHub OIDC Trusted Publishing**
(launched 2025). **crates.io is live on it:** the `publish-crates` job
carries `id-token: write` and mints a short-lived token, so there is no
`CARGO_REGISTRY_TOKEN` to rotate. **npm is now live on it too:** the
`publish-npm` job carries `id-token: write` and publishes tokenlessly
via Trusted Publishing, so the `NPM_TOKEN` PAT is retired. This
eliminated the one token with a recurring expiry-failure history (it
bit us again at v0.15.1, the trigger for the migration).

- **crates.io:** configure a Trusted Publisher (crate Settings →
  Trusted Publishing: repo `asamarts/alint`, the release workflow file,
  optional environment). The `publish-crates` job then needs
  `permissions: id-token: write` and exchanges the OIDC token for an
  ephemeral registry token (via `rust-lang/crates-io-auth-action`)
  instead of `CARGO_REGISTRY_TOKEN`.
- **npm:** configure a Trusted Publisher on the package
  (npmjs.com → package → Settings → Trusted Publisher → GitHub Actions:
  `asamarts/alint` + workflow + optional environment). The
  `publish-npm` job needs `permissions: id-token: write`, npm CLI
  `>= 11.5.1`, and Node `>= 22.14`; `npm publish` then authenticates via
  OIDC (no `NODE_AUTH_TOKEN`) and gets build provenance for free.

crates.io and npm are both migrated, so `CARGO_REGISTRY_TOKEN` and
`NPM_TOKEN` can both be deleted once each first OIDC release proves out.

## Per-channel setup runbook

Work top to bottom. Each ends with the exact `gh secret set` (skip the
two that go keyless if you take the OIDC path).

1. **ghcr.io** — nothing to do (`GITHUB_TOKEN` is automatic).
2. **crates.io** — keyless: add the Trusted Publisher (above). Token
   fallback: crates.io → Account Settings → API Tokens → new token
   scoped to publish; `gh secret set CARGO_REGISTRY_TOKEN`.
3. **npm** — currently the token path (OIDC deferred, npmjs UI bug):
   npmjs.com → Access Tokens → Granular, `@asamarts` scope, read+write
   packages, set a calendar reminder for the expiry; `gh secret set
   NPM_TOKEN`.
4. **Homebrew tap** — `ssh-keygen -t ed25519 -f tap_key -N ""`; add
   `tap_key.pub` as a **write** deploy key on `asamarts/homebrew-alint`;
   `gh secret set HOMEBREW_TAP_DEPLOY_KEY --repo asamarts/alint < tap_key`;
   delete the local key files.
5. **VS Code Marketplace** — create the **`asamarts` publisher** at
   marketplace.visualstudio.com/manage; create an **Azure DevOps** org,
   then a PAT (all accessible orgs, Marketplace → Manage, max expiry);
   `gh secret set VSCE_PAT`.
6. **Open VSX** — sign in at open-vsx.org, claim the `asamarts`
   namespace, create an access token; `gh secret set OVSX_PAT`.
7. **JetBrains Marketplace** — create a vendor at
   plugins.jetbrains.com, generate a permanent token (My Tokens);
   `gh secret set JETBRAINS_MARKETPLACE_TOKEN`. Then generate a signing
   chain (per JetBrains' "Plugin Signing" docs, openssl) and set
   `JETBRAINS_CERTIFICATE_CHAIN`, `JETBRAINS_PRIVATE_KEY`,
   `JETBRAINS_PRIVATE_KEY_PASSWORD`.
8. **Codecov / alint.org** — already configured; no action.

## Residual manual actions (and how we minimize them)

After the one-time setup, the only recurring human steps are:

- **VS Code Azure DevOps PAT** and **`NPM_TOKEN`** rotation (the two
  credentials with expiry; npm has no keyless path yet, VS Code never
  will). Minimize: set the maximum expiry, add a calendar reminder, and
  on a `401` rotate + `gh run rerun <id> --failed` (no new tag, the
  artifacts are idempotent per version).
- **Zed extension version bump.** The Zed registry pins a version, so
  each release needs a one-line bump PR to `zed-industries/extensions`.
  This is the only per-release PR; it can be automated later with a
  release job that opens the bump PR via a GitHub App token.

Everything else (crates.io, npm, ghcr, Homebrew, VS Code, Open VSX,
JetBrains) fires automatically on the tag with no portal interaction.

## PR-based registries (one-time, then hands-off)

- **MELPA** (Emacs) and **Package Control** (Sublime) build from the
  source repo on every tagged commit once the recipe/package is
  registered, so after the one-time submission there is no per-release
  action.
- **nvim-lspconfig** (Neovim) hosts a static server definition; no
  per-release action after the one-time PR.
- **Zed** is the exception (per-release version bump, see above).

## Optional: a `release` Environment

If a manual approval gate is ever wanted (e.g. to pause publishing for a
human sign-off), move the publish secrets into a GitHub `release`
Environment with required reviewers and add `environment: release` to
the publish jobs. This trades full automation for a gate, so it is off
by default.
