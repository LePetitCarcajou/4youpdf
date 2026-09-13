#!/usr/bin/env python3
"""Download the public PDF test suites into tests/corpus/.

Each suite (a "lot") is one directory under tests/corpus/, filled from a
pinned commit of its upstream repository. Only *.pdf files under the chosen
sub-directory are kept, plus the upstream licence files. A `.source.json`
marker records where the lot comes from, and tests/corpus/SOURCES.md is
regenerated from the markers so that the provenance of every file is
documented.

Idempotent: a lot whose marker exists is not downloaded again. `--update`
resolves the upstream branch again and downloads only if its commit moved.

Standard library only. Set GITHUB_TOKEN to raise the API rate limit.
"""

import argparse
import datetime as dt
import io
import json
import os
import shutil
import sys
import tarfile
import urllib.error
import urllib.request
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "tests" / "corpus"
MARKER = ".source.json"
USER_AGENT = "4youpdf-fetch-corpus (https://github.com/LePetitCarcajou/4youpdf)"

# Suites downloaded from GitHub. `ref` None means the default branch.
SOURCES = [
    {
        "lot": "pdfjs",
        "repo": "mozilla/pdf.js",
        "ref": None,
        "subdir": "test/pdfs",
        "license": "Apache-2.0",
        "description": (
            "Suite de test du lecteur pdf.js : plusieurs centaines de fichiers "
            "réels et pathologiques. Les fichiers `.link` (documents externes "
            "que pdf.js ne redistribue pas) sont ignorés."
        ),
    },
    {
        "lot": "qpdf",
        "repo": "qpdf/qpdf",
        "ref": None,
        "subdir": "qpdf/qtest/qpdf",
        "license": "Apache-2.0",
        "description": (
            "Suite de test de qpdf : xref streams, object streams, fichiers "
            "chiffrés, mises à jour incrémentales, fichiers volontairement "
            "cassés."
        ),
    },
    {
        "lot": "verapdf",
        "repo": "veraPDF/veraPDF-corpus",
        "ref": None,
        "subdir": "",
        "license": "CC-BY-4.0",
        "description": (
            "Corpus veraPDF : PDF/A (1b à 4f) et PDF/UA valides et invalides, "
            "un fichier par règle, plus les fichiers de la suite Isartor et "
            "des cas ISO 32000-1/2. Licence indiquée dans le README du dépôt."
        ),
    },
]

# Suites that cannot be fetched automatically.
MANUAL = [
    {
        "name": "Ghent Output Suite",
        "note": (
            "PDF/X et impression. Distribuée par le Ghent Workgroup (gwg.org) "
            "sous ses propres conditions : à télécharger à la main et à placer "
            "dans `tests/corpus/ghent/` ; ce dossier est ignoré par Git comme "
            "les autres lots."
        ),
    },
]

# Top-level files copied into `<lot>/_upstream/`. README is included because
# some corpora (veraPDF) state their licence there and nowhere else.
LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "readme")


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def github_json(url):
    headers = {"User-Agent": USER_AGENT, "Accept": "application/vnd.github+json"}
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)


def resolve(source):
    """Return (ref, sha, commit date) for the configured ref of a source."""
    repo = source["repo"]
    ref = source["ref"]
    if ref is None:
        ref = github_json(f"https://api.github.com/repos/{repo}")["default_branch"]
    commit = github_json(f"https://api.github.com/repos/{repo}/commits/{ref}")
    return ref, commit["sha"], commit["commit"]["committer"]["date"]


class CountingReader(io.RawIOBase):
    """Wrap the HTTP response to report progress while tarfile streams it."""

    def __init__(self, inner, label):
        self.inner = inner
        self.label = label
        self.count = 0
        self.reported = 0

    def readable(self):
        return True

    def readinto(self, b):
        data = self.inner.read(len(b))
        n = len(data)
        b[:n] = data
        self.count += n
        if self.count - self.reported >= 16 * 1024 * 1024:
            self.reported = self.count
            log(f"  {self.label}: {self.count // (1024 * 1024)} Mio reçus…")
        return n


def wanted(member, subdir):
    """Relative destination path for a tar member, or None to skip it."""
    if not member.isfile():
        return None
    parts = PurePosixPath(member.name).parts
    if len(parts) < 2 or any(p in ("..", "") for p in parts):
        return None
    rel = PurePosixPath(*parts[1:])  # drop the `repo-sha/` root
    if len(rel.parts) == 1 and rel.name.lower().startswith(LICENSE_PREFIXES):
        return PurePosixPath("_upstream") / rel.name
    if subdir:
        base = PurePosixPath(subdir)
        if rel.parts[: len(base.parts)] != base.parts:
            return None
        rel = PurePosixPath(*rel.parts[len(base.parts):])
        if not rel.parts:
            return None
    if rel.suffix.lower() != ".pdf":
        return None
    return rel


