# Migrating from Overleaf

You can work locally with quire and still share the project through Overleaf.

## Bring a project home

1. In Overleaf, download the project's source as a zip and unpack it.
2. In that folder, run:

   ```console
   $ quire init
   note: using existing `main.tex` as the main file
        Created document `my-paper`
   ```

3. Set the engine to match Overleaf's compiler setting (Menu → Compiler):

   ```toml
   [document]
   engine = "pdflatex"   # or "xelatex" / "lualatex"
   ```

4. Run `quire build`. Missing packages are reported by name. Install them
   with your TeX distribution's package manager. MiKTeX can also install them
   on demand.

Overleaf keeps everything in one flat project. That works unchanged with quire,
because paths are relative to the main file in both.

## Keep using Overleaf

`quire.toml` and `quire.lock` are plain files that Overleaf ignores, so a
project can live in both places, for example through Overleaf's git
integration. Keep `target/` out of the Overleaf project.

Overleaf can't fetch quire dependencies. If the project uses path or git
dependencies, copy their files into the Overleaf project, or keep the shared
files in Overleaf and use quire only locally.

## Submitting

Overleaf has its own arXiv export. With quire, `quire pack` builds the
submission locally and checks that it compiles on its own (see
[Submitting to arXiv](arxiv.md)).
