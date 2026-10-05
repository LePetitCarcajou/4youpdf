#!/usr/bin/env python3
"""Time of a request for a page, from end to end, before and after the
rendering worker (ADR 0008, « Mesures »): v0.5.0, where PDFium ran on a
thread of the window, against this checkout, where it runs in a process of
its own.

Both sides run the same loop in a release build: page 1 of each file, as a
thumbnail (160 pixels wide) and as a page of the view (1400), asked
`--requests` times of one service, each request timed from the call of
`RenderService::render` to the PNG. The baseline is a `git archive v0.5.0`
tree outside this repository, to which
`tools/render_timing/baseline_v0_5_0.rs` is added as an example of
`fyp-app`; this checkout runs `app/examples/render_timing.rs` with the
release `fyp-app` as the worker. The two take turns, `--rounds` times, and
the medians are those of all the requests of a side.

    python tools/fetch_pdfium.py
    python tools/fetch_corpus.py
    python tools/render_timing.py

The two files of the corpus measured by default come from pdf.js at
`fd453c2ce3e1` (`tests/corpus/SOURCES.md`); `fetch_corpus.py` takes the
default branch, so another commit may change or drop them.

The criterion (ADR 0008, « Mesures »): where drawing dominates, a baseline
median over `--dominant` milliseconds (100), a median at most `--fail-above`
percent over it (15); elsewhere, at most `--fail-above-ms` milliseconds
over it (10). Exit status 1 when a median fails it. Times hold for the
machine they were taken on.
"""

import argparse
import os
import shutil
import statistics
import subprocess
import sys

sys.dont_write_bytecode = True  # no __pycache__ left in the repository

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TAG = "v0.5.0"
EXE = ".exe" if os.name == "nt" else ""
FILES = [
    "tests/fixtures/mixed12.pdf",
    "tests/corpus/pdfjs/22060_A1_01_Plans.pdf",
    "tests/corpus/pdfjs/issue3188.pdf",
]


def run(command, cwd, capture=False):
    done = subprocess.run(command, cwd=cwd, text=True, stdout=subprocess.PIPE if capture else None)
    if done.returncode != 0:
        sys.exit(f"{' '.join(command)}: exit status {done.returncode}")
    return done.stdout


def baseline_tree(path):
    """The tree of the tag, with the harness as an example of fyp-app."""
    if not os.path.isdir(path):
        os.makedirs(path)
        archive = subprocess.Popen(["git", "archive", TAG], cwd=ROOT, stdout=subprocess.PIPE)
        subprocess.run(["tar", "-x", "-C", path], stdin=archive.stdout, check=True)
        if archive.wait() != 0:
            sys.exit(f"git archive {TAG} failed")
    examples = os.path.join(path, "app", "examples")
    os.makedirs(examples, exist_ok=True)
    shutil.copyfile(
        os.path.join(ROOT, "tools", "render_timing", "baseline_v0_5_0.rs"),
        os.path.join(examples, "render_timing.rs"),
    )


def samples(output):
    """{(file, width): [milliseconds]} from the lines of a harness."""
    found = {}
    for line in output.splitlines():
        file, width, *times = line.split("\t")
        found[(os.path.basename(file), int(width))] = [float(t) for t in times]
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("files", nargs="*", help="PDF files, instead of the three of the ADR")
    parser.add_argument("--baseline", default=os.path.join(os.path.dirname(ROOT), f"4YouPDF-{TAG}"),
                        help="where the tree of the tag is, or is extracted")
    parser.add_argument("--requests", type=int, default=30)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--dominant", type=float, default=100.0, metavar="MS",
                        help="baseline median above which drawing dominates")
    parser.add_argument("--fail-above", type=float, default=15.0, metavar="PERCENT",
                        help="where drawing dominates: the most a median may grow")
    parser.add_argument("--fail-above-ms", type=float, default=10.0, metavar="MS",
                        help="elsewhere: the most a median may grow")
    args = parser.parse_args()

    pdfium = os.path.join(ROOT, "app", "pdfium")
    if not os.path.isdir(pdfium):
        sys.exit("app/pdfium missing: python tools/fetch_pdfium.py")
    files = [os.path.abspath(f) for f in args.files] or [os.path.join(ROOT, f) for f in FILES]
    baseline_tree(args.baseline)
    build = ["cargo", "build", "--release", "-p", "fyp-app", "--example", "render_timing"]
    run(build, args.baseline)
    run(build + ["--bin", "fyp-app"], ROOT)

    release = os.path.join("target", "release")
    harness = os.path.join(release, "examples", "render_timing" + EXE)
    before_command = [os.path.join(args.baseline, harness), pdfium, str(args.requests)] + files
    after_command = [os.path.join(ROOT, harness), os.path.join(ROOT, release, "fyp-app" + EXE), pdfium,
                     str(args.requests)] + files
    before, after = {}, {}
    for _ in range(args.rounds):
        for command, into in ((before_command, before), (after_command, after)):
            for key, times in samples(run(command, ROOT, capture=True)).items():
                into.setdefault(key, []).extend(times)

    print(f"{'file':32} {'width':>5} {'n':>4} {TAG + ' ms':>10} {'now ms':>10} {'change':>8} {'ms':>8}  criterion")
    failed = []
    for key in before:
        a, b = statistics.median(before[key]), statistics.median(after[key])
        change = (b - a) / a * 100
        if a > args.dominant:
            criterion, ok = f"<= +{args.fail_above:g}%", change <= args.fail_above
        else:
            criterion, ok = f"<= +{args.fail_above_ms:g} ms", b - a <= args.fail_above_ms
        if not ok:
            failed.append(key)
        print(f"{key[0]:32} {key[1]:5} {len(after[key]):4} {a:10.2f} {b:10.2f} {change:+7.1f}% {b - a:+8.2f}  "
              f"{criterion} {'ok' if ok else 'FAILED'}")
    if failed:
        sys.exit(f"{len(failed)} median(s) over the criterion")


if __name__ == "__main__":
    main()
