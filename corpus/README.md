# Corpus test

Builds real arXiv papers with quire to find what breaks before users do.
`papers.txt` lists about 250 papers submitted in the first half of 2025, around
ten from each of 29 categories, from `math.AP` to `econ.EM`. Each category has
its own LaTeX habits: revtex, amsart, IEEEtran, Biber, latin-1 sources, and
standalone figures.

For each paper, `run.py`:

1. downloads the source from arXiv, at most one request every three seconds
   and with retries when rate limited, and caches it in `cache/`
2. runs `quire init` on the sources as they are, then `quire build`
3. edits the main file and builds again, which exercises the preamble cache
4. runs `quire pack`, which compiles the bundle in isolation
5. if the first build failed, builds a fresh copy with plain `latexmk`

| Result | Meaning |
|---|---|
| `ok` | Build, rebuild and pack all succeeded. |
| `quire` | quire failed where plain latexmk succeeded, or the rebuild or the pack failed after a good build. These are bugs. |
| `paper` | Plain latexmk fails too, for example a missing package or a DVI-only paper. |
| `skipped` | arXiv has no TeX source (PDF-only submission). |
| `unavailable` | The download failed even after retries. |

The run fails when a paper lands in `quire` and is not listed in
`known-issues.txt`.

## Running it

The nightly [Corpus workflow](../.github/workflows/corpus.yml) runs the whole
list in six shards against the full TeX Live image, and writes the summary to
the run page. It also runs on pull requests that change `corpus/`, and you can
start it by hand from the Actions tab.

Locally:

```sh
cargo build --release
python corpus/run.py --quire target/release/quire --limit 10 --keep
python corpus/run.py --quire target/release/quire --only 2501.00687v3
```

`--keep` leaves the folders of failed papers in `work/` for debugging. A local
run compiles third-party sources with your TeX installation, so prefer a
sandbox or a container.

`python corpus/select.py` picks a new list from the arXiv API. Commit the
result, because the list should only change on purpose.
