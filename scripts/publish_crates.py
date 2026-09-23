#!/usr/bin/env python3
"""Publish the tagged workspace version; never hide a failed cargo publish."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parent.parent
# Runtime dependencies must be published before their consumers. Dev-only
# workspace cycles are stripped by Cargo when they have no registry version.
CRATES = (
    "mixtape-anthropic-sdk",
    "mixtape-core",
    "mixtape-tools",
    "mixtape-cli",
    "mixtape-server",
    "mixtape-acp",
)


def validate_workspace(version):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise RuntimeError("Expected a crate version without the leading v")
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    members = set(metadata["workspace_members"])
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in members
    }
    if set(packages) != set(CRATES):
        raise RuntimeError("Publish order must include every workspace crate exactly once")
    published = set()
    for name in CRATES:
        package = packages[name]
        if package["version"] != version:
            raise RuntimeError(f"{name} version does not match release tag {version}")
        if package["publish"] is not None and "crates-io" not in package["publish"]:
            raise RuntimeError(f"{name} is not enabled for crates.io publication")
        for dependency in package["dependencies"]:
            dependency_name = dependency["name"]
            if dependency["kind"] != "dev" and dependency_name in packages:
                if dependency_name not in published:
                    raise RuntimeError(f"Publish {dependency_name} before {name}")
        published.add(name)


def version_exists(crate, version):
    request = Request(
        f"https://crates.io/api/v1/crates/{crate}/{version}",
        headers={
            "User-Agent": "mixtape-release (https://github.com/adlio/mixtape)",
            "Accept": "application/json",
        },
    )
    try:
        with urlopen(request, timeout=30) as response:
            data = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return False
        raise RuntimeError(f"crates.io lookup failed for {crate}: HTTP {error.code}") from error
    except (URLError, TimeoutError, ValueError) as error:
        raise RuntimeError(f"Could not verify crates.io version for {crate}") from error
    record = data.get("version") if isinstance(data, dict) else None
    if not isinstance(record, dict) or record.get("crate") != crate or record.get("num") != version:
        raise RuntimeError(f"Unexpected crates.io version response for {crate}")
    if record.get("yanked") is not False:
        raise RuntimeError(f"Refusing to skip yanked or unverified {crate}@{version}")
    return True


def publish(version):
    validate_workspace(version)
    for crate in CRATES:
        if version_exists(crate, version):
            print(f"{crate}@{version} already exists on crates.io; skipping", flush=True)
            continue
        print(f"Publishing {crate}@{version}", flush=True)
        subprocess.run(
            ["cargo", "publish", "--locked", "--registry", "crates-io", "-p", crate],
            cwd=ROOT,
            check=True,
        )
        # Allow registry/index propagation before a dependent crate is packaged.
        time.sleep(30)
        if not version_exists(crate, version):
            raise RuntimeError(f"Published {crate}@{version} is not visible on crates.io yet")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="Workspace version, e.g. 0.5.0")
    arguments = parser.parse_args()
    try:
        publish(arguments.version)
    except (RuntimeError, subprocess.CalledProcessError, ValueError, KeyError) as error:
        parser.exit(1, f"Release failed: {error}\n")
