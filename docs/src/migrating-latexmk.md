# Migrating from latexmk

quire runs latexmk for you. It adds dependency management, profiles, the
preamble cache, clearer errors and arXiv packing.

## Step by step

1. Run `quire init` in the project folder. It finds the `.tex` file with
   `\documentclass` and writes `quire.toml` pointing at it.
2. Set the engine in `quire.toml`, if it isn't pdflatex:

   ```toml
   [document]
   engine = "lualatex"   # what $pdf_mode = 4 or -lualatex meant in latexmk
   ```

3. Move shared `.sty`, `.cls` and `.bib` files out of `TEXINPUTS` or `~/texmf`
   and into [dependencies](dependencies.md), so the project builds on any
   machine.
4. Replace `latexmk -pdf main.tex` with `quire build`, and `latexmk -pvc` with
   `quire watch`.
5. Add `target/` to `.gitignore`.

## latexmkrc settings

| latexmk | quire |
|---|---|
| `$pdf_mode = 1` / `-pdf` | `engine = "pdflatex"` (default) |
| `$pdf_mode = 4` / `-lualatex` | `engine = "lualatex"` |
| `$pdf_mode = 5` / `-xelatex` | `engine = "xelatex"` |
| `$out_dir = 'build'` | Always `target/<profile>/` |
| `-shell-escape` | `shell-escape = true` in `[build]` or a profile |
| `-synctex=1` | On in `dev`, off in `release`; `synctex = …` in a profile |
| Any other option | `latexmk-args = ["…"]` in `[build]` or a profile |
| `$ENV{TEXINPUTS}` | Path or git [dependencies](dependencies.md) |

latexmk runs in the main file's folder, so a `latexmkrc` there is still read
and custom rules (for example for `glossaries` or `makeindex`) keep working. If
your `latexmkrc` sits at the project root and the main file is in `src/`, move
it next to the main file.

## What's different

- Output goes to `target/<profile>/<name>.pdf`, where `<name>` is
  `document.name`, not the main file's name.
- latexmk compiles a small generated file that sets the profile and then
  `\input`s your main file. With the preamble cache on, your main file is
  compiled directly. Either way, errors point at your files.
