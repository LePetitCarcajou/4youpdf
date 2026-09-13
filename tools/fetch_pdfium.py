#!/usr/bin/env python3
"""Fetch the PDFium shared library used by the desktop application to draw
pages (ADR 0005). The build from bblanchon/pdfium-binaries (PDFium:
BSD-3-Clause; the notice of the build itself: MIT) lands in app/pdfium/,
ignored by Git, with the licences of PDFium and of the libraries built into
it (app/pdfium/licenses/), which the installer and the portable archive
ship along with the library (tools/package_app.py). The application looks
there in development, next to its executable once installed, or in the
directory named by FYP_PDFIUM_DIR.

The release is pinned below, with the SHA-256 of each of its archives:
an archive that changed on the release page stops the script. Bump both
deliberately.
"""

import hashlib
import io
import os
import platform
import shutil
import sys
import tarfile
import urllib.request

RELEASE = "chromium/8044"
# SHA-256 of the archives of that release, as downloaded when it was pinned.
SHA256 = {
    "pdfium-win-x64.tgz": "78a17d9a5f14467631c26a3ac8741b27a0471ecc05bd6a119b523598160a0537",
    "pdfium-win-arm64.tgz": "6c9ac0ddc69edd8a18d47b95098a5b843eaed5c5bbdcb9587a18c196457449f8",
    "pdfium-linux-x64.tgz": "eb142f416aed3a72fc5a02dbd5884868a16cb99dc0cf53e6bdd64afbf67b05f4",
    "pdfium-linux-arm64.tgz": "e98400ef5f005f27cfba5c14f72d464e25187298f04950de46646033cf24cef0",
    "pdfium-mac-x64.tgz": "a93d44238e05de20028446561b951d50988b849efbbe56fe40c0d376c05b45e8",
    "pdfium-mac-arm64.tgz": "61424884d4a7f153b808deba6437848e4400834ce30aaf95d3050da44df8f420",
}
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.path.join(ROOT, "app", "pdfium")


def asset():
    system = platform.system()
    machine = platform.machine().lower()
    arch = "arm64" if machine in ("arm64", "aarch64") else "x64"
    if system == "Windows":
        return f"pdfium-win-{arch}.tgz", "bin/pdfium.dll"
    if system == "Darwin":
        return f"pdfium-mac-{arch}.tgz", "lib/libpdfium.dylib"
    return f"pdfium-linux-{arch}.tgz", "lib/libpdfium.so"


def main():
    name, member = asset()
    url = f"https://github.com/bblanchon/pdfium-binaries/releases/download/{RELEASE}/{name}"
    print(f"PDFium {RELEASE}: {url}")
    data = urllib.request.urlopen(url, timeout=600).read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SHA256[name]:
        sys.exit(f"{name}: SHA-256 {digest}, expected {SHA256[name]}")
    os.makedirs(TARGET, exist_ok=True)
    # The licences of this release only, not those of an earlier one.
    shutil.rmtree(os.path.join(TARGET, "licenses"), ignore_errors=True)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        found = False
        for entry in tar.getmembers():
            path = entry.name.lstrip("./")
            if not entry.isfile():
                continue
            if path == member:
                entry.name = os.path.basename(member)
                tar.extract(entry, TARGET)
                found = True
            elif path in ("LICENSE", "VERSION"):
                entry.name = path
                tar.extract(entry, TARGET)
            elif path.startswith("licenses/"):
                entry.name = "licenses/" + os.path.basename(path)
                tar.extract(entry, TARGET)
        if not found:
            sys.exit(f"{member} not found in {name}")
    with open(os.path.join(TARGET, "RELEASE"), "w", encoding="utf-8") as f:
        f.write(RELEASE + "\n")
    print(f"  -> {os.path.join(TARGET, os.path.basename(member))}")


if __name__ == "__main__":
    main()
