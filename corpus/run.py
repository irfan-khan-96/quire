#!/usr/bin/env python3
"""Run quire over real arXiv papers and report where it breaks.

For every paper in papers.txt:

  1. download its source from arXiv (cached in corpus/cache)
  2. `quire init` adopts it, then `quire build`
  3. edit the main file and `quire build` again (the preamble-cache path)
  4. `quire pack`, which compiles the bundle in isolation
  5. if the first build failed, build a pristine copy with plain latexmk

Failures are classified:

  quire     quire failed where plain latexmk succeeded, or the rebuild or the
            pack failed after a good build. These are quire bugs, and the run
            fails unless the paper is listed in known-issues.txt.
  paper     plain latexmk fails as well (missing packages, DVI-only papers, ...)
  skipped   no TeX source on arXiv (PDF-only submissions)
  unavailable  the download failed even after retries

    python corpus/run.py [--quire PATH] [--shard I/N] [--limit N] [--only ID ...]
    python corpus/run.py --fetch-only      # fill the cache, nothing else
"""

import argparse
import gzip
import http.client
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
EPRINT = "https://export.arxiv.org/e-print/{}"
USER_AGENT = "quire-corpus/0.1 (+https://github.com/irfan-khan-96/quire)"
TIMEOUT = 600
EDIT = "\n% quire corpus: edited to test the rebuild\n"
LATEXMK_ENGINE = {"pdflatex": "-pdf", "xelatex": "-pdfxe", "lualatex": "-pdflua"}

_download_lock = threading.Lock()
_last_download = 0.0


def read_list(path):
    entries = []
    if not path.exists():
        return entries
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            entries.append(line.split(None, 1))
    return entries


# --- Fetching ------------------------------------------------------------------------


def fetch(paper, cache):
    """The paper's e-print as bytes, downloading it at most once."""
    global _last_download
    blob = cache / f"{paper}.eprint"
    if blob.exists():
        return blob.read_bytes()
    with _download_lock:
        delay = 15
        for attempt in range(6):
            # arXiv asks automated clients to wait three seconds between requests.
            wait = _last_download + 3 - time.monotonic()
            if wait > 0:
                time.sleep(wait)
            request = urllib.request.Request(EPRINT.format(paper), headers={"User-Agent": USER_AGENT})
            try:
                with urllib.request.urlopen(request, timeout=120) as response:
                    data = response.read()
                break
            except (urllib.error.URLError, http.client.HTTPException, TimeoutError) as e:
                status = getattr(e, "code", None)
                if (status is not None and status not in (429, 500, 502, 503, 504)) or attempt == 5:
                    raise
                # Rate limited or cut off: back off, as long as the server asks.
                retry_after = getattr(e, "headers", None) and e.headers.get("Retry-After")
                pause = int(retry_after) if retry_after and retry_after.isdigit() else delay
                print(f"  {paper}: {e}; retrying in {pause}s", flush=True)
                time.sleep(min(pause, 300))
                delay *= 2
            finally:
                _last_download = time.monotonic()
    cache.mkdir(parents=True, exist_ok=True)
    tmp = blob.with_suffix(".part")
    tmp.write_bytes(data)
    tmp.replace(blob)
    return data


def unpack(data, dest):
    """Unpack an e-print into `dest`. Returns False when it holds no TeX source."""
    dest.mkdir(parents=True)
    if data[:2] == b"\x1f\x8b":
        try:
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
                tar.extractall(dest, filter="data")
            return True
        except tarfile.ReadError:
            data = gzip.decompress(data)
    if data[:4] == b"%PDF":
        return False
    try:
        with tarfile.open(fileobj=io.BytesIO(data)) as tar:
            tar.extractall(dest, filter="data")
        return True
    except tarfile.ReadError:
        pass
    # A single gzipped .tex file; arXiv names it after the paper.
    (dest / "main.tex").write_bytes(data)
    return True


