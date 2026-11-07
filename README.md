# quire

A Cargo-like build tool and package manager for LaTeX.

```
$ quire new thesis --engine lualatex
     Created document `thesis` in `thesis`
$ cd thesis
$ quire add lab-macros --git https://github.com/mylab/lab-macros --tag v1.2
      Adding `lab-macros` (git https://github.com/mylab/lab-macros, tag v1.2)
     Cloning https://github.com/mylab/lab-macros
$ quire build
   Compiling thesis (lualatex, profile `dev`)
    Finished `dev` profile in 2.31s → target/dev/thesis.pdf
```

LaTeX projects don't say which packages they need, where those packages come from,
or how to build them. quire adds one manifest (`quire.toml`), a lockfile
(`quire.lock`), and one command for the whole workflow.

## Requirements

- TeX Live or MiKTeX (provides `latexmk`, the engines, and `kpsewhich`); latexmk needs Perl.
- `git`, for git dependencies.

Run `quire doctor` to check your setup.

## Install

```bash
cargo install --path .
```

## Commands

| Command | What it does |
|---|---|
| `quire new <dir>` / `quire init` | Create a project (`--template article\|report\|beamer\|package`, `--engine`). `init` adopts an existing `.tex` file with a `\documentclass`. |
| `quire build` | Compile to `target/<profile>/<name>.pdf`. `--profile <p>`, `--release`, `--open`, `-v` for full latexmk output. |
| `quire watch` | Rebuild on changes to the project or any path dependency. |
| `quire add <name>` | Add a dependency: `--path <dir>`, `--git <url> [--tag\|--branch\|--rev]`, or a TeX distribution package (`--version`). |
| `quire remove <name>` | Remove a dependency. |
| `quire fetch` | Resolve and download dependencies, write `quire.lock`. |
| `quire update [names]` | Move git dependencies to the newest commit their branch or tag allows. |
| `quire pack` | Bundle the sources into `target/arxiv/<name>-arxiv.zip`, checked to compile on their own. |
| `quire clean` | Delete `target/` (or one `--profile`). |
| `quire doctor` | Check that the external tools are installed. |

### Global flags

| Flag | Effect |
|---|---|
| `--locked` | Fail instead of changing `quire.lock`. Use it in CI so builds use exactly the committed revisions. |
| `--offline` | Never touch the network; use only `~/.quire`. Fails if something isn't cached. |
| `--frozen` | Both `--locked` and `--offline`. |

### Machine-readable output

`quire build --message-format json` (and `quire watch`) print one JSON object per
line on stdout, like `cargo --message-format=json`. Status lines stay on stderr.

```json
{"reason":"compiler-message","document":"thesis","profile":"dev","message":{"level":"error","message":"Undefined control sequence.","file":"C:/thesis/src/ch1.tex","line":42,"context":["l.42 \\foo"],"rendered":"..."}}
{"reason":"document-artifact","document":"thesis","profile":"dev","pdf":"C:/thesis/target/dev/thesis.pdf","log":"C:/thesis/target/dev/thesis.log"}
{"reason":"build-finished","success":true}
```

Warnings are reported the same way with `"level":"warning"`. Editors and CI can
use these to put diagnostics on the right file and line.

### Concurrency

quire takes file locks, as Cargo does. One covers the shared git cache in
`~/.quire`, and one covers each `target/<profile>` directory. Builds and
dependency fetches running in parallel wait for each other (`Blocking waiting
for file lock ...`) and never corrupt each other's files. Git clones are written
to a temporary directory and renamed into place, so an interrupted clone never
leaves a broken cache.

## quire.toml

