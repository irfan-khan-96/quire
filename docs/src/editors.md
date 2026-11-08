# Editors

quire is a command-line tool, so any editor that can run a command can use it.
`quire build` finds `quire.toml` from any file inside the project, and the PDF
and its SyncTeX data are in `target/dev/`.

Errors are printed as `error: <file>:<line>: <message>`, with the absolute file
path. Most editors can turn that into clickable problems. Tools that want more
structure can use [JSON messages](reference/json.md).

## VS Code

Add a build task in `.vscode/tasks.json`. Its problem matcher puts TeX errors
in the Problems panel:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "quire build",
      "type": "shell",
      "command": "quire build",
      "group": { "kind": "build", "isDefault": true },
      "problemMatcher": {
        "owner": "quire",
        "fileLocation": "absolute",
        "pattern": {
          "regexp": "^error: (.*):(\\d+): (.*)$",
          "file": 1,
          "line": 2,
          "message": 3
        }
      }
    }
  ]
}
```

Run it with **Terminal → Run Build Task** (Ctrl+Shift+B). For continuous
rebuilding, run `quire watch` in a terminal and open `target/dev/<name>.pdf` in
a PDF viewer that reloads on change.

## Vim and Neovim

```vim
set makeprg=quire\ build
set errorformat=error:\ %f:%l:\ %m
```

`:make` builds and fills the quickfix list.

## Emacs

```elisp
(setq compile-command "quire build")
(add-to-list 'compilation-error-regexp-alist-alist
             '(quire "^error: \\(.*\\):\\([0-9]+\\): " 1 2))
(add-to-list 'compilation-error-regexp-alist 'quire)
```

## Forward and inverse search

The `dev` profile writes SyncTeX data next to the PDF
(`target/dev/<name>.synctex.gz`). Point your viewer at the PDF in `target/dev/`.
Positions refer to your own `.tex` files, including when the
[preamble cache](preamble-cache.md) is on.
