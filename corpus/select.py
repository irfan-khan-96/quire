#!/usr/bin/env python3
"""Pick the arXiv papers for the corpus test and write them to papers.txt.

Queries the arXiv API for papers submitted in a fixed window, a few per
category, so the list covers many communities and their LaTeX habits. The
output is committed; rerun only to refresh the corpus.

    python corpus/select.py [--per-category N] [--window 202501010000:202506302359]
"""

import argparse
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
API = "https://export.arxiv.org/api/query"
USER_AGENT = "quire-corpus/0.1 (+https://github.com/irfan-khan-96/quire)"
ATOM = "{http://www.w3.org/2005/Atom}"

# One or two categories from every major archive, plus a few large CS ones.
CATEGORIES = [
    "math.AP", "math.CO", "math.AG", "math.PR", "math.NT",
    "cs.LG", "cs.CL", "cs.CV", "cs.DS", "cs.LO", "cs.CR",
    "hep-th", "hep-ph", "hep-ex", "gr-qc", "nucl-th",
    "astro-ph.GA", "astro-ph.SR", "cond-mat.str-el", "cond-mat.mtrl-sci",
    "quant-ph", "physics.optics", "physics.flu-dyn",
    "q-bio.NC", "q-fin.ST", "stat.ME", "econ.EM", "eess.SP", "nlin.CD",
]


def query(category, window, count):
    start, end = window.split(":")
    params = {
        "search_query": f"cat:{category} AND submittedDate:[{start} TO {end}]",
        "sortBy": "submittedDate",
        "sortOrder": "ascending",
        "max_results": str(count),
    }
    url = f"{API}?{urllib.parse.urlencode(params)}"
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        feed = ET.fromstring(response.read())
    ids = []
    for entry in feed.iter(f"{ATOM}entry"):
        # http://arxiv.org/abs/2501.01234v2 -> 2501.01234v2 (versions are pinned)
        ids.append(entry.find(f"{ATOM}id").text.rsplit("/abs/", 1)[1])
    return ids


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--per-category", type=int, default=10)
    parser.add_argument("--window", default="202501010000:202506302359")
    parser.add_argument("--out", type=Path, default=HERE / "papers.txt")
    args = parser.parse_args()

    seen = set()
    lines = [
        "# arXiv papers for the corpus test, written by corpus/select.py.",
        f"# Window {args.window}, {args.per_category} per category. Format: <id> <category>",
    ]
    for i, category in enumerate(CATEGORIES):
        if i:
            time.sleep(3)  # arXiv asks for at most one API request every three seconds
        for paper in query(category, args.window, args.per_category):
            if paper not in seen:
                seen.add(paper)
                lines.append(f"{paper} {category}")
        print(f"{category}: {len(seen)} papers so far")
    args.out.write_text("\n".join(lines) + "\n")
    print(f"wrote {len(seen)} papers to {args.out}")


if __name__ == "__main__":
    main()
