# Emacs — `navigator-lsp`

Both built-in `eglot` (Emacs 29+) and the community `lsp-mode` work.

## `eglot` (recommended)

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(markdown-mode . ("navigator-lsp"))))

(add-hook 'markdown-mode-hook #'eglot-ensure)
```

Fix-on-save:

```elisp
(add-hook 'markdown-mode-hook
          (lambda ()
            (add-hook 'before-save-hook
                      (lambda ()
                        (eglot-code-actions nil nil "source.fixAll" t))
                      nil t)))
```

## `lsp-mode`

```elisp
(use-package lsp-mode
  :hook (markdown-mode . lsp-deferred)
  :config
  (add-to-list 'lsp-language-id-configuration '(markdown-mode . "markdown"))
  (lsp-register-client
   (make-lsp-client :new-connection (lsp-stdio-connection "navigator-lsp")
                    :major-modes '(markdown-mode)
                    :server-id 'navigator-lsp)))
```

Use `M-x lsp-execute-code-action` and pick `source.fixAll` for the same behavior `cli validate --fix` ships.
