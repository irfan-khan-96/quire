# Commands

Run `quire help <command>` for the full list of options.

| Command | |
|---|---|
| `quire new <dir>` | Create a project in a new folder. `--template article\|report\|beamer\|package`, `--engine`, `--name`, `--vcs git\|none`. |
| `quire init` | Create a project in the current folder, adopting an existing main file. Same options as `new`. |
| `quire build` (`b`) | Compile. `--profile <name>`, `--release`, `--open`, `-v`, `--message-format human\|json`. |
| `quire watch` (`w`) | Rebuild when the project or a path dependency changes. Same options as `build`. |
| `quire pack` | Make a submission bundle. `--target arxiv` (default), `--profile`, `--no-verify`, `-v`. |
| `quire add <name>` | Add a dependency: `--path <dir>`, `--git <url> [--tag\|--branch\|--rev]`, or `--version` for a distribution package. |
| `quire remove <name>` (`rm`) | Remove a dependency. |
| `quire fetch` | Resolve and download dependencies, write `quire.lock`. |
| `quire update [names…]` | Move git dependencies to the newest commit their tag or branch allows. |
| `quire clean` | Delete `target/`, or one profile's output with `--profile`. |
| `quire doctor` | Check that latexmk, the engines and the other tools are installed. |

## Global flags

These work with every command:

| Flag | |
|---|---|
| `--locked` | Fail instead of changing `quire.lock`. |
| `--offline` | Never use the network; use only `~/.quire`. |
| `--frozen` | `--locked` and `--offline`. |

## Exit status

`0` on success, `1` on any failure, including a TeX error. `quire doctor`
exits with `1` if a required tool is missing.
