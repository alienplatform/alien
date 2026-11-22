#!/usr/bin/env python3
"""Select the Rust packages a pull request can affect, for ci-fast.yml.

Usage: affected-rust-packages.py <base-sha> <head-sha> [--own-only]
       affected-rust-packages.py --run-repo-wide

Prints GitHub Actions outputs (key=value lines):
  mode       "affected", "full", or "none" (only documentation changed)
  packages   cargo package arguments, e.g. "-p alien-cli -p alien-manager"
  features   the CI feature list, limited to selected packages
  core       "true" when alien-core is selected (the image contract test)
  agent      "true" when alien-sandbox-agent is selected (the privileged tests)
  count      number of selected packages
  test_filter  nextest filterset limiting the selected packages to the tests that count
               (own-only mode with dependents; empty otherwise)
  repo_wide    names of the repository-wide checks the pull request runs (--run-repo-wide)
  ts_filters   pnpm filters of the TypeScript packages whose tests the change can break

With --own-only (the pull request tier) the outputs describe the changed packages alone,
without their dependents. A change outside the packages (Cargo.lock, say) does not drop the
changed packages' tests; with no changed package it reports mode "check-only": the caller
compiles everything but tests nothing. Ready pull requests run without the flag and tests
the dependents too.

A package is affected when one of its files changed, or when it depends (normally, as a
dev-dependency or as a build-dependency) on an affected package. A change to any file
outside the workspace packages selects every package ("full"), unless it is documentation:
manifests, the lockfile, toolchain and CI config, Dockerfiles, scripts, examples and the
TypeScript packages can all change what Rust tests see. Every selection also includes
alien-client-core, so its `no-retry-delay` feature can be turned on.

With --own-only, scripts/ci-checks.json adds two things (each entry says why it exists):
  - `dependents`: a changed package also selects named test targets of its direct dependents
    that check its output (golden, snapshot, compatibility, permission rendering), not their
    whole suites. alien-core is deliberately absent: most packages depend on it; a pull request
    with the `ci:full` label runs everything instead.
  - `typescript`: the TypeScript packages whose tests read the changed files (their own
    directory, their workspace dependencies, and listed extra inputs such as client-sdks/).
`--run-repo-wide` runs the `repo_wide` checks of that file, the tests that read files outside
their own package; the pull request tier runs them on every pull request.

Packages under `examples/` and `tests/` are fixtures: other packages' tests read their files
(for example the CLI checks every example's template.toml), so a change there selects "full".
"""

import json
import subprocess
import sys
from pathlib import Path, PurePosixPath

# Features the full workspace run enables; each applies only if its package is selected.
# `no-retry-delay` keeps mocked tests from waiting between client retries.
# `alien-preflights/runtime-checks`: its tests call the deployment-time checks; in the full run
# the feature comes from the packages that depend on it, so a selection without them needs it.
CI_FEATURES = [
    "alien-manager/openapi",
    "alien-bindings/platform-sdk",
    "alien-client-core/no-retry-delay",
    "alien-preflights/runtime-checks",
]

# Paths outside the packages that cannot change a Rust build or test.
IGNORED_PREFIXES = ("docs/", ".claude/", ".agents/")
IGNORED_SUFFIXES = (".md", ".mdx")
# Fixture packages whose files other packages' tests read.
FIXTURE_PREFIXES = ("examples/", "tests/")


import time
def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


CONFIG_PATH = PurePosixPath(__file__).with_name("ci-checks.json")


def load_config() -> dict:
    with open(CONFIG_PATH) as handle:
        return json.load(handle)


def load_ts_packages(root: PurePosixPath) -> dict[str, dict]:
    """The pnpm workspace's packages: name -> {dir, deps (workspace dependencies)}."""
    packages: dict[str, dict] = {}
    manifests = sorted(Path(root).glob("packages/*/package.json"))
    manifests += sorted(Path(root).glob("client-sdks/*/typescript/package.json"))
    for manifest in manifests:
        data = json.loads(manifest.read_text())
        deps = {
            name
            for key in ("dependencies", "devDependencies", "peerDependencies")
            for name, spec in data.get(key, {}).items()
            if str(spec).startswith("workspace:")
        }
        packages[data["name"]] = {
            "dir": PurePosixPath(manifest.parent.relative_to(root)),
            "deps": deps,
        }
    return packages


