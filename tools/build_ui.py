#!/usr/bin/env python3
"""Type-check and bundle the interface of the desktop application.

app/ui/src/*.ts  --tsgo (type check)--> nothing
app/ui/src/main.ts --esbuild (bundle)--> app/dist/main.js
app/ui/index.html, app/ui/styles.css  --copy-->  app/dist/

Needs the binaries fetched by tools/fetch_ui_tools.py. Pass --check to
type-check only, --no-check to skip the type check. Tauri embeds app/dist/
at compile time: rebuild the application afterwards (`cargo build -p
fyp-app` notices the change).
"""

import os
import platform
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = os.path.join(ROOT, "app")
UI = os.path.join(APP, "ui")
DIST = os.path.join(APP, "dist")
TOOLS = os.path.join(APP, ".tools")
EXE = ".exe" if platform.system() == "Windows" else ""


def tool(*parts):
    path = os.path.join(TOOLS, *parts) + EXE
    if not os.path.exists(path):
        sys.exit(f"{path} missing: run tools/fetch_ui_tools.py first")
    return path


def main():
    check_only = "--check" in sys.argv
    if "--no-check" not in sys.argv:
        print("type check (tsgo)")
        subprocess.run(
            [tool("tsgo", "lib", "tsgo"), "-p", os.path.join(UI, "tsconfig.json"), "--noEmit"],
            check=True,
        )
    if check_only:
        return
    os.makedirs(DIST, exist_ok=True)
    print("bundle (esbuild)")
    subprocess.run(
        [
            tool("esbuild"),
            os.path.join(UI, "src", "main.ts"),
            "--bundle",
            "--format=iife",
            "--target=es2022",
            "--charset=utf8",
            "--log-level=warning",
            "--outfile=" + os.path.join(DIST, "main.js"),
        ],
        check=True,
    )
    for name in ("index.html", "styles.css"):
        shutil.copyfile(os.path.join(UI, name), os.path.join(DIST, name))
    print(f"-> {DIST}")


if __name__ == "__main__":
    main()