```toml
[document]
name = "thesis"            # output is target/<profile>/thesis.pdf
main = "src/main.tex"      # default
engine = "lualatex"        # pdflatex (default) | xelatex | lualatex
texlive = "2025"           # recorded; not enforced yet (see roadmap)

[dependencies]
siunitx    = "*"                                    # from your TeX distribution
lab-macros = { path = "../lab-macros" }             # a local directory
uni-thesis = { git = "https://github.com/org/uni-thesis", tag = "v1.0" }

[build]
shell-escape = false       # needed by e.g. minted; off by default
preamble-cache = true      # cache the compiled preamble (pdflatex); on by default
latexmk-args = []          # extra latexmk arguments for every profile

[profile.draft]            # quire build --profile draft
class-options = ["draft"]  # passed to the document class

[profile.print]
inherits = "release"
class-options = ["twoside"]
pretex = "\\def\\PrintVersion{}"   # TeX run before your main file
```

### Profiles

The built-in profiles are `dev` (the default, with SyncTeX) and `release` (no SyncTeX).
Custom profiles inherit from `dev` unless `inherits` says otherwise. Each profile has
its own output directory. Your document can check `\QuireProfile`. Add
`\providecommand\QuireProfile{none}` to keep it compilable without quire.

### Dependencies

- **Path and git dependencies** are added to TeX's search path (`TEXINPUTS`,
  `BIBINPUTS` and `BSTINPUTS`, searched recursively), so `\usepackage`,
  `\documentclass` and `.bib` files in them just work. A dependency can be any
  directory of `.sty`/`.cls` files, or a quire **package** whose own dependencies
  are resolved transitively.
- **Git dependencies** are cached in `~/.quire/git` (override with `QUIRE_HOME`),
  one checkout per commit, shared across projects. The exact commit is recorded in
  `quire.lock`; commit the lockfile so everyone builds against the same revision.
- **Distribution dependencies** (`siunitx = "*"`) still come from your installed
  TeX Live or MiKTeX for now. quire warns if `kpsewhich` cannot find them.

A dependency name that is requested from two different sources is an error. A git
package's path dependencies must stay inside its repository.

### Packages

`quire new my-style --template package` creates a reusable package:

```toml
[package]
name = "my-style"
version = "0.1.0"
tex-dir = "tex"      # directory exposed to dependents

[document]           # optional: the package's documentation
name = "my-style-doc"
main = "doc/my-style-doc.tex"
```

## Fast rebuilds: the preamble cache

Most of a LaTeX rebuild goes into loading the preamble again: TikZ, fonts,
hyperref, siunitx. With pdflatex, quire compiles the preamble once into a format
file (`target/<profile>/preamble/`) and starts later builds from it, so editing
the body only pays for the body.

Median edit-and-rebuild time (change one word, rebuild):

| Document | latexmk | quire, no cache | quire, cached | Speed-up |
|---|---:|---:|---:|---:|
| [paper](bench/docs/paper.tex): microtype, AMS, siunitx, TikZ, hyperref, cleveref | 5.82s | 5.92s | **3.30s** | 1.8× |
| [heavy](bench/docs/heavy.tex): paper plus pgfplots and TikZ libraries | 6.41s | 6.21s | **3.07s** | 2.1× |
| [beamer](bench/docs/beamer.tex): beamer, Madrid theme, TikZ | 7.04s | 5.39s | **2.90s** | 2.4× |
| [light](bench/docs/light.tex): article and amsmath | 2.36s | 2.39s | 2.20s | 1.1× |

These were measured with MiKTeX 26.5 (pdfTeX 1.40.29) on Windows 11 and include
latexmk's own startup. Run `python bench/bench.py` to measure on your machine.

How it works and stays correct:

- **The cache is rebuilt when anything it depends on changes:** the preamble text,
  the profile's settings, the dependency search path, or any file the preamble
  read (packages, path/git dependencies, the TeX distribution itself, compared by
  size and modification time).
- **Your main file is still the file TeX compiles**, so error line numbers and
  SyncTeX are unaffected.
- **Preambles that can't be cached fall back automatically.** If the format can't
  be built, or the document fails with it but builds without it, quire builds
  normally and remembers not to try again until the preamble changes. Errors in
  the body are reported straight away, without a second compile.
- **To keep one package out of the cache**, put it after `\endofdump`. Everything
  before that line is still cached:

  ```latex
  \usepackage{tikz,siunitx}   % cached
  \endofdump
  \usepackage{somepackage}    % loaded on every build
  \begin{document}
  ```

