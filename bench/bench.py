#!/usr/bin/env python3
"""Benchmark quire's preamble cache against plain latexmk.

For each document in bench/docs, measures the edit-and-rebuild loop: change one
word in the body, rebuild, and record the wall time. Each configuration warms up
once (so caches exist) and then runs N times; the table shows medians.

    python bench/bench.py [--quire PATH] [--runs N]

Requires latexmk, pdflatex and the packages the documents use.
"""

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANIFEST = """[document]
name = "{name}"
main = "src/main.tex"

[profile.nocache]
preamble-cache = false
"""


def timed(cmd, cwd, env=None):
    start = time.perf_counter()
    result = subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    elapsed = time.perf_counter() - start
    if result.returncode != 0:
        sys.exit(f"command failed in {cwd}: {' '.join(cmd)}\n{result.stderr[-2000:]}")
    return elapsed


def edit(main, counter):
    text = main.read_text()
    start = text.index("REVISION")
    end = text.index(".", start)
    main.write_text(text[:start] + f"REVISION {counter}" + text[end:])


def bench(doc, quire, runs, env):
    work = Path(tempfile.mkdtemp(prefix=f"quire-bench-{doc.stem}-"))
    try:
        (work / "src").mkdir()
        main = work / "src" / "main.tex"
        shutil.copy(doc, main)
        (work / "quire.toml").write_text(MANIFEST.format(name=doc.stem))

        configs = {
            "latexmk": (["latexmk", "-pdf", "-synctex=1", "-interaction=nonstopmode", "-outdir=../target/plain",
                         "main.tex"], work / "src"),
            "quire (no cache)": ([quire, "build", "--profile", "nocache"], work),
            "quire (cached)": ([quire, "build"], work),
        }
        cold = {}
        results = {}
        counter = 0
        for label, (cmd, cwd) in configs.items():
            cold[label] = timed(cmd, cwd, env)  # first build, also creates caches
            times = []
            for _ in range(runs):
                counter += 1
                edit(main, counter)
                times.append(timed(cmd, cwd, env))
            results[label] = statistics.median(times)
        return cold, results
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser()
    default_quire = HERE.parent / "target" / "release" / ("quire.exe" if os.name == "nt" else "quire")
    parser.add_argument("--quire", default=str(default_quire))
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()

    env = dict(os.environ, QUIRE_HOME=tempfile.mkdtemp(prefix="quire-bench-home-"))
    env.pop("QUIRE_PREAMBLE_CACHE", None)
    version = subprocess.run(["pdflatex", "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    print(f"{version}; median of {args.runs} edit-and-rebuild runs\n")
    print("| Document | latexmk | quire, no cache | quire, cached | Speed-up vs latexmk | First build (cached) |")
    print("|---|---:|---:|---:|---:|---:|")
    for doc in sorted((HERE / "docs").glob("*.tex")):
        cold, r = bench(doc, args.quire, args.runs, env)
        speedup = r["latexmk"] / r["quire (cached)"]
        print(f"| {doc.stem} | {r['latexmk']:.2f}s | {r['quire (no cache)']:.2f}s | {r['quire (cached)']:.2f}s "
              f"| {speedup:.1f}x | {cold['quire (cached)']:.2f}s |", flush=True)


if __name__ == "__main__":
    main()
