#!/usr/bin/env bash
# Static validation of the Tier-2 editor configs (snippet-only
# integrations with no headless harness) so a typo can't ship unnoticed:
#   * helix/languages.toml          -> TOML parse (python tomllib)
#   * sublime/LSP.sublime-settings  -> JSONC parse (strip comments, json)
#   * emacs/alint.el                -> emacs --batch byte-compile
#
# nvim is covered by its own headless e2e smoke (editors/nvim/test);
# eclipse is docs-only. Emacs is validated only when `emacs` is on PATH
# (CI installs it; locally it's skipped with a note).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" # editors/
fail=0

echo "==> helix/languages.toml (TOML)"
if python3 -c 'import tomllib,sys; tomllib.load(open(sys.argv[1],"rb"))' \
  "$root/helix/languages.toml"; then
  echo "  ok"
else
  echo "  FAIL: invalid TOML" >&2
  fail=1
fi

echo "==> sublime/LSP.sublime-settings (JSONC)"
if python3 - "$root/sublime/LSP.sublime-settings" <<'PY'; then
import json, sys

src = open(sys.argv[1]).read()
# Strip // line comments and /* */ block comments, respecting strings.
out, i, n = [], 0, len(src)
in_str = esc = False
while i < n:
    c = src[i]
    if in_str:
        out.append(c)
        if esc:
            esc = False
        elif c == "\\":
            esc = True
        elif c == '"':
            in_str = False
        i += 1
    elif c == '"':
        in_str = True
        out.append(c)
        i += 1
    elif c == "/" and i + 1 < n and src[i + 1] == "/":
        while i < n and src[i] != "\n":
            i += 1
    elif c == "/" and i + 1 < n and src[i + 1] == "*":
        i += 2
        while i + 1 < n and not (src[i] == "*" and src[i + 1] == "/"):
            i += 1
        i += 2
    else:
        out.append(c)
        i += 1
json.loads("".join(out))
PY
  echo "  ok"
else
  echo "  FAIL: invalid JSONC" >&2
  fail=1
fi

echo "==> emacs/alint.el (byte-compile + eglot precedence & risky vars)"
if command -v emacs >/dev/null 2>&1; then
  # Compile a copy in a temp dir so no .elc lands in the source tree.
  tmp="$(mktemp -d)"
  cp "$root/emacs/alint.el" "$tmp/alint.el"
  if emacs --batch -f batch-byte-compile "$tmp/alint.el" 2>&1 \
    && emacs --batch -L "$tmp" --eval '
      (require (quote eglot))
      (require (quote alint))
      (unless (get (quote alint-command) (quote risky-local-variable))
        (error "alint-command must be marked :risky t"))
      (unless (get (quote alint-trusted) (quote risky-local-variable))
        (error "alint-trusted must be marked :risky t"))
      (alint-setup)
      (alint-setup)
      (let ((alint-entries (seq-filter (lambda (e) (and (listp (cdr e)) (equal (car (cdr e)) "alint"))) eglot-server-programs)))
        (unless (= (length alint-entries) 1)
          (error "alint-setup must deduplicate entries, got %d" (length alint-entries))))
      (let ((rust-first (cl-find-if (lambda (e)
                          (let ((modes (car e)))
                            (or (eq modes (quote rust-mode))
                                (and (listp modes) (memq (quote rust-mode) modes)))))
                        eglot-server-programs)))
        (when (and (listp (cdr rust-first)) (equal (car (cdr rust-first)) "alint"))
          (error "alint shadowed built-in rust-mode Eglot server: %S" rust-first)))
    ' 2>&1; then
    echo "  ok"
  else
    echo "  FAIL: emacs check error" >&2
    fail=1
  fi
  rm -rf "$tmp"
else
  echo "  SKIP (emacs not on PATH)"
fi

echo "==> emacs/README.md (lsp-mode fallback append)"
if python3 - "$root/emacs/README.md" <<'PY'; then
import re, sys
text = open(sys.argv[1]).read()
for m in re.finditer(r'\(add-to-list\s+\x27lsp-language-id-configuration[^\n]*\.\*[^\n]*\)', text):
    if not re.search(r'\bt\s*\)\s*$', m.group(0)):
        sys.stderr.write(f"lsp-language-id-configuration wildcard missing APPEND t: {m.group(0)}\n")
        sys.exit(1)
PY
  echo "  ok"
else
  echo "  FAIL: emacs README lsp-mode snippet" >&2
  fail=1
fi

echo "==> zed/extension.toml (TOML)"
if python3 -c 'import tomllib,sys; d=tomllib.load(open(sys.argv[1],"rb")); assert "language_servers" in d and "alint" in d["language_servers"]' \
  "$root/zed/extension.toml"; then
  echo "  ok"
else
  echo "  FAIL: invalid zed/extension.toml" >&2
  fail=1
fi

echo "==> vscode/package.json (JSON + activationEvents config parity)"
if python3 - "$root/vscode/package.json" <<'PY'; then
import json, sys
pkg = json.load(open(sys.argv[1]))
events = set(pkg.get("activationEvents", []))
required = {
    "workspaceContains:**/.alint.yml",
    "workspaceContains:**/.alint.yaml",
    "workspaceContains:**/alint.yml",
    "workspaceContains:**/alint.yaml",
}
missing = sorted(required - events)
if missing:
    sys.stderr.write(f"missing activationEvents: {missing}\n")
    sys.exit(1)
PY
  echo "  ok"
else
  echo "  FAIL: vscode/package.json" >&2
  fail=1
fi

if [[ "$fail" == 0 ]]; then
  echo "==> all Tier-2 configs valid"
else
  echo "==> Tier-2 config validation FAILED" >&2
  exit 1
fi
