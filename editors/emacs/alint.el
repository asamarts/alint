;;; alint.el --- alint language server integration -*- lexical-binding: t; -*-

;; Copyright (C) 2026 alint contributors
;; Author: alint contributors
;; URL: https://github.com/asamarts/alint
;; Version: 0.10.2
;; Package-Requires: ((emacs "29.1"))
;; SPDX-License-Identifier: Apache-2.0 OR MIT

;;; Commentary:

;; Registers the alint language server (`alint lsp') with Eglot so
;; alint's repository-structure diagnostics, hover, and quick-fixes
;; appear in Emacs.  Requires the `alint' binary on PATH (Homebrew,
;; `cargo install alint', `npm i -g @asamarts/alint', Docker, or the
;; install script).
;;
;; Usage:
;;   (require 'alint)        ; or (use-package alint)
;;   ;; then `M-x eglot' in a buffer of a registered mode.
;;
;; alint lints every file, but Eglot attaches per major mode — there is
;; no "all files" wildcard.  `alint-modes' lists a broad set of common
;; programming modes; extend it as needed.
;;
;; Until it is told the workspace is trusted, the server runs nothing the
;; repository's config could start (command rules, rules that run Git,
;; custom facts), fetches no remote `extends:' and reads no environment
;; value.  Set `alint-trusted' to t, before this package loads, to trust
;; every workspace alint lints.

;;; Code:

(require 'eglot)

(defgroup alint nil
  "alint language server integration."
  :group 'tools
  :prefix "alint-")

(defcustom alint-command '("alint" "lsp")
  "Command (program followed by arguments) that starts the alint server."
  :type '(repeat string)
  :group 'alint)

(defcustom alint-modes
  '(rust-mode rust-ts-mode
    python-mode python-ts-mode
    js-mode js-ts-mode
    typescript-mode typescript-ts-mode tsx-ts-mode
    go-mode go-ts-mode
    c-mode c-ts-mode c++-mode c++-ts-mode
    ruby-mode ruby-ts-mode
    json-mode json-ts-mode
    yaml-mode yaml-ts-mode
    conf-toml-mode toml-ts-mode
    markdown-mode)
  "Major modes the alint language server should attach to."
  :type '(repeat symbol)
  :group 'alint)

(defcustom alint-trusted nil
  "Non-nil to tell the alint server every workspace is trusted.
A trusted workspace's config may run commands (command rules, rules
that run Git and custom facts), fetch a remote `extends:' and read environment
variables; an untrusted one lints without them.  Set it in your own
init file, before this package loads, and only if you open
repositories you trust."
  :type 'boolean
  :group 'alint)

;;;###autoload
(defun alint-setup ()
  "Register the alint language server with Eglot for `alint-modes'."
  (add-to-list 'eglot-server-programs
               (cons alint-modes
                     (if alint-trusted
                         (append alint-command
                                 '(:initializationOptions (:trusted t)))
                       alint-command))))

;;;###autoload
(with-eval-after-load 'eglot
  (alint-setup))

(provide 'alint)
;;; alint.el ends here
