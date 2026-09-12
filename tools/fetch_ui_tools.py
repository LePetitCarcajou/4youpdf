#!/usr/bin/env python3
"""Fetch the two standalone binaries that build the interface, without Node.

- esbuild (MIT): bundles app/ui/src/main.ts into app/dist/main.js.
- tsgo, the native TypeScript compiler preview (Apache-2.0): type-checks
  the sources (`tsc --noEmit` equivalent).

Both come from the npm registry as platform packages that hold a single
executable; they land in app/.tools/ (ignored by Git) with their licences.
Versions are pinned below; run again after changing them.
"""

import io
import os
import platform
import sys
import tarfile
import urllib.request

ESBUILD_VERSION = "0.28.2"
TSGO_VERSION = "7.0.0-dev.20260707.2"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOLS = os.path.join(ROOT, "app", ".tools")


def platform_suffix():
    system = platform.system()
    machine = platform.machine().lower()
    arch = "arm64" if machine in ("arm64", "aarch64") else "x64"
    if system == "Windows":
        return "win32-" + arch, ".exe"
    if system == "Darwin":
        return "darwin-" + arch, ""
    return "linux-" + arch, ""


def download(package, version):
    name = package.split("/")[1]
    url = f"https://registry.npmjs.org/{package}/-/{name}-{version}.tgz"
    print(f"{package} {version}: {url}")
    return urllib.request.urlopen(url, timeout=300).read()


def fetch_esbuild(suffix, exe):
    """The esbuild package holds one executable: keep it and its licence."""
    data = download(f"@esbuild/{suffix}", ESBUILD_VERSION)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        found = False
        for member in tar.getmembers():
            base = os.path.basename(member.name)
            if base == "esbuild" + exe:
                member.name = base
                tar.extract(member, TOOLS)
                found = True
            elif base.upper().startswith("LICENSE"):
                member.name = "LICENSE-esbuild.txt"
                tar.extract(member, TOOLS)
        if not found:
            sys.exit("esbuild: no executable in the archive")
    path = os.path.join(TOOLS, "esbuild" + exe)
    if platform.system() != "Windows":
        os.chmod(path, 0o755)
    print(f"  -> {path}")


def fetch_tsgo(suffix, exe):
    """tsgo reads its `lib.*.d.ts` next to its executable: keep the whole
    package under app/.tools/tsgo/ (the executable is lib/tsgo)."""
    data = download(f"@typescript/native-preview-{suffix}", TSGO_VERSION)
    target = os.path.join(TOOLS, "tsgo")
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        members = []
        for member in tar.getmembers():
            if member.name.startswith("package/") and len(member.name) > len("package/"):
                member.name = member.name[len("package/") :]
                members.append(member)
        tar.extractall(target, members=members)
    path = os.path.join(target, "lib", "tsgo" + exe)
    if not os.path.exists(path):
        sys.exit("tsgo: no lib/tsgo executable in the archive")
    if platform.system() != "Windows":
        os.chmod(path, 0o755)
    print(f"  -> {path}")


def main():
    os.makedirs(TOOLS, exist_ok=True)
    suffix, exe = platform_suffix()
    fetch_esbuild(suffix, exe)
    fetch_tsgo(suffix, exe)


if __name__ == "__main__":
    main()
