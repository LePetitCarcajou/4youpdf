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
workspace. With --tag, also checks that the tag is a release tag of that
workspace, under the two kinds of release of docs/paliers.md:

- a rampe adds features and takes the next minor version. Its tag ends in
  .0, and it must name the version of the workspace exactly: v0.4.0 on a
  workspace at 0.4.0, and nothing else.
- a palier adds nothing: it settles the debt of the rampe it follows and
  takes the next patch. The version of the workspace may stay where that
  rampe left it, so the tag has only to share its major and its minor, with
  a patch that does not go back below that of the workspace: v0.3.4 passes
  on a workspace at 0.3.3 as on one at 0.3.4, v0.3.1 and v0.5.1 do not.

The tags up to v0.3.3 came before this method (docs/paliers.md): their
patches above 0 are not paliers.

Either way the value printed is the version of the workspace, never that of
the tag, because that is the version the build carries: a package names its
files with it (tools/package_app.py) and tauri-build writes it into the
properties of the executable. A palier tag above the workspace therefore
ships files that do not carry its number. The release workflow runs the
check before building anything (job version-check); tools/package_app.py
imports it and applies the same check before packaging.

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

# A version, and the tag that names a release of it: three plain numbers,
# no pre-release suffix and no leading zero. The workflow triggers on every
# `v*` tag, so this is the only gate: a tag the rule of check_tag cannot
# classify (v0.4, v0.4.0-rc1, v0.3.04) is refused rather than guessed at,
# that rule turning on the patch alone.
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")


class VersionError(Exception):
    """The versions of the workspace disagree, or the tag is not a release
    tag of them."""


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


def _numbers(version, what):
    """The three numbers of `version`, which `what` names in any error."""
    match = VERSION.fullmatch(version)
    if match is None:
        raise VersionError(f"{what} {version!r} is not a version of three plain numbers")
    return tuple(int(part) for part in match.groups())


def check_tag(tag, root=ROOT):
    """The version of the workspace, if `tag` names a release of it.

    Which rule applies is read off the patch of the tag (docs/paliers.md):
    a patch of 0 is the tag of a rampe, which adds features, so the
    workspace carries the version its tag names and the two must be equal;
    any other patch is the tag of a palier, which adds nothing, so the
    workspace may still carry the version of the rampe that palier follows
    and the tag has only to share its major and minor without going back
    below its patch.

    Returns the version of the workspace, not that of the tag: see the
    module docstring on what a package names its files with.
    """
    version = workspace_version(root)
    workspace = _numbers(version, "Cargo.toml: [workspace.package] version")
    if not tag.startswith("v") or VERSION.fullmatch(tag[1:]) is None:
        raise VersionError(
            f"tag {tag} is not of the form vMAJOR.MINOR.PATCH, with three plain numbers: "
            "the rule of docs/paliers.md reads the patch of the tag, and this project "
            "gives a release no other kind of tag"
        )
    named = _numbers(tag[1:], "tag")
    if named[2] == 0:
        # A rampe: it added the features of this version, so the workspace
        # carries it and the tag names it, both parts of the same bump.
        if named != workspace:
            raise VersionError(
                f"tag {tag} ends in .0, so it is the tag of a rampe (docs/paliers.md), "
                f"which adds features: the workspace must carry the version such a tag "
                f"names, and [workspace.package] version is {version}, whose tag is "
                f"v{version}"
            )
    else:
        # A palier: it added nothing, so the workspace may still carry the
        # version of the rampe it follows. Same series, and no going back.
        if named[:2] != workspace[:2]:
            raise VersionError(
                f"tag {tag} has a patch above 0, so it is the tag of a palier "
                f"(docs/paliers.md), which settles the debt of the rampe "
                f"v{named[0]}.{named[1]}.0: its major and its minor must be those of "
                f"the workspace, {workspace[0]}.{workspace[1]}, not "
                f"{named[0]}.{named[1]} ([workspace.package] version is {version})"
            )
        if named[2] < workspace[2]:
            raise VersionError(
                f"tag {tag} has a patch above 0, so it is the tag of a palier "
                f"(docs/paliers.md): its patch may stay ahead of the workspace, which "
                f"a palier does not have to advance, but never go back below it, and "
                f"[workspace.package] version is {version}"
            )
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
    mode.add_argument(
        "--tag",
        help="Git tag of the release: vX.Y.0 names the version of the workspace exactly, "
        "vX.Y.Z above it shares its major and minor without going back (docs/paliers.md)",
    )
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