The cache needs the `mylatexformat` package, which comes with TeX Live and
MiKTeX (`quire doctor` checks for it). It is used with pdflatex only, because
XeTeX and LuaTeX can't reliably store their fonts in a format. Turn it off with
`preamble-cache = false` in `[build]` or in a profile, or for one run with
`QUIRE_PREAMBLE_CACHE=0`.

## Submitting to arXiv

```
$ quire pack
   Compiling paper (pdflatex, profile `release`)
    Finished `release` profile in 9.17s → target/release/paper.pdf
   Verifying the bundle compiles on its own
      Packed 5 files (0.8 KB) → target/arxiv/paper-arxiv.zip
```

`quire pack` builds the document, then puts in the zip exactly the files TeX
read, as recorded by its `-recorder` output:

- **Project files** keep their paths relative to the main file.
- **Files from path and git dependencies** are copied next to the main file,
  because arXiv can't fetch them. Their comments are kept, since they often hold
  license text.
- **Files from the TeX distribution** are left out, because arXiv has its own.
  Any other file the build read (an absolute `\input`, a file somewhere else on
  your disk) is an error, because arXiv won't have it.
- **The bibliography** goes in as `<main>.bbl`, because arXiv doesn't run Biber.
  quire warns if it came from biblatex, since arXiv only accepts that if the
  biblatex versions match.
- **The main file** starts with `\pdfoutput=1` (for pdflatex), followed by the
  profile's settings, so class options and `pretex` still apply.
- **Comments are stripped** the way arXiv's own cleaner does: `% ...` becomes `%`,
  comment-only lines and `comment` environments go away, and `\verb`, `\url`,
  `\href` and verbatim environments are left alone.

Before zipping, quire checks the bundle:
- It refuses `shell-escape`, which arXiv doesn't allow.
- It rejects file names arXiv won't accept.
- It warns past arXiv's usual 50 MB limit.
- It compiles the bundle in a scratch folder with no dependencies and no
  BibTeX/Biber, as arXiv will.

The zip has sorted entries and fixed timestamps, so the same sources always
produce the same bytes.

```toml
[target.arxiv]
profile = "release"        # default
flatten = true             # inline \input/\include into the main file (default: false)
strip-comments = true      # default
include = ["src/data"]     # extra files the recorder can't see, e.g. data read by a package
```

The isolated build check uses your local TeX distribution, not arXiv's TeX Live
version. Use `--no-verify` to skip it.

## How a build works

1. Dependencies are resolved (using `quire.lock` for git revisions) and the lockfile is updated.
2. quire writes `target/<profile>/quire-driver.tex`. It sets `\QuireProfile`, passes
   the class options, runs `pretex`, then `\input`s your main file. It also lists
   the dependency directories, so changing dependencies triggers a rebuild.
3. latexmk runs in the main file's directory with the dependency search path set,
   and handles the repeated passes and the bibliography.
4. On failure, quire prints the errors from the TeX log with their source lines.

## Roadmap

- **Phase 3, TeX Live packages:** read `texlive.tlpdb`, detect a document's packages
  by recording the files a build opens (`-recorder`) and mapping them to TeX Live
  packages, then lock each one to a TeX Live year, revision and checksum.
- **Phase 4:** a shared, checksum-verified cache that builds a per-project TeX tree
  from a frozen TeX Live archive.
- **Phase 5:** checking bundles against arXiv's exact TeX Live version (needs
  phase 3), journal targets (`--target ieee|acm|springer`: fonts embedded, no
  Type 3 fonts, PDF/A), and features.
- **Phase 6:** pin the engine and fonts, `quire publish` to CTAN, and third-party
  `quire-<cmd>` subcommands.

## Development

```bash
cargo test
```

The integration tests in `tests/cli.rs` skip the git and latexmk cases when those
tools are not installed. CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`
and `clippy -D warnings`. It runs the tests on Linux (with TeX Live, so every
test runs) and on macOS and Windows (where the LaTeX build tests skip).
