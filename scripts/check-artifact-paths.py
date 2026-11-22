#!/usr/bin/env python3
"""Fail when an artifact upload or cache step could include env or key files.

Reads every workflow in .github/workflows/ and every composite action in
.github/actions/. An `actions/upload-artifact` or `actions/cache` step fails when
a `path` entry matches a sensitive file pattern, or when an upload sets
`include-hidden-files` to anything but false without an allow-list entry.

Allow list: .github/artifact-allowlist.txt, one `workflow:step-name | reason`
per line. Keep this file identical in every repo that carries it.

Usage: python3 -I scripts/check-artifact-paths.py [--root DIR] [--allowlist FILE]
"""

import argparse
import fnmatch
import sys
from pathlib import Path

import yaml

# Matched case-insensitively against every component of a path entry.
PATH_PATTERNS = [
    ".env",
    ".env.*",
    "*.env",
    "*kubeconfig*",
    ".kube",
    "*.pem",
    "*.p12",
    "*key*.json",
    "*credentials*",
    "*.tfstate*",
    "id_rsa*",
]


# Whole-checkout or home-directory paths include every file below them.
BROAD_PATHS = {"", "*", "**", "~", "$home", "${{ github.workspace }}", "$github_workspace"}


def is_broad(entry):
    text = entry.replace("\\", "/").lower()
    while text.startswith("./"):
        text = text[2:]
    text = text.rstrip("/")
    if text == ".":
        text = ""
    return text in BROAD_PATHS or text.removesuffix("/**").removesuffix("/*") in BROAD_PATHS


def path_findings(value):
    found = []
    for raw in str(value).splitlines():
        entry = raw.strip()
        if not entry or entry.startswith("!"):
            continue
        if is_broad(entry):
            found.append(f"path `{entry}` includes the whole checkout or home directory")
            continue
        for part in entry.replace("\\", "/").split("/"):
            for pattern in PATH_PATTERNS:
                if fnmatch.fnmatchcase(part.lower(), pattern):
                    found.append(f"path `{entry}` matches `{pattern}`")
                    break
            else:
                continue
            break
    return found


def load_allowlist(path):
    allowed = set()
    if not path.is_file():
        return allowed
    for number, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, reason = line.partition("|")
        if not sep or not reason.strip() or ":" not in key:
            raise SystemExit(f"{path}:{number}: expected `workflow:step-name | reason`")
        allowed.add(key.strip())
    return allowed


def steps_of(document):
    if not isinstance(document, dict):
        return
    for job in (document.get("jobs") or {}).values():
        if isinstance(job, dict):
            yield from job.get("steps") or []
    runs = document.get("runs")
    if isinstance(runs, dict):
        yield from runs.get("steps") or []


def check_file(path, label, allowed):
    problems = []
    document = yaml.safe_load(path.read_text())
    for index, step in enumerate(steps_of(document), 1):
        if not isinstance(step, dict):
            continue
        uses = str(step.get("uses") or "")
        is_upload = uses.startswith("actions/upload-artifact")
        if not (is_upload or uses.startswith("actions/cache")):
            continue
        name = str(step.get("name") or f"step-{index}")
        key = f"{label}:{name}"
        with_ = step.get("with") or {}
        found = path_findings(with_.get("path", ""))
        hidden = str(with_.get("include-hidden-files", "false")).strip().lower()
        if is_upload and hidden != "false":
            found.append("sets `include-hidden-files`")
        if found and key not in allowed:
            problems.extend(f"{key}: {item}" for item in found)
    return problems


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=str(Path(__file__).resolve().parent.parent))
    parser.add_argument("--allowlist")
    args = parser.parse_args()
    root = Path(args.root)
    allowed = load_allowlist(Path(args.allowlist) if args.allowlist else root / ".github/artifact-allowlist.txt")

    files = sorted((root / ".github/workflows").glob("*.y*ml"))
    files += sorted((root / ".github/actions").glob("*/action.y*ml"))
    problems = []
    for path in files:
        label = path.name if path.parent.name == "workflows" else f"actions/{path.parent.name}/{path.name}"
        problems += check_file(path, label, allowed)

    if problems:
        print("Artifact or cache steps that could include env or key files:")
        for problem in problems:
            print(f"  {problem}")
        print("Upload only the files that are needed. If a step is reviewed and safe, add")
        print("`workflow:step-name | reason` to .github/artifact-allowlist.txt.")
        return 1
    print(f"checked {len(files)} workflow and action files: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