def readme_hints(src):
    """The top-level file and compiler from arXiv's 00README.json or 00README.XXX."""
    main, compiler = None, None
    readme = src / "00README.json"
    if readme.exists():
        try:
            spec = json.loads(readme.read_text(encoding="utf-8"))
            for source in spec.get("sources", []):
                if source.get("usage") == "toplevel":
                    main = main or source.get("filename")
            compiler = spec.get("process", {}).get("compiler")
        except (ValueError, AttributeError):
            pass
    legacy = src / "00README.XXX"
    if legacy.exists():
        for line in legacy.read_text(encoding="utf-8", errors="replace").splitlines():
            parts = line.split()
            if len(parts) == 2 and parts[1] == "toplevelfile":
                main = main or parts[0]
    return main, compiler


def documentclass_files(src):
    """Every .tex file with an uncommented \\documentclass, for ambiguity reporting."""
    found = []
    for path in sorted(src.rglob("*.tex")):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if re.search(r"^[^%\n]*\\documentclass", text, re.MULTILINE):
            found.append(path.relative_to(src).as_posix())
    return found


# --- Running -------------------------------------------------------------------------


def run(cmd, cwd, timeout=TIMEOUT):
    """Run a command, killing the whole process tree on timeout."""
    start = time.perf_counter()
    kwargs = {}
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    proc = subprocess.Popen(
        cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, stdin=subprocess.DEVNULL,
        text=True, encoding="utf-8", errors="replace", **kwargs,
    )
    try:
        out, err = proc.communicate(timeout=timeout)
        code = proc.returncode
    except subprocess.TimeoutExpired:
        if os.name == "nt":
            subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True)
        else:
            os.killpg(proc.pid, 9)
        out, err = proc.communicate()
        code = "timeout"
    return code, out, err, time.perf_counter() - start


def first_error(code, out, err):
    """A one-line reason for a failed quire command."""
    if code == "timeout":
        return f"timed out after {TIMEOUT}s"
    for line in out.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-message" and msg["message"].get("level") == "error":
            return msg["message"]["rendered"].splitlines()[0][:300]
    lines = [line for line in err.splitlines() if line.strip()]
    for i, line in enumerate(lines):
        if line.startswith("error"):
            return " | ".join(lines[i:i + 3])[:300]
    return (lines[-1] if lines else f"exit code {code}")[:300]


def latex_error(out):
    for line in out.splitlines():
        if re.match(r"^(\S+:\d+: |! )", line):
            return line[:300]
    return None


def write_manifest(work, main, engine):
    text = (work / "quire.toml").read_text(encoding="utf-8")
    text = re.sub(r'(?m)^main = ".*"$', f'main = "{main}"', text)
    text = re.sub(r'(?m)^engine = ".*"$', f'engine = "{engine}"', text)
    (work / "quire.toml").write_text(text, encoding="utf-8")


def test_paper(paper, category, args):
    result = shorten_paths(_test_paper(paper, category, args), args.work / paper)
    if result["class"] == "ok" or not args.keep:
        shutil.rmtree(args.work / paper, ignore_errors=True)
    return result


def shorten_paths(result, work):
    """Make reasons readable: `.../work/<id>/src/main.tex:3: ...` -> `main.tex:3: ...`."""
    for key in ("reason", "latexmk_reason"):
        if result.get(key):
            for root in (work / "src", work / "plain"):
                for form in (str(root) + os.sep, root.as_posix() + "/"):
                    result[key] = result[key].replace(form, "")
    return result


