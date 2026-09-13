#!/usr/bin/env python3
"""Package the desktop application for Windows: the NSIS installer and the
portable archive, both with PDFium and the licences (ADR 0005).

    python tools/fetch_ui_tools.py      # once
    python tools/fetch_pdfium.py        # once per PDFium release
    python tools/package_app.py [--tag vX.Y.Z] [--out DIR]

1. The interface is type-checked, tested and bundled (tools/build_ui.py):
   a package never embeds a stale app/dist/.
2. The Tauri CLI, pinned below, is built from crates.io with its lockfile
   into app/.tools/tauri-cli/ the first time (a few minutes).
3. `cargo tauri build` merges app/tauri.bundle.json into app/tauri.conf.json:
   bundling on, and the files that exist only once fetched (pdfium.dll,
   licences). Out comes
   target/release/bundle/nsis/4YouPDF_<version>_<arch>-setup.exe.
4. The same executable and the same files, under a 4YouPDF/ folder with an
   empty data/ folder that makes the copy portable (app/src/main.rs), are
   zipped into
   target/release/bundle/portable/4YouPDF_<version>_<arch>_portable.zip.

The versions are checked before anything is built, by tools/check_version.py
(the application takes the version of the workspace), and --tag stops there
too unless it names that version: the release workflow passes the Git tag,
after running the same check in its own job. --out copies both files into
DIR. The SHA-256 of each is printed. Windows only for now: macOS and Linux
come next.
"""

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import zipfile

sys.dont_write_bytecode = True  # no tools/__pycache__/ in the checkout
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_version  # noqa: E402  (tools/, next to this script)
import fetch_pdfium  # noqa: E402

TAURI_CLI_VERSION = "2.11.4"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = os.path.join(ROOT, "app")
CLI_ROOT = os.path.join(APP, ".tools", "tauri-cli")
BUNDLE_CONFIG = "tauri.bundle.json"


def run(*args, cwd=ROOT):
    print("$ " + " ".join(args), flush=True)
    subprocess.run(list(args), cwd=cwd, check=True)


def target_directory():
    """The target directory of Cargo."""
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        encoding="utf-8",
    ).stdout
    return json.loads(out)["target_directory"]


def tauri_cli():
    """The pinned Tauri CLI, built into app/.tools/tauri-cli/ if missing."""
    exe = os.path.join(CLI_ROOT, "bin", "cargo-tauri.exe")
    wanted = f"tauri-cli {TAURI_CLI_VERSION}"
    if os.path.exists(exe):
        found = subprocess.run(
            [exe, "--version"], check=True, capture_output=True, encoding="utf-8"
        ).stdout.strip()
        if found == wanted:
            return exe
        print(f"{found} in {CLI_ROOT}, {wanted} wanted")
    run(
        "cargo", "install", "tauri-cli", "--version", f"={TAURI_CLI_VERSION}",
        "--locked", "--force", "--root", CLI_ROOT,
    )
    return exe


def resources():
    """(source, target) of each file shipped next to the executable, as
    app/tauri.bundle.json maps them; a source ending in / is a directory
    whose files go under the target directory."""
    release = os.path.join(APP, "pdfium", "RELEASE")
    found = None
    if os.path.exists(release):
        with open(release, encoding="utf-8") as f:
            found = f.read().strip()
    if found != fetch_pdfium.RELEASE:
        sys.exit(f"app/pdfium/ holds PDFium {found}, {fetch_pdfium.RELEASE} wanted: run tools/fetch_pdfium.py")
    with open(os.path.join(APP, BUNDLE_CONFIG), encoding="utf-8") as f:
        mapping = json.load(f)["bundle"]["resources"]
    files = []
    for source, target in mapping.items():
        path = os.path.normpath(os.path.join(APP, source))
        if source.endswith("/"):
            if not os.path.isdir(path):
                sys.exit(f"{path} missing: run tools/fetch_pdfium.py")
            for folder, _, names in os.walk(path):
                for name in sorted(names):
                    full = os.path.join(folder, name)
                    files.append((full, target + os.path.relpath(full, path).replace(os.sep, "/")))
        elif os.path.isfile(path):
            files.append((path, target))
        else:
            sys.exit(f"{path} missing: run tools/fetch_pdfium.py")
    return files


def portable(exe, files, archive, folder):
    """Zip `exe`, `files` and an empty data/ folder under `folder`/."""
    os.makedirs(os.path.dirname(archive), exist_ok=True)
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.write(exe, f"{folder}/{os.path.basename(exe)}")
        for source, target in files:
            zf.write(source, f"{folder}/{target}")
        data = zipfile.ZipInfo(f"{folder}/data/")
        data.external_attr = (0o40755 << 16) | 0x10  # a directory, for Unix and for Windows
        zf.writestr(data, b"")


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description="Package the desktop application for Windows.")
    parser.add_argument("--tag", help="Git tag of the release, which must be v<version of the workspace>")
    parser.add_argument("--out", help="directory to copy the installer and the portable archive into")
    args = parser.parse_args()
    if platform.system() != "Windows":
        sys.exit("packaging is Windows-only for now")
    try:
        if args.tag is None:
            version = check_version.workspace_version()
        else:
            version = check_version.check_tag(args.tag)
    except check_version.VersionError as e:
        sys.exit(f"version check failed: {e}")
    target = target_directory()
    files = resources()
    with open(os.path.join(APP, "tauri.conf.json"), encoding="utf-8") as f:
        config = json.load(f)
    product, binary = config["productName"], config["mainBinaryName"]
    arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x64"

    run(sys.executable, os.path.join(ROOT, "tools", "build_ui.py"))
    run(tauri_cli(), "build", "--ci", "--bundles", "nsis", "--config", BUNDLE_CONFIG, cwd=APP)

    release = os.path.join(target, "release")
    installer = os.path.join(release, "bundle", "nsis", f"{product}_{version}_{arch}-setup.exe")
    if not os.path.isfile(installer):
        sys.exit(f"{installer} missing after the build")
    archive = os.path.join(release, "bundle", "portable", f"{product}_{version}_{arch}_portable.zip")
    portable(os.path.join(release, binary + ".exe"), files, archive, product)

    outputs = [installer, archive]
    if args.out is not None:
        os.makedirs(args.out, exist_ok=True)
        outputs = [shutil.copy2(path, args.out) for path in outputs]
    for path in outputs:
        print(f"{sha256(path)}  {path}")


if __name__ == "__main__":
    main()