def select_typescript(changed_files, ts_packages, config) -> list[str]:
    """Test packages whose inputs changed: own directory, workspace dependencies (transitive), extras."""
    wanted = config["typescript"]["test_packages"]
    extras = config["typescript"]["extra_inputs"]

    def inputs(name: str) -> set[str]:
        seen: set[str] = set()
        pending = [name]
        while pending:
            current = pending.pop()
            if current in seen or current not in ts_packages:
                continue
            seen.add(current)
            pending.extend(ts_packages[current]["deps"])
        return seen

    shared = config["typescript"]["every_package_inputs"]["paths"]
    if any(path in shared for path in changed_files):
        return [name for name in wanted if name in ts_packages]

    selected = []
    for name in wanted:
        if name not in ts_packages:
            continue
        directories = [ts_packages[dep]["dir"] for dep in inputs(name)]
        extra = extras.get(name, {})
        for path in changed_files:
            posix = PurePosixPath(path)
            if (
                any(posix.is_relative_to(directory) for directory in directories)
                or any(path.startswith(prefix) for prefix in extra.get("paths", []))
                or any(path.endswith(suffix) for suffix in extra.get("suffixes", []))
            ):
                selected.append(name)
                break
    return selected


def select(changed_files, packages, dependents, config, own_only, ts_packages=None):
    """Return the selection as a dict of the outputs, plus `reason` for the log."""
    # Longest directory first, so a package nested inside another is matched before its parent.
    by_depth = sorted(packages.items(), key=lambda item: len(item[1].parts), reverse=True)
    changed: set[str] = set()
    full_reason = None
    for path in changed_files:
        if path.startswith(IGNORED_PREFIXES) or path.endswith(IGNORED_SUFFIXES):
            continue
        if path.startswith(FIXTURE_PREFIXES):
            full_reason = full_reason or path
            continue
        owner = next(
            (name for name, directory in by_depth if PurePosixPath(path).is_relative_to(directory)),
            None,
        )
        if owner is not None:
            changed.add(owner)
        else:
            full_reason = full_reason or path

    test_filter = ""
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

    if own_only:
        # Changed crates keep their own tests even when another file needs a workspace compile check.
        selected = set(changed)
        mode = "affected" if selected else ("check-only" if full_reason is not None else "none")
        # The named test targets of direct dependents that check a changed package's output.
        targets: dict[str, list[str]] = {}
        for name in sorted(changed):
            for entry in config["dependents"].get(name, []):
                if entry["package"] not in changed and entry["package"] in packages:
                    targets.setdefault(entry["package"], []).append(entry["filter"])
        if targets:
            selected |= set(targets)
            terms = [f"package({name})" for name in sorted(changed)]
            terms += [
                f"(package({name}) & ({' | '.join(f'({f})' for f in filters)}))"
                for name, filters in sorted(targets.items())
            ]
            test_filter = " | ".join(terms)
    if selected:
        # Cargo takes a feature only of a selected package or of one's direct dependency, and
        # most packages reach alien-client-core through others.
        selected.add("alien-client-core")
    features = [feature for feature in CI_FEATURES if feature.split("/")[0] in selected]
    ts = select_typescript(changed_files, ts_packages, config) if (own_only and ts_packages) else []
    reason = (
        f"{full_reason} needs the whole workspace"
        if full_reason
        else f"{len(changed)} changed package(s)"
    )
    return {
        "mode": mode,
        "packages": " ".join(f"-p {name}" for name in sorted(selected)),
        "features": ",".join(features),
        "core": "true" if "alien-core" in selected else "false",
        "agent": "true" if "alien-sandbox-agent" in selected else "false",
        "count": len(selected),
        "test_filter": test_filter,
        "ts_filters": " ".join(f"--filter {name}" for name in ts),
        # The checks of `--run-repo-wide`: every pull request that changes more than documentation.
        "repo_wide": "" if mode == "none" else ";".join(check["name"] for check in config["repo_wide"]),
        "reason": reason,
    }


def run_repo_wide(config) -> int:
    root = PurePosixPath(__file__).parent.parent
    failed = []
    total = time.monotonic()
    for check in config["repo_wide"]:
        start = time.monotonic()
        result = subprocess.run(check["run"], cwd=Path(root) / check["cwd"])
        seconds = time.monotonic() - start
        print(f"repo-wide check {check['name']!r}: exit {result.returncode} in {seconds:.1f}s", flush=True)
        if result.returncode != 0:
            failed.append(check["name"])
    print(f"repo-wide checks: {len(config['repo_wide'])} run in {time.monotonic() - total:.1f}s, failed: {failed or 'none'}")
    return 1 if failed else 0


def main() -> None:
    config = load_config()
    if sys.argv[1] == "--run-repo-wide":
        sys.exit(run_repo_wide(config))
    base, head = sys.argv[1], sys.argv[2]
    own_only = "--own-only" in sys.argv[3:]
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
    result = select(changed_files, packages, dependents, config, own_only, load_ts_packages(root))
    reason = result.pop("reason")
    for key, value in result.items():
        print(f"{key}={value}")
    print(
        f"Rust test selection: {result['mode']}, {result['count']} of {len(packages)} packages ({reason})",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