def _test_paper(paper, category, args):
    result = {"id": paper, "category": category}
    work = args.work / paper
    shutil.rmtree(work, ignore_errors=True)
    try:
        data = fetch(paper, args.cache)
    except Exception as e:  # network trouble is not a quire failure
        return {**result, "class": "unavailable", "reason": f"download failed: {e}"}
    if not unpack(data, work / "src"):
        return {**result, "class": "skipped", "reason": "no TeX source (PDF only)"}
    src = work / "src"

    readme_main, compiler = readme_hints(src)
    candidates = documentclass_files(src)
    result["candidates"] = len(candidates)
    if not candidates:
        return {**result, "class": "skipped", "reason": "no file with \\documentclass"}

    # Step 2: let quire adopt the sources as-is, the way a user would.
    quire = str(args.quire)
    code, out, err, _ = run([quire, "init", "--vcs", "none", "--name", "paper"], src)
    if code != 0:
        return {**result, "class": "quire", "stage": "init", "reason": first_error(code, out, err)}
    manifest = (src / "quire.toml").read_text(encoding="utf-8")
    init_main = re.search(r'(?m)^main = "(.*)"$', manifest).group(1)
    engine = re.search(r'(?m)^engine = "(.*)"$', manifest).group(1)
    main = readme_main or init_main
    result["main"] = main
    result["engine"] = engine
    if readme_main and readme_main != init_main:
        # init should have followed the README; record it and build the right file anyway.
        result["init_main"] = init_main
        write_manifest(src, main, engine)
    if compiler and compiler not in LATEXMK_ENGINE and compiler != "pdftex":
        return {**result, "class": "paper", "stage": "build", "reason": f"needs the `{compiler}` compiler (DVI)"}

    # Step 3: the first build.
    code, out, err, elapsed = run([quire, "build", "--message-format", "json"], src)
    result["build_s"] = round(elapsed, 1)
    if code != 0:
        result["stage"] = "build"
        result["reason"] = first_error(code, out, err)
        # Step 5: does it build without quire?
        plain = work / "plain"
        unpack(data, plain)
        flags = ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error"]
        code, out, _, elapsed = run(["latexmk", LATEXMK_ENGINE[engine], *flags, main], plain)
        result["latexmk_s"] = round(elapsed, 1)
        if code == 0:
            result["class"] = "quire"
        else:
            result["class"] = "paper"
            result["latexmk_reason"] = latex_error(out) or ("timed out" if code == "timeout" else "latexmk failed")
        return result

    # Step 3b: edit the body and rebuild, which uses the cached preamble.
    main_path = src / main
    with open(main_path, "a", encoding="utf-8", newline="") as f:
        f.write(EDIT)
    code, out, err, elapsed = run([quire, "build", "--message-format", "json"], src)
    result["rebuild_s"] = round(elapsed, 1)
    if code != 0:
        return {**result, "class": "quire", "stage": "rebuild", "reason": first_error(code, out, err)}

    # Step 4: pack and verify the arXiv bundle.
    code, out, err, elapsed = run([quire, "pack"], src)
    result["pack_s"] = round(elapsed, 1)
    if code != 0:
        return {**result, "class": "quire", "stage": "pack", "reason": first_error(code, out, err)}
    return {**result, "class": "ok"}


# --- Reporting -----------------------------------------------------------------------


