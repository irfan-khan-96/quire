# Introduction

quire is a build tool and package manager for LaTeX, modelled on Rust's Cargo.

A LaTeX project usually doesn't record which packages it needs, where they come
from, or how to build it. That knowledge lives in someone's head, in a Makefile,
or in an editor setting. quire puts it in one file, `quire.toml`, and gives you
one command for the whole workflow:

```console
$ quire new thesis
     Created document `thesis` in `thesis`
$ cd thesis
$ quire add lab-macros --git https://github.com/mylab/lab-macros --tag v1.2
      Adding `lab-macros` (git https://github.com/mylab/lab-macros, tag v1.2)
     Cloning https://github.com/mylab/lab-macros
$ quire build
   Compiling thesis (pdflatex, profile `dev`)
     Caching preamble of `thesis`
    Finished `dev` profile in 4.12s → target/dev/thesis.pdf
```

What you get:

- **One manifest and a lockfile.** Dependencies on local folders and git
  repositories are resolved, including their own dependencies, and pinned to an
  exact commit in `quire.lock`. Everyone builds against the same macros.
- **Fast rebuilds.** The preamble is compiled once and reused, so rebuilding
  after an edit is often twice as fast as plain latexmk.
- **Clear errors.** quire reads the TeX log and shows each error with its file,
  line and source context, not the whole log.
- **arXiv submissions in one command.** `quire pack` produces a bundle that is
  checked to compile on its own, with the bibliography included and comments
  removed.
- **Ready for CI and editors.** `--locked` builds, JSON output and a GitHub Action.

quire runs on top of your existing TeX Live or MiKTeX installation; it does not
replace it.

This book is organised in three parts: a **user guide** that walks through the
features, **migration guides** for latexmk and Overleaf users, and a
**reference** for every setting and command.
