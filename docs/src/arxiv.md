# Submitting to arXiv

```console
$ quire pack
   Compiling paper (pdflatex, profile `release`)
    Finished `release` profile in 9.17s → target/release/paper.pdf
   Verifying the bundle compiles on its own
      Packed 5 files (0.8 KB) → target/arxiv/paper-arxiv.zip
```

Upload `target/arxiv/paper-arxiv.zip` to arXiv. The same folder holds the
unpacked bundle, `target/arxiv/paper/`, if you want to look inside first.

## What goes in

quire builds the document, then takes exactly the files TeX read, from TeX's
own record of the build:

- **Your files** keep their paths relative to the main file (or to `root`, if
  set).
- **Files from path and git dependencies** are copied to the top of the bundle,
  because arXiv can't fetch them. Their comments are kept, since they often hold
  license text.
- **Files from the TeX distribution** are left out, because arXiv has its own.
  Any other file the build read is an error, because arXiv won't have it. That
  covers an absolute `\input`, or a file somewhere else on your disk.
- **The bibliography** goes in as `<main>.bbl`, already processed, because arXiv
  doesn't run Biber. If it came from biblatex, quire warns that arXiv only
  accepts it when the biblatex versions match.

The main file is changed in two ways:

- It starts with `\pdfoutput=1`, for pdflatex, which arXiv recommends.
- The profile's settings are written at the top, so `class-options` and
  `pretex` still apply.

**Comments are removed** the way arXiv's own cleaner does it:

- `% note` becomes `%`, which keeps the line-break behaviour.
- Lines that are only a comment, and `comment` environments, are dropped.
- `\verb`, `\url`, `\href` and verbatim environments are left alone.

## Checks

Before writing the zip, quire:

- refuses profiles with `shell-escape`, which arXiv doesn't allow
- rejects file names arXiv won't accept (only letters, digits and `_ + - . , = /`)
- warns if the bundle is over arXiv's usual 50 MB limit
- **compiles the bundle in a scratch folder** with no dependencies and no
  BibTeX or Biber, as arXiv will. Skip this with `--no-verify`.

The scratch build uses your local TeX distribution, not arXiv's TeX Live
version, so it can't catch differences between the two.

The zip has sorted entries and fixed timestamps, so the same sources always
produce the same bytes.

## Settings

```toml
[target.arxiv]
profile = "release"       # default
flatten = true            # inline \input/\include into the main file (default: false)
strip-comments = true     # default
include = ["src/data"]    # extra files the build record doesn't show, e.g. data read by a package
```

`include` paths are relative to the project root and must be inside the folder
TeX runs in (the main file's folder, or `root` in `[document]`), because arXiv
resolves paths from the top of the bundle.

That folder becomes the top of the bundle. When `root` puts the main file in a
subfolder of it, the bundle keeps that layout and gets a `00README.json` naming
the main file as the top-level file, so arXiv compiles it from the top folder
just as quire did. See [`root`](reference/manifest.md#document).
