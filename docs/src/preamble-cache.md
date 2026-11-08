# Fast rebuilds

Most of a LaTeX rebuild goes into loading the preamble again: TikZ, fonts,
hyperref, siunitx. With pdflatex, quire compiles the preamble once into a
*format file* and starts later builds from it, so editing the body only pays for
the body.

Median edit-and-rebuild time (change one word, rebuild):

| Document | latexmk | quire, no cache | quire, cached | Speed-up |
|---|---:|---:|---:|---:|
| paper: microtype, AMS, siunitx, TikZ, hyperref, cleveref | 5.82s | 5.92s | **3.30s** | 1.8× |
| heavy: paper plus pgfplots and TikZ libraries | 6.41s | 6.21s | **3.07s** | 2.1× |
| beamer: beamer, Madrid theme, TikZ | 7.04s | 5.39s | **2.90s** | 2.4× |
| light: article and amsmath | 2.36s | 2.39s | 2.20s | 1.1× |

These were measured with MiKTeX 26.5 on Windows 11 and include latexmk's own
startup. The documents and the script are in
[`bench/`](https://github.com/irfan-khan-96/quire/tree/main/bench). Run
`python bench/bench.py` to measure on your machine.

## What you'll see

The first build after a preamble change shows an extra step:

```console
   Compiling paper (pdflatex, profile `dev`)
     Caching preamble of `paper`
    Finished `dev` profile in 5.11s → target/dev/paper.pdf
```

Later builds reuse the cache until something it depends on changes.

## When the cache is rebuilt

When any of these change:

- the preamble text (everything before `\begin{document}`)
- the profile's `class-options` or `pretex`
- the dependency search path
- any file the preamble read: a package, a path or git dependency, or the TeX
  distribution itself (compared by size and modification time)

Your main file is still the file TeX compiles, so line numbers in errors and
SyncTeX positions are exactly as without the cache.

## When a preamble can't be cached

Some packages don't survive being stored in a format. quire handles this on its
own:

- If the format can't be built, or the document fails with it but builds
  without it, quire builds normally and prints a warning.
- It won't try the cache again until the preamble changes.
- An error in the body of your document is reported straight away, with no
  extra compile.

To keep the rest of the preamble cached, put the troublesome package after
`\endofdump`:

```latex
\documentclass{article}
\usepackage{tikz,siunitx}   % cached
\endofdump
\usepackage{somepackage}    % loaded on every build
\begin{document}
```

## Requirements and switches

- Works with **pdflatex** only. XeTeX and LuaTeX can't reliably store their
  fonts in a format, so those engines always build normally.
- Needs the `mylatexformat` package, which comes with TeX Live and MiKTeX.
  `quire doctor` checks for it. On Debian and Ubuntu it is in
  `texlive-latex-extra`.
- Turn it off for a project or profile with `preamble-cache = false` in
  `[build]` or `[profile.<name>]`, or for one run with `QUIRE_PREAMBLE_CACHE=0`.
