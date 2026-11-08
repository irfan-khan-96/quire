# Installation

## 1. A TeX distribution

quire drives an existing TeX installation. Install one of:

- **TeX Live** (Linux, macOS, Windows): <https://tug.org/texlive/>. On macOS,
  **MacTeX** is TeX Live. On Debian or Ubuntu, `apt install latexmk
  texlive-latex-extra` is a good start.
- **MiKTeX** (Windows, macOS, Linux): <https://miktex.org/>.

Both include `latexmk`, which quire uses to run the passes. latexmk needs Perl,
which TeX Live bundles on Windows. MiKTeX users may need to install Perl, for
example Strawberry Perl.

## 2. quire

**Linux and macOS:**

```sh
curl -fsSL https://github.com/irfan-khan-96/quire/releases/latest/download/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://github.com/irfan-khan-96/quire/releases/latest/download/install.ps1 | iex
```

Both scripts download the build for your platform from the latest GitHub
Release and check its SHA-256 checksum before installing.

- On Linux and macOS, `quire` goes into `~/.quire/bin`, and the script tells you
  the line to add to your shell profile.
- On Windows, `quire.exe` goes into `%USERPROFILE%\.quire\bin`, which the script
  adds to your user `PATH`. Set `QUIRE_NO_MODIFY_PATH=1` to stop it changing
  `PATH`.

To install a specific version, set `QUIRE_VERSION=v0.1.0`. To install somewhere
else, set `QUIRE_INSTALL_DIR`.

You can also download an archive from the
[Releases page](https://github.com/irfan-khan-96/quire/releases) yourself, or
build from source with Rust 1.89 or newer:

```sh
cargo install --git https://github.com/irfan-khan-96/quire --locked
```

## 3. Check the setup

```console
$ quire doctor
  ok       latexmk       Latexmk, John Collins, 9 March 2026. Version 4.88
  ok       perl          This is perl 5, version 42 ...
  ok       pdflatex      MiKTeX-pdfTeX 4.27 (MiKTeX 26.5)
  ...
  ok       mylatexformat C:/.../mylatexformat.ltx
```

A missing required tool is marked `missing` in red. Optional tools such as
`biber` or `mylatexformat` are marked in yellow, with what they're needed for.
