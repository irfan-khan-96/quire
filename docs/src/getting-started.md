# Getting started

## A new document

```console
$ quire new paper
     Created document `paper` in `paper`
$ cd paper
$ quire build
   Compiling paper (pdflatex, profile `dev`)
     Caching preamble of `paper`
    Finished `dev` profile in 3.95s → target/dev/paper.pdf
```

`quire new` creates this layout and runs `git init`:

```text
paper/
├── .gitignore        # ignores target/
├── quire.toml
└── src/
    └── main.tex
```

Templates: `--template article` (the default), `report`, `beamer`, and
`package`, which creates a reusable package (see [Dependencies](dependencies.md)).
Choose the engine with `--engine pdflatex|xelatex|lualatex`.

## An existing document

Run `quire init` in the folder. If it finds a `.tex` file with
`\documentclass` in the folder or in `src/`, it uses that as the main file
instead of creating a template:

```console
$ cd old-thesis
$ quire init
note: using existing `thesis.tex` as the main file
     Created document `old-thesis`
```

## The manifest

```toml
[document]
name = "paper"           # output is target/<profile>/paper.pdf
main = "src/main.tex"
engine = "pdflatex"

[dependencies]

[profile.draft]
class-options = ["draft"]
```

quire looks for `quire.toml` in the current directory and its parents, so you
can run it from anywhere inside the project.

## Everyday commands

| Command | |
|---|---|
| `quire build` | Compile once. Add `--open` to open the PDF. |
| `quire watch` | Rebuild whenever a file changes (Ctrl-C to stop). |
| `quire build --profile draft` | Use another [profile](profiles.md). |
| `quire build --release` | The release profile: no SyncTeX, ready to share. |
| `quire clean` | Delete `target/`. |

## When something is wrong

quire prints each TeX error with its file, line and the source line TeX
stopped at, instead of the whole log:

```console
$ quire build
   Compiling paper (pdflatex, profile `dev`)
error: /home/me/paper/src/main.tex:12: Undefined control sequence.
  l.12 \secton
              {Results}
note: full log: target/dev/paper.log
error: could not compile `paper`
```

Add `-v` to see latexmk's complete output.
