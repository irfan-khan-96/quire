# Profiles

A profile is a named set of build settings. Each profile builds into its own
directory, `target/<profile>/`, so switching between them doesn't throw away
the other's output.

## Built-in profiles

| Profile | Use | SyncTeX |
|---|---|---|
| `dev` | The default, for writing. | on |
| `release` | `quire build --release`, for sharing. `quire pack` uses it too. | off |

## Your own profiles

```toml
[profile.draft]
class-options = ["draft"]        # fast: no images, overfull boxes marked

[profile.print]
inherits = "release"
class-options = ["twoside", "a4paper"]
pretex = "\\def\\PrintVersion{}"
```

```console
$ quire build --profile draft
```

A custom profile starts from `dev` unless `inherits` names another profile.
Settings it gives replace the inherited ones.

| Setting | Meaning |
|---|---|
| `inherits` | Profile to start from (default `dev`). |
| `class-options` | Options passed to your document class, as if written in `\documentclass[...]`. |
| `pretex` | TeX code run before your main file. |
| `synctex` | Write SyncTeX data for editor forward and inverse search. |
| `shell-escape` | Allow `\write18`. Needed by `minted`. Off unless set here or in `[build]`. |
| `preamble-cache` | Use the [preamble cache](preamble-cache.md). |
| `latexmk-args` | Extra arguments for latexmk. |

## Checking the profile from TeX

quire defines `\QuireProfile` as the profile's name, so the document can
change with it:

```latex
\providecommand\QuireProfile{none}   % keeps the file compilable without quire
\usepackage{etoolbox}
\ifdefstring{\QuireProfile}{print}{\usepackage[hidelinks]{hyperref}}{\usepackage{hyperref}}
```
