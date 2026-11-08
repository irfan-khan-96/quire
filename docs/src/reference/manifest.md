# quire.toml

Every key quire understands. Unknown top-level sections are reported as
warnings, and unknown keys inside known sections are errors, so typos don't go
unnoticed.

## `[document]`

A buildable document. A manifest needs `[document]`, `[package]`, or both.

| Key | Default | |
|---|---|---|
| `name` | required | Output name: `target/<profile>/<name>.pdf`. Letters, digits, `-` and `_`. |
| `main` | `"src/main.tex"` | The main `.tex` file, relative to `quire.toml`. |
| `engine` | `"pdflatex"` | `"pdflatex"`, `"xelatex"` or `"lualatex"`. |
| `texlive` | | TeX Live release to target. Recorded, but not enforced yet. |

## `[package]`

A reusable package that other projects can depend on.

| Key | Default | |
|---|---|---|
| `name` | required | Must match the name dependents use. |
| `version` | required | E.g. `"0.1.0"`. |
| `description` | | |
| `tex-dir` | `"."` | Folder added to dependents' search path. |

## `[dependencies]`

```toml
[dependencies]
name = "*"                                         # TeX distribution package
name = { version = ">=3.3" }                       # the same, as a table
name = { path = "../folder" }                      # local folder
name = { git = "https://…" }                       # default branch
name = { git = "https://…", tag = "v1.0" }         # or branch = "…", or rev = "<commit>"
```

See [Dependencies](../dependencies.md).

## `[build]`

Settings for every profile.

| Key | Default | |
|---|---|---|
| `shell-escape` | `false` | Allow `\write18`, e.g. for `minted`. |
| `preamble-cache` | `true` | Cache the compiled preamble (pdflatex). |
| `latexmk-args` | `[]` | Extra latexmk arguments. |

## `[profile.<name>]`

Built-in profiles are `dev` and `release`. Custom profiles start from `dev`,
or from the profile named in `inherits`. See [Profiles](../profiles.md).

| Key | |
|---|---|
| `inherits` | Profile to start from. Not allowed on `dev` and `release`. |
| `class-options` | List of document class options. |
| `pretex` | TeX code run before the main file. |
| `synctex` | `true` or `false`. |
| `shell-escape` | `true` or `false`. |
| `preamble-cache` | `true` or `false`. |
| `latexmk-args` | List; replaces the inherited list. |

## `[target.arxiv]`

Settings for `quire pack`. See [Submitting to arXiv](../arxiv.md).

| Key | Default | |
|---|---|---|
| `profile` | `"release"` | Profile to pack. |
| `flatten` | `false` | Inline `\input` and `\include` files into the main file. |
| `strip-comments` | `true` | Remove comments from your `.tex` files. |
| `include` | `[]` | Extra files or folders to ship, relative to the project root. |
