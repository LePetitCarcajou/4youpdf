#!/usr/bin/env python3
"""Build the modules of plugins/ for the WebAssembly sandbox.

plugins/<name>/  (a crate with manifest.toml)
  --cargo build --target wasm32-wasip1 --release-->  target/.../<package>.wasm
  --copy-->  plugins/<name>/module.wasm

fyp-host loads module.wasm next to manifest.toml: `fyp run`, and the
fyp-host and fyp-cli tests that use the real modules. The copies are
ignored by Git: rerun this script after changing a module or fyp-core.
"""

import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PLUGINS = os.path.normcase(os.path.join(ROOT, "plugins"))
TARGET = "wasm32-wasip1"


def module_crates(metadata):
    """(package name, directory) of each crate directly under plugins/ that has a manifest."""
    for package in metadata["packages"]:
        directory = os.path.dirname(package["manifest_path"])
        parent = os.path.normcase(os.path.dirname(os.path.abspath(directory)))
        if parent == PLUGINS and os.path.isfile(os.path.join(directory, "manifest.toml")):
            yield package["name"], directory


def main():
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    modules = sorted(module_crates(metadata))
    if not modules:
        sys.exit(f"no module crate found under {PLUGINS}")
    for name, directory in modules:
        print(f"build {name} ({TARGET})")
        subprocess.run(
            ["cargo", "build", "-p", name, "--bin", name, "--target", TARGET, "--release"],
            cwd=ROOT,
            check=True,
        )
        built = os.path.join(metadata["target_directory"], TARGET, "release", name + ".wasm")
        if not os.path.isfile(built):
            sys.exit(f"{built} missing: the crate needs a binary named after its package")
        destination = os.path.join(directory, "module.wasm")
        shutil.copyfile(built, destination)
        print(f"-> {destination} ({os.path.getsize(destination)} bytes)")


if __name__ == "__main__":
    main()
