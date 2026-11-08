# Dependencies

A quire dependency is anything you'd otherwise copy between projects: your
lab's macros, a university thesis class, a company letterhead, or a folder of
shared `.bib` files.

```toml
[dependencies]
lab-macros = { path = "../lab-macros" }
uni-thesis = { git = "https://github.com/uni/thesis-class", tag = "v2.1" }
siunitx    = "*"
```

## Kinds of dependency

| Kind | Example | Where it comes from |
|---|---|---|
| Path | `{ path = "../lab-macros" }` | A local folder, relative to the project. |
| Git | `{ git = "https://…", tag = "v1.0" }` | A git repository. Use at most one of `tag`, `branch` or `rev`; with none, the default branch is used. |
| Distribution | `"*"` | Your TeX Live or MiKTeX installation. quire warns if `kpsewhich` can't find it. |

Path and git dependencies are put on TeX's search path (`TEXINPUTS`, plus
`BIBINPUTS` and `BSTINPUTS` for BibTeX and Biber), searched recursively. So
`\usepackage{lab-macros}`, `\documentclass{uni-thesis}` and
`\bibliography{shared}` find their files with no copying and no paths in your
`.tex` files.

## Adding and removing

```console
$ quire add lab-macros --path ../lab-macros
$ quire add uni-thesis --git https://github.com/uni/thesis-class --tag v2.1
$ quire add siunitx
$ quire remove siunitx
```

`quire add` keeps the comments and formatting of `quire.toml`. If the new
dependency can't be resolved (a wrong path, a tag that doesn't exist), it puts
the manifest back as it was and shows the error.

## Packages

Any folder of `.sty` and `.cls` files works as a dependency. To give it
dependencies of its own, a version and documentation, make it a quire
**package**:

```console
$ quire new lab-macros --template package
```

```toml
[package]
name = "lab-macros"
version = "0.1.0"
tex-dir = "tex"        # the folder dependents see; defaults to the package root

[dependencies]
base-style = { git = "https://github.com/mylab/base-style" }

# Optional: the package's documentation, built by `quire build`.
[document]
name = "lab-macros-doc"
main = "doc/lab-macros-doc.tex"
```

Dependencies of dependencies are resolved transitively. Two rules keep this
predictable:

- A name has one source. If two packages ask for `base-style` from different
  places, quire stops and shows both.
- A package that comes from git may only use path dependencies inside its own
  repository.

## Locking and updating

The exact commit of every git dependency is written to `quire.lock`. Commit this
file, so collaborators and CI build against the same revisions.

```console
$ quire fetch                 # resolve and download; write quire.lock
$ quire update                # move every git dependency to the newest commit its tag or branch allows
$ quire update uni-thesis     # just one
```

A new upstream commit never changes your build until you run `quire update`.
See [quire.lock and the cache](reference/lockfile.md) for the details.