def download(source, sha, dest):
    """Stream the tarball of `sha` and extract the wanted files into `dest`."""
    url = f"https://codeload.github.com/{source['repo']}/tar.gz/{sha}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    files = 0
    size = 0
    with urllib.request.urlopen(req, timeout=120) as resp:
        reader = io.BufferedReader(CountingReader(resp, source["lot"]), 1024 * 1024)
        with tarfile.open(fileobj=reader, mode="r|gz") as tar:
            for member in tar:
                rel = wanted(member, source["subdir"])
                if rel is None:
                    continue
                target = dest.joinpath(*rel.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                extracted = tar.extractfile(member)
                if extracted is None:
                    continue
                with open(target, "wb") as out:
                    shutil.copyfileobj(extracted, out)
                if rel.suffix.lower() == ".pdf":
                    files += 1
                    size += member.size
    return files, size


def read_marker(lot_dir):
    try:
        with open(lot_dir / MARKER, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def fetch(source, update):
    lot_dir = CORPUS / source["lot"]
    marker = read_marker(lot_dir)
    if marker and not update:
        log(f"{source['lot']}: déjà présent ({marker['files']} fichiers, "
            f"{source['repo']}@{marker['sha'][:10]}) ; --update pour vérifier en amont")
        return marker
    ref, sha, commit_date = resolve(source)
    if marker and marker.get("sha") == sha:
        log(f"{source['lot']}: à jour ({source['repo']}@{sha[:10]})")
        return marker
    log(f"{source['lot']}: téléchargement de {source['repo']}@{sha[:10]} ({ref})…")
    part = CORPUS / (source["lot"] + ".part")
    if part.exists():
        shutil.rmtree(part)
    part.mkdir(parents=True)
    files, size = download(source, sha, part)
    marker = {
        "lot": source["lot"],
        "repo": source["repo"],
        "url": f"https://github.com/{source['repo']}",
        "ref": ref,
        "sha": sha,
        "commit_date": commit_date,
        "subdir": source["subdir"],
        "license": source["license"],
        "fetched": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d"),
        "files": files,
        "bytes": size,
    }
    with open(part / MARKER, "w", encoding="utf-8") as f:
        json.dump(marker, f, indent=2, ensure_ascii=False)
        f.write("\n")
    if lot_dir.exists():
        shutil.rmtree(lot_dir)
    part.rename(lot_dir)
    log(f"{source['lot']}: {files} fichiers PDF, {size / (1024 * 1024):.1f} Mio")
    return marker


def write_sources_md():
    lines = [
        "# Provenance du corpus",
        "",
        "Généré par `tools/fetch_corpus.py` ; ne pas modifier à la main. Les lots",
        "téléchargés vivent dans des sous-dossiers de `tests/corpus/` ignorés par",
        "Git : chaque poste les récupère avec le script. Seuls les fichiers `*.pdf`",
        "sont conservés, avec les fichiers de licence du dépôt d'origine dans",
        "`<lot>/_upstream/`.",
        "",
        "| Lot | Dépôt | Sous-dossier | Commit | Date du commit | Licence | Fichiers | Téléchargé le |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for source in SOURCES:
        lot_dir = CORPUS / source["lot"]
        marker = read_marker(lot_dir)
        if marker:
            pdf_count = sum(1 for p in lot_dir.rglob("*") if p.suffix.lower() == ".pdf")
            lines.append(
                f"| `{source['lot']}/` | [{marker['repo']}]({marker['url']}) | "
                f"`{marker['subdir'] or '/'}` | `{marker['sha'][:12]}` ({marker['ref']}) | "
                f"{marker['commit_date'][:10]} | {marker['license']} | "
                f"{pdf_count} | {marker['fetched']} |"
            )
        else:
            lines.append(
                f"| `{source['lot']}/` | [{source['repo']}](https://github.com/{source['repo']}) | "
                f"`{source['subdir'] or '/'}` | non téléchargé | — | {source['license']} | — | — |"
            )
    lines += ["", "## Description des lots", ""]
    for source in SOURCES:
        lines.append(f"- **{source['lot']}** — {source['description']}")
    lines += ["", "## Suites à importer à la main", ""]
    for entry in MANUAL:
        lines.append(f"- **{entry['name']}** — {entry['note']}")
    lines += [
        "",
        "## Licences",
        "",
        "Les fichiers sont utilisés comme données de test, sans modification et sans",
        "redistribution par ce dépôt. Apache-2.0 et CC BY 4.0 l'autorisent avec",
        "mention de la provenance : c'est le rôle de ce fichier et des copies de",
        "licence placées dans chaque lot.",
        "",
    ]
    (CORPUS / "SOURCES.md").write_text("\n".join(lines), encoding="utf-8", newline="\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--only",
        action="append",
        choices=[s["lot"] for s in SOURCES],
        help="ne traiter que ce lot (répétable)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="interroger le dépôt amont et retélécharger si son commit a changé",
    )
    parser.add_argument("--list", action="store_true", help="lister les lots et sortir")
    args = parser.parse_args()
    # Consoles that cannot encode accented letters must not abort the download.
    sys.stderr.reconfigure(errors="replace")

    if args.list:
        for source in SOURCES:
            marker = read_marker(CORPUS / source["lot"])
            state = f"{marker['files']} fichiers @{marker['sha'][:10]}" if marker else "absent"
            print(f"{source['lot']:10} {source['repo']:28} {source['license']:12} {state}")
        return 0

    CORPUS.mkdir(parents=True, exist_ok=True)
    failures = 0
    for source in SOURCES:
        if args.only and source["lot"] not in args.only:
            continue
        try:
            fetch(source, args.update)
        except (urllib.error.URLError, OSError, tarfile.TarError, KeyError) as e:
            failures += 1
            log(f"{source['lot']}: échec : {e}")
    write_sources_md()
    log(f"SOURCES.md mis à jour ({CORPUS / 'SOURCES.md'})")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
