#!/usr/bin/env python3
"""Select the Rust packages a pull request can affect, for ci-fast.yml.

Usage: affected-rust-packages.py <base-sha> <head-sha>

Prints GitHub Actions outputs (key=value lines):
  mode       "affected", "full", or "none" (only documentation changed)
  packages   cargo package arguments, e.g. "-p alien-cli -p alien-manager"
  features   the CI feature list, limited to selected packages
  core       "true" when alien-core is selected (the image contract test)
  agent      "true" when alien-sandbox-agent is selected (the privileged tests)
  count      number of selected packages

A package is affected when one of its files changed, or when it depends (normally, as a
dev-dependency or as a build-dependency) on an affected package. A change to any file
outside the workspace packages selects every package ("full"), unless it is documentation:
manifests, the lockfile, toolchain and CI config, Dockerfiles, scripts, examples and the
TypeScript packages can all change what Rust tests see.

Packages under `examples/` and `tests/` are fixtures: other packages' tests read their files
(for example the CLI checks every example's template.toml), so a change there selects "full".
"""

import json
import subprocess
import sys
from pathlib import PurePosixPath

# Features the full workspace run enables; each applies only if its package is selected.
CI_FEATURES = ["alien-manager/openapi", "alien-bindings/platform-sdk"]

# Paths outside the packages that cannot change a Rust build or test.
IGNORED_PREFIXES = ("docs/", ".claude/", ".agents/")
IGNORED_SUFFIXES = (".md", ".mdx")
# Fixture packages whose files other packages' tests read.
FIXTURE_PREFIXES = ("examples/", "tests/")


def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def main() -> None:
    base, head = sys.argv[1], sys.argv[2]
    metadata = json.loads(run("cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"))
    root = PurePosixPath(metadata["workspace_root"])
    packages = {
        package["name"]: PurePosixPath(package["manifest_path"]).parent.relative_to(root)
        for package in metadata["packages"]
    }

    dependents: dict[str, set[str]] = {name: set() for name in packages}
    for package in metadata["packages"]:
        for dependency in package["dependencies"]:
            if dependency["name"] in packages:
                dependents[dependency["name"]].add(package["name"])

    changed_files = [line for line in run("git", "diff", "--name-only", base, head).splitlines() if line]

    # Longest directory first, so a package nested inside another is matched before its parent.
    by_depth = sorted(packages.items(), key=lambda item: len(item[1].parts), reverse=True)
    changed: set[str] = set()
    full_reason = None
    for path in changed_files:
        if path.startswith(IGNORED_PREFIXES) or path.endswith(IGNORED_SUFFIXES):
            continue
        if path.startswith(FIXTURE_PREFIXES):
            full_reason = path
            break
        owner = next(
            (name for name, directory in by_depth if PurePosixPath(path).is_relative_to(directory)),
            None,
        )
        if owner is not None:
            changed.add(owner)
        else:
            full_reason = path
            break

    if full_reason is not None:
        selected = set(packages)
        mode = "full"
    else:
        selected = set()
        pending = list(changed)
        while pending:
            name = pending.pop()
            if name in selected:
                continue
            selected.add(name)
            pending.extend(dependents[name] - selected)
        mode = "affected" if selected else "none"

    features = [feature for feature in CI_FEATURES if feature.split("/")[0] in selected]
    print(f"mode={mode}")
    print(f"packages={' '.join(f'-p {name}' for name in sorted(selected))}")
    print(f"features={','.join(features)}")
    print(f"core={'true' if 'alien-core' in selected else 'false'}")
    print(f"agent={'true' if 'alien-sandbox-agent' in selected else 'false'}")
    print(f"count={len(selected)}")
    reason = (
        f"{full_reason} needs the whole workspace"
        if full_reason
        else f"{len(changed)} changed package(s)"
    )
    print(f"Rust test selection: {mode}, {len(selected)} of {len(packages)} packages ({reason})", file=sys.stderr)


if __name__ == "__main__":
    main()
