# Environment variables

## Read by quire

| Variable | |
|---|---|
| `QUIRE_HOME` | Cache folder for git dependencies (default `~/.quire`). |
| `QUIRE_PREAMBLE_CACHE` | Set to `0` to build without the [preamble cache](../preamble-cache.md). |
| `TEXINPUTS`, `BIBINPUTS`, `BSTINPUTS` | If you set these, quire keeps your value and puts the dependency folders in front of it. |

## Set by quire for TeX

| Variable | |
|---|---|
| `TEXINPUTS` | `.`, then each dependency's folder (searched recursively), then your value or the default path. |
| `BIBINPUTS`, `BSTINPUTS` | The main file's folder, then the dependency folders, then your value or the default. |
| `TEXFORMATS` | On TeX Live, the folder holding the cached preamble. |

## Read by the install scripts

| Variable | |
|---|---|
| `QUIRE_VERSION` | Release to install, e.g. `v0.1.0` (default `latest`). |
| `QUIRE_INSTALL_DIR` | Install folder (default `~/.quire/bin`). |
| `QUIRE_NO_MODIFY_PATH` | `install.ps1`: set to `1` to leave your user `PATH` unchanged. |
| `QUIRE_TARGET` | `install.sh`: override the detected platform. |
| `QUIRE_DOWNLOAD_URL` | Base URL or local folder holding the release files. |