def summarize(results, known):
    counts = {}
    for r in results:
        counts[r["class"]] = counts.get(r["class"], 0) + 1
    tested = len(results) - counts.get("skipped", 0) - counts.get("unavailable", 0)
    lines = ["# quire corpus results", ""]
    lines.append(f"{len(results)} papers, {tested} tested: "
                 + ", ".join(f"**{counts.get(c, 0)}** {c}" for c in ("ok", "quire", "paper", "skipped")))
    if counts.get("unavailable"):
        lines.append("")
        lines.append(f"**{counts['unavailable']}** could not be downloaded from arXiv and were not tested.")
    buildable = counts.get("ok", 0) + counts.get("quire", 0)
    if buildable:
        lines.append("")
        lines.append(f"quire handles **{counts.get('ok', 0)} of {buildable}** papers that plain latexmk can build "
                     f"({100 * counts.get('ok', 0) / buildable:.0f}%).")
    ok = [r for r in results if r["class"] == "ok"]
    if ok:
        first = sorted(r["build_s"] for r in ok)
        again = sorted(r["rebuild_s"] for r in ok)
        lines.append(f"Median first build {first[len(first) // 2]}s, median rebuild after an edit "
                     f"{again[len(again) // 2]}s.")

    for cls, title in (("quire", "quire failures"), ("paper", "Papers that plain latexmk cannot build either")):
        rows = [r for r in results if r["class"] == cls]
        if not rows:
            continue
        lines += ["", f"## {title}", "", "| Paper | Stage | Reason |", "|---|---|---|"]
        for r in sorted(rows, key=lambda r: r["id"]):
            reason = (r.get("latexmk_reason") if cls == "paper" else None) or r.get("reason")
            note = f" (known: {known[r['id']]})" if r["id"] in known else ""
            reason = (reason or "").replace("|", "\\|")
            lines.append(f"| [{r['id']}](https://arxiv.org/abs/{r['id']}) | {r.get('stage', '')} | `{reason}`{note} |")

    mismatched = [r for r in results if "init_main" in r]
    if mismatched:
        lines += ["", "## `quire init` picked a different main file than the arXiv README", ""]
        for r in mismatched:
            lines.append(f"- {r['id']}: README says `{r['main']}`, init picked `{r['init_main']}`")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--quire", type=Path, default=shutil.which("quire"))
    parser.add_argument("--papers", type=Path, default=HERE / "papers.txt")
    parser.add_argument("--cache", type=Path, default=HERE / "cache")
    parser.add_argument("--work", type=Path, default=HERE / "work")
    parser.add_argument("--out", type=Path, default=HERE / "results.json")
    parser.add_argument("--summary", type=Path, help="also write the Markdown summary here")
    parser.add_argument("--shard", default="1/1", help="run the I-th of N slices of the list")
    parser.add_argument("--limit", type=int, help="test only the first N papers of the shard")
    parser.add_argument("--only", nargs="*", help="test only these paper IDs")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--keep", action="store_true", help="keep the folders of failed papers for debugging")
    parser.add_argument("--merge", nargs="+", type=Path, metavar="RESULTS",
                        help="combine results.json files from several shards instead of testing")
    parser.add_argument("--fetch-only", action="store_true", help="only download the sources into the cache")
    args = parser.parse_args()
    known = {p[0]: (p[1] if len(p) > 1 else "") for p in read_list(HERE / "known-issues.txt")}
    if args.merge:
        results = [r for path in args.merge for r in json.loads(path.read_text(encoding="utf-8"))]
        return report(results, known, args)
    args.cache = args.cache.resolve()
    papers = read_list(args.papers)
    if args.fetch_only:
        return fetch_all(papers, args.cache)
    if not args.quire:
        sys.exit("quire not found; pass --quire")
    args.quire = args.quire.resolve()
    args.work = args.work.resolve()

    if args.only:
        papers = [p for p in papers if p[0] in args.only] or [[p, "?"] for p in args.only]
    index, count = map(int, args.shard.split("/"))
    papers = papers[index - 1::count][: args.limit]

    def one(entry):
        paper, category = entry[0], (entry[1] if len(entry) > 1 else "?")
        result = test_paper(paper, category, args)
        detail = result.get("reason") if result["class"] != "ok" else f"{result.get('build_s')}s"
        print(f"{result['class']:>8}  {paper:<16} {detail}", flush=True)
        return result

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        results = list(pool.map(one, papers))
    args.out.write_text(json.dumps(results, indent=1), encoding="utf-8")
    report(results, known, args)


def fetch_all(papers, cache):
    """Download every source, one at a time, so the test shards need no network."""
    failed = []
    for i, entry in enumerate(papers, 1):
        cached = (cache / f"{entry[0]}.eprint").exists()
        try:
            fetch(entry[0], cache)
        except Exception as e:
            failed.append(entry[0])
            print(f"  {entry[0]}: {e}", flush=True)
        if not cached and i % 25 == 0:
            print(f"{i}/{len(papers)} fetched", flush=True)
    print(f"{len(papers) - len(failed)} of {len(papers)} sources cached; {len(failed)} failed")


def report(results, known, args):
    summary = summarize(results, known)
    if args.summary:
        args.summary.write_text(summary, encoding="utf-8")
    print()
    print(summary)
    new = [r["id"] for r in results if r["class"] == "quire" and r["id"] not in known]
    if new:
        print(f"{len(new)} new quire failure(s): {' '.join(new)}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
