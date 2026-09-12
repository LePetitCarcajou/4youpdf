#!/usr/bin/env python3
"""Fetch the PDFium shared library used by the desktop application to draw
page thumbnails (ADR 0005). The build from bblanchon/pdfium-binaries
(PDFium: BSD-3-Clause; build scripts: Apache-2.0) lands in app/pdfium/,
ignored by Git, with its licence. The application looks there in
development, next to its executable once installed, or in the directory
named by FYP_PDFIUM_DIR.

The release is pinned below; bump it deliberately.
"""

import io
import os
import platform
import sys
import tarfile
import urllib.request

RELEASE = "chromium/8044"
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
    os.makedirs(TARGET, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        found = False
        for entry in tar.getmembers():
            if entry.name.lstrip("./") == member:
                entry.name = os.path.basename(member)
                tar.extract(entry, TARGET)
                found = True
            elif entry.name.lstrip("./") in ("LICENSE", "VERSION"):
                entry.name = os.path.basename(entry.name)
                tar.extract(entry, TARGET)
        if not found:
            sys.exit(f"{member} not found in {name}")
    with open(os.path.join(TARGET, "RELEASE"), "w", encoding="utf-8") as f:
        f.write(RELEASE + "\n")
    print(f"  -> {os.path.join(TARGET, os.path.basename(member))}")


if __name__ == "__main__":
    main()
