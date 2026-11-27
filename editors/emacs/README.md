# alint for Emacs

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in Emacs, via the `alint lsp` language server. This is a **config**
integration (Tier 2).

Requires the `alint` binary on `PATH` (Homebrew, `cargo install alint`,
`npm i -g @asamarts/alint`, Docker, or the install script).

## Eglot (built into Emacs 29+)

Manual registration — no package needed (pass `t` to `add-to-list` so `alint`
is appended after built-in per-language servers rather than shadowing them):

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '((rust-mode python-mode js-mode typescript-mode go-mode
                  c-mode c++-mode ruby-mode json-mode yaml-mode markdown-mode)
                 . ("alint" "lsp"))
               t))
```

Then run `M-x eglot` in a buffer of a registered mode. Note that Eglot runs at
most one language server per major mode (the first match in
`eglot-server-programs`). Appending keeps `rust-analyzer`, `pylsp`, `gopls`,
etc. active for modes that already have a server; to run `alint lsp` alongside
a language-specific server, use `lsp-mode` (`:add-on? t`) below or an LSP
multiplexer.

Or use the bundled package, which provides `alint-setup` plus a customizable
mode/command list (see [`alint.el`](./alint.el)):

```elisp
(use-package alint
  :load-path "path/to/alint/editors/emacs" ;; until it's on MELPA
  :config
  (alint-setup))
```

## lsp-mode

```elisp
(with-eval-after-load 'lsp-mode
  (add-to-list 'lsp-language-id-configuration '(".*" . "plaintext") t)
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection '("alint" "lsp"))
    :activation-fn (lambda (&rest _) t)
    :server-id 'alint
    :add-on? t)))
```

Appending `(".*" . "plaintext")` with `t` acts purely as a fallback for
unmapped modes without overriding `languageId` for Rust, Python, Go, etc.,
and `:add-on? t` lets alint run alongside a language-specific server.

## Workspace trust

Until the server is told a workspace is trusted, it runs nothing the
repository's `.alint.yml` could start: command rules, rules that run Git
and `custom:` facts are skipped, a remote `extends:` is not fetched,
`allow_out_of_root:` is refused, and `{{env.*}}` values are left unset.
Every other rule lints. See
[Workspace trust](https://alint.org/docs/cli/lsp/#workspace-trust).

To turn them on with Eglot, add the initialization option after the
command:

```elisp
'(... . ("alint" "lsp" :initializationOptions (:trusted t)))
```

With the bundled `alint.el`, set `(setq alint-trusted t)` before it
loads. With lsp-mode, add
`:initialization-options (lambda () (list :trusted t))` to the
`make-lsp-client` call.
Set it in your user-level config, never in a file a repository can
supply, and only if you open repositories you trust: it trusts every
workspace alint lints.
A repository's `.dir-locals.el` must not be the place.

## Known limitation

Eglot attaches language servers **per major mode** — there is no "all
files" wildcard. alint is repo-structural, so `alint-modes` (or the
manual snippet) lists a broad set of common modes; add others as needed.
(`lsp-mode`'s `:activation-fn` returning `t` can attach more broadly.)

## MELPA

`alint.el` is intended for MELPA (a recipe PR) so users can
`(use-package alint)` directly — not yet submitted; tracked as a
follow-up.
