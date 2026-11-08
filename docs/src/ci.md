# Continuous integration

## The GitHub Action

Build the PDF on every push and attach it to the workflow run:

```yaml
# .github/workflows/paper.yml
name: Paper
on: [push, pull_request]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: irfan-khan-96/quire@v0.1.0
```

The PDF appears under the run's **Artifacts** as `quire-output`.

The action installs TeX Live and quire, then runs `quire build --locked`. With
`--locked`, the build fails if `quire.lock` is missing or out of date, so CI
always uses the dependency revisions you committed.

### Pack for arXiv as well

```yaml
      - uses: irfan-khan-96/quire@v0.1.0
        with:
          command: pack
          artifact-name: arxiv-bundle
```

### Inputs

| Input | Default | |
|---|---|---|
| `command` | `build` | `build` (PDF) or `pack` (arXiv bundle). |
| `args` | | Extra arguments, e.g. `--release` or `--profile print`. |
| `working-directory` | `.` | The folder with `quire.toml`. |
| `version` | `latest` | quire release to install, e.g. `v0.1.0`. |
| `tex` | `standard` | `standard`: common LaTeX packages and Biber. `full`: all of TeX Live, which is slow. `none`: TeX is already installed. Linux runners only. |
| `locked` | `true` | Pass `--locked`. |
| `upload` | `true` | Upload the result as an artifact. |
| `artifact-name` | `quire-output` | |

The output `path` is a glob matching the produced files. Use it to attach the
PDF to a release, for example.

### Attach the PDF to a GitHub Release

```yaml
on:
  push:
    tags: ["v*"]
permissions:
  contents: write
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - id: quire
        uses: irfan-khan-96/quire@v0.1.0
        with:
          args: --release
      - run: gh release create "$GITHUB_REF_NAME" ${{ steps.quire.outputs.path }}
        env:
          GH_TOKEN: ${{ github.token }}
```

## Other CI systems

Install TeX and quire, then build with `--locked`:

```sh
curl -fsSL https://github.com/irfan-khan-96/quire/releases/latest/download/install.sh | sh
export PATH="$HOME/.quire/bin:$PATH"
quire build --locked --release
```

Useful flags for CI:

| Flag | |
|---|---|
| `--locked` | Fail instead of changing `quire.lock`. |
| `--offline` | Never use the network. Dependencies must already be cached in `~/.quire`. |
| `--frozen` | Both of the above. |
| `--message-format json` | Machine-readable errors and results ([JSON messages](reference/json.md)). |

Cache `~/.quire` between runs to skip cloning git dependencies again.
