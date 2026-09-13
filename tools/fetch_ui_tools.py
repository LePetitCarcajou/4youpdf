#!/usr/bin/env python3
"""Fetch the standalone binaries that build and test the interface, without
Node.

- esbuild (MIT): bundles app/ui/src/main.ts into app/dist/main.js, and each
  test of app/ui/tests/.
- tsgo, the native TypeScript compiler preview (Apache-2.0): type-checks
  the sources (`tsc --noEmit` equivalent).
- QuickJS-ng (MIT): a small JavaScript engine that runs the tests of the
  interface logic, which need no DOM (tools/build_ui.py).

esbuild and tsgo come from the npm registry as platform packages that hold a
single executable; QuickJS-ng from its GitHub release, checked against the
SHA-256 pinned below. They land in app/.tools/ (ignored by Git) with their
licences. Versions are pinned below; run again after changing them.
"""

import hashlib
import io
import os
import platform
import sys
import tarfile
import urllib.request

ESBUILD_VERSION = "0.28.2"
TSGO_VERSION = "7.0.0-dev.20260707.2"
QUICKJS_VERSION = "0.16.2"
# SHA-256 of the executables of the QuickJS-ng release, as published with it.
QUICKJS_SHA256 = {
    "qjs-windows-x86_64.exe": "7b27412de844403545bd151fbe49191b4d5b91a9e15b5db7c863fea54639a82b",
    "qjs-linux-x86_64": "c5e1b16adfa36def7ac523d6ba54edc77ef66a4dfd65d73e6eae19025f9b7b0a",
    "qjs-linux-aarch64": "5fb05fd4e81f26c0039f7166ed9af1050968a0e252c981c461a6aa3376244e6b",
    "qjs-darwin-x86_64": "4448991c0500dbe40c7b2f91ba39275995413aa4ee59db3b513b68350908a413",
    "qjs-darwin-arm64": "f6200e9856c45578a5d42ac873a32f3f994b421e29df9f63b452d9c7145015fc",
}

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


def fetch_quickjs():
    """One executable per platform, checked against its pinned SHA-256, and
    the licence of the tagged sources. Windows on ARM runs the x86-64 one."""
    system = platform.system()
    arm = platform.machine().lower() in ("arm64", "aarch64")
    if system == "Windows":
        asset, exe = "qjs-windows-x86_64.exe", ".exe"
    elif system == "Darwin":
        asset, exe = ("qjs-darwin-arm64" if arm else "qjs-darwin-x86_64"), ""
    else:
        asset, exe = ("qjs-linux-aarch64" if arm else "qjs-linux-x86_64"), ""
    tag = f"v{QUICKJS_VERSION}"
    url = f"https://github.com/quickjs-ng/quickjs/releases/download/{tag}/{asset}"
    print(f"quickjs-ng {QUICKJS_VERSION}: {url}")
    data = urllib.request.urlopen(url, timeout=300).read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != QUICKJS_SHA256[asset]:
        sys.exit(f"{asset}: SHA-256 {digest}, expected {QUICKJS_SHA256[asset]}")
    path = os.path.join(TOOLS, "qjs" + exe)
    with open(path, "wb") as f:
        f.write(data)
    if system != "Windows":
        os.chmod(path, 0o755)
    licence = f"https://raw.githubusercontent.com/quickjs-ng/quickjs/{tag}/LICENSE"
    with open(os.path.join(TOOLS, "LICENSE-quickjs.txt"), "wb") as f:
        f.write(urllib.request.urlopen(licence, timeout=60).read())
    print(f"  -> {path}")


def main():
    os.makedirs(TOOLS, exist_ok=True)
    suffix, exe = platform_suffix()
    fetch_esbuild(suffix, exe)
    fetch_tsgo(suffix, exe)
    fetch_quickjs()


if __name__ == "__main__":
    main()
