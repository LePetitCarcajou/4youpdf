#!/usr/bin/env python3
"""Check the versions of the workspace, and that a release tag names it.

    python tools/check_version.py [--tag vX.Y.Z]
    python tools/check_version.py --rust-version [CRATE]

Two lines coexist (CLAUDE.md), for the version of the crates as for their
minimum Rust:

- The product crates, PRODUCT below, take the version of [workspace.package]
  in Cargo.toml, and the entries of [workspace.dependencies] that name them
  require exactly that version. fyp-plugin-api and fyp-host keep their own,
  the contract of the modules being versioned apart (ADR 0003): they are not
  checked here.
- Every member of the workspace takes [workspace.package] rust-version,
  except the crates of OWN_RUST_VERSION: the contract keeps the lowest Rust
  it builds with, for the authors of modules.

Without argument, checks the versions and prints the version of the
workspace. With --tag, also fails unless the tag is v<version>. The release
workflow runs it before building anything (job version-check);
tools/package_app.py imports it and applies the same check before packaging.

With --rust-version, checks the rule of the rust-versions and prints the
minimum Rust of the product, or that of CRATE: the CI jobs msrv-product and
msrv-plugin-api install exactly that toolchain.

Standard library only (tomllib, Python 3.11 or later): Cargo is not needed.
"""

import argparse
import os
import re
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Name and folder of each crate that takes the version of the workspace.
PRODUCT = {
    "fyp-core": "crates/fyp-core",
    "fyp-crypto": "crates/fyp-crypto",
    "fyp-conformance": "crates/fyp-conformance",
    "fyp-cli": "crates/fyp-cli",
    "fyp-app": "app",
}

# Crates that give a rust-version of their own; every other member of the
# workspace takes that of the workspace.
OWN_RUST_VERSION = {"fyp-plugin-api"}

# What Cargo accepts as a rust-version: a bare version of two or three parts.
# Checked here too, since the CI puts the value in a shell and an environment.
RUST_VERSION = re.compile(r"\d+\.\d+(\.\d+)?")


class VersionError(Exception):
    """The versions of the workspace disagree, or the tag does not name them."""


def _manifest(path):
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise VersionError(f"{path}: {e}") from e


def workspace_version(root=ROOT):
    """[workspace.package] version, once every product crate is checked to
    take it."""
    workspace = _manifest(os.path.join(root, "Cargo.toml")).get("workspace", {})
    version = workspace.get("package", {}).get("version")
    if not isinstance(version, str):
        raise VersionError("Cargo.toml: [workspace.package] gives no version")
    dependencies = workspace.get("dependencies", {})
    for name, folder in PRODUCT.items():
        package = _manifest(os.path.join(root, folder, "Cargo.toml")).get("package", {})
        if package.get("name") != name:
            raise VersionError(f"{folder}/Cargo.toml: package {package.get('name')!r}, {name!r} expected")
        if package.get("version") != {"workspace": True}:
            raise VersionError(
                f"{folder}/Cargo.toml: {name} must take the version of the workspace "
                "(version.workspace = true)"
            )
        entry = dependencies.get(name)
        if isinstance(entry, dict) and "version" in entry and entry["version"] != version:
            raise VersionError(
                f"Cargo.toml: [workspace.dependencies] requires {name} {entry['version']}, "
                f"the workspace is at {version}"
            )
    return version


def check_tag(tag, root=ROOT):
    """The version of the workspace, if `tag` is v<that version>."""
    version = workspace_version(root)
    if tag != f"v{version}":
        raise VersionError(f"tag {tag} does not name the version of the workspace: v{version} expected")
    return version


def rust_version(crate=None, root=ROOT):
    """The minimum Rust of the product (`crate` None) or of `crate`, once
    every member of the workspace is checked: the crates of OWN_RUST_VERSION
    give their own, all the others take [workspace.package] rust-version."""
    workspace = _manifest(os.path.join(root, "Cargo.toml")).get("workspace", {})
    product = workspace.get("package", {}).get("rust-version")
    if not isinstance(product, str) or not RUST_VERSION.fullmatch(product):
        raise VersionError(f"Cargo.toml: [workspace.package] rust-version {product!r} is not a version")
    own = {}
    for folder in workspace.get("members", []):
        package = _manifest(os.path.join(root, folder, "Cargo.toml")).get("package", {})
        name, value = package.get("name"), package.get("rust-version")
        if name in OWN_RUST_VERSION:
            if not isinstance(value, str) or not RUST_VERSION.fullmatch(value):
                raise VersionError(f"{folder}/Cargo.toml: {name} must give its own rust-version, not {value!r}")
            own[name] = value
        elif value != {"workspace": True}:
            raise VersionError(
                f"{folder}/Cargo.toml: {name} must take the rust-version of the workspace "
                "(rust-version.workspace = true)"
            )
    if crate is None:
        return product
    if crate not in own:
        raise VersionError(f"{crate} gives no rust-version of its own: only {', '.join(sorted(OWN_RUST_VERSION))} do")
    return own[crate]


def main():
    parser = argparse.ArgumentParser(description="Check the versions of the workspace and of a release tag.")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--tag", help="Git tag of the release, which must be v<version of the workspace>")
    mode.add_argument(
        "--rust-version",
        nargs="?",
        const="",
        metavar="CRATE",
        help="print the minimum Rust of the product, or of CRATE, once the rule of the rust-versions is checked",
    )
    args = parser.parse_args()
    try:
        if args.rust_version is not None:
            print(rust_version(args.rust_version or None))
        elif args.tag is not None:
            print(check_tag(args.tag))
        else:
            print(workspace_version())
    except VersionError as e:
        sys.exit(f"version check failed: {e}")


if __name__ == "__main__":
    main()
